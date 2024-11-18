use core::str;
use std::{
    cmp::Ordering,
    collections::VecDeque,
    net::SocketAddr,
    num::NonZeroU32,
    process::Stdio,
    sync::{Arc, Mutex},
};

use common::{
    lunasim::{FromLunasim, FromLunasimbot},
    LunabotStage,
};
use crossbeam::atomic::AtomicCell;
use gputter::{
    init_gputter_blocking,
    types::{AlignedMatrix4, AlignedVec4},
};
use lunabot_ai::{run_ai, Action, Input, PollWhen};
use nalgebra::{Isometry3, UnitQuaternion, UnitVector3, Vector2, Vector3, Vector4};
use serde::{Deserialize, Serialize};
use thalassic::DepthProjectorBuilder;
use urobotics::tokio;
use urobotics::{
    app::Application,
    callbacks::caller::CallbacksStorage,
    define_callbacks, fn_alias, get_tokio_handle,
    log::{error, warn},
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        process::{ChildStdin, Command},
        runtime::Handle,
    },
    BlockOn,
};

use crate::{
    localization::Localizer,
    pipelines::thalassic::{spawn_thalassic_pipeline, PointsStorageChannel},
};

use super::{create_packet_builder, create_robot_chain, log_teleop_messages, wait_for_ctrl_c};

fn_alias! {
    pub type FromLunasimRef = CallbacksRef(FromLunasim) + Send
}
define_callbacks!(FromLunasimCallbacks => CloneFn(msg: FromLunasim) + Send);

#[derive(Clone)]
pub struct LunasimStdin(Arc<Mutex<ChildStdin>>);

impl LunasimStdin {
    pub fn write(&self, bytes: &[u8]) {
        self.0.clear_poison();
        let mut stdin = self.0.lock().unwrap();
        if let Err(e) = stdin
            .write_all(&u32::to_ne_bytes(bytes.len() as u32))
            .block_on()
        {
            if e.kind() != std::io::ErrorKind::BrokenPipe {
                error!("Failed to send to lunasim: {e}");
            }
            return;
        }
        if let Err(e) = stdin.write_all(bytes).block_on() {
            if e.kind() != std::io::ErrorKind::BrokenPipe {
                error!("Failed to send to lunasim: {e}");
            }
        }
    }
}

#[cfg(target_os = "windows")]
const DELIMIT: &[u8] = b"READY\r\n";

#[cfg(not(target_os = "windows"))]
const DELIMIT: &[u8] = b"READY\n";

#[derive(Serialize, Deserialize)]
pub struct LunasimbotApp {
    pub lunabase_address: SocketAddr,
    #[serde(default = "super::default_max_pong_delay_ms")]
    pub max_pong_delay_ms: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    simulation_command: Vec<String>,
}

impl Application for LunasimbotApp {
    const APP_NAME: &'static str = "sim";

    const DESCRIPTION: &'static str = "The lunabot application in a simulated environment";

    fn run(mut self) {
        log_teleop_messages();
        if let Err(e) = init_gputter_blocking() {
            error!("Failed to initialize gputter: {e}");
        }

        let mut cmd = if self.simulation_command.is_empty() {
            let mut cmd = Command::new("godot");
            cmd.args(["--path", "godot/lunasim"]);
            cmd
        } else {
            let mut cmd = Command::new(self.simulation_command.remove(0));
            cmd.args(self.simulation_command);
            cmd
        };

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let _guard = get_tokio_handle().enter();
        let child;
        let (lunasim_stdin, from_lunasim_ref) = match cmd.spawn() {
            Ok(tmp) => {
                child = tmp;
                let stdin = child.stdin.unwrap();
                let mut stdout = child.stdout.unwrap();
                let mut stderr = child.stderr.unwrap();
                macro_rules! handle_err {
                    ($msg: literal, $err: ident) => {{
                        match $err.kind() {
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::Other | std::io::ErrorKind::UnexpectedEof => {}
                            _ => {
                                error!(target: "lunasim", "Faced the following error while reading {}: {}", $msg, $err);
                            }
                        }
                        break;
                    }}
                }

                // Log stderr from Lunasim
                let handle = Handle::current();
                handle.spawn(async move {
                    let mut bytes = Vec::with_capacity(1024);
                    let mut buf = [0u8; 1024];

                    loop {
                        if bytes.len() == bytes.capacity() {
                            bytes.reserve(bytes.len());
                        }
                        match stderr.read(&mut buf).await {
                            Ok(0) => {}
                            Ok(n) => {
                                bytes.extend_from_slice(&buf[0..n]);
                                if let Ok(string) = std::str::from_utf8(&bytes) {
                                    if let Some(i) = string.find('\n') {
                                        warn!(target: "lunasim", "{}", &string[0..i]);
                                        bytes.drain(0..=i);
                                    }
                                }
                            }
                            Err(e) => handle_err!("stderr", e),
                        }
                    }
                });

                let mut callbacks = FromLunasimCallbacks::default();
                let callbacks_ref = callbacks.get_ref();

                // Read lunasim stdout
                handle.spawn(async move {
                    {
                        let mut bytes = VecDeque::with_capacity(DELIMIT.len());
                        let mut buf = [0u8];
                        loop {
                            match stdout.read_exact(&mut buf).await {
                                Ok(0) => unreachable!("godot program should not exit"),
                                Ok(_) => {
                                    bytes.push_back(buf[0]);
                                }
                                Err(e) => error!(target: "lunasim", "Faced the following error while reading stdout: {e}"),
                            }
                            match bytes.len().cmp(&DELIMIT.len()) {
                                Ordering::Equal => {}
                                Ordering::Greater => {bytes.pop_front();}
                                Ordering::Less => continue,
                            }
                            if bytes == DELIMIT {
                                break;
                            }
                        }
                    }
                    let mut size_buf = [0u8; 4];
                    let mut bytes = Vec::with_capacity(1024);
                    let mut bitcode_buffer = bitcode::Buffer::new();
                    loop {
                        let size = match stdout.read_exact(&mut size_buf).await {
                            Ok(_) => u32::from_ne_bytes(size_buf),
                            Err(e) => handle_err!("stdout", e)
                        };
                        bytes.resize(size as usize, 0u8);
                        match stdout.read_exact(&mut bytes).await {
                            Ok(_) => {},
                            Err(e) => handle_err!("stdout", e)
                        }

                        match bitcode_buffer.decode(&bytes) {
                            Ok(msg) => {
                                callbacks.call(msg);
                            }
                            Err(e) => {
                                error!(target: "lunasim", "Failed to deserialize from lunasim: {e}");
                                continue;
                            }
                        }
                    }
                });

                (LunasimStdin(Arc::new(stdin.into())), callbacks_ref)
            }
            Err(e) => {
                error!("Failed to run simulation command: {e}");
                return;
            }
        };
        let robot_chain = create_robot_chain();
        let localizer = Localizer::new(robot_chain.clone(), Some(lunasim_stdin.clone()));
        let localizer_ref = localizer.get_ref();
        std::thread::spawn(|| localizer.run());

        let camera_link = robot_chain.find_link("depth_camera_link").unwrap().clone();

        let depth_projecter_builder = DepthProjectorBuilder {
            image_size: Vector2::new(NonZeroU32::new(36).unwrap(), NonZeroU32::new(24).unwrap()),
            focal_length_px: 10.392,
            principal_point_px: Vector2::new(17.5, 11.5),
        };
        let mut point_cloud: Box<[_]> =
            std::iter::repeat_n(AlignedVec4::from(Vector4::default()), 36 * 24).collect();
        let mut depth_projecter = depth_projecter_builder.build();
        let pcl_storage = depth_projecter_builder.make_points_storage();
        let pcl_storage_channel = Arc::new(PointsStorageChannel::new_for(&pcl_storage));
        pcl_storage_channel.set_projected(pcl_storage);
        let (heightmap_callbacks,) =
            spawn_thalassic_pipeline(Box::new([pcl_storage_channel.clone()]));

        let axis_angle = |axis: [f32; 3], angle: f32| {
            let axis = UnitVector3::new_normalize(Vector3::new(
                axis[0] as f64,
                axis[1] as f64,
                axis[2] as f64,
            ));

            UnitQuaternion::from_axis_angle(&axis, angle as f64)
        };

        let lunasim_stdin2 = lunasim_stdin.clone();

        from_lunasim_ref.add_fn_mut(move |msg| match msg {
            common::lunasim::FromLunasim::Accelerometer {
                id: _,
                acceleration,
            } => {
                let acceleration = Vector3::new(
                    acceleration[0] as f64,
                    acceleration[1] as f64,
                    acceleration[2] as f64,
                );
                localizer_ref.set_acceleration(acceleration);
            }
            common::lunasim::FromLunasim::Gyroscope { id: _, axis, angle } => {
                localizer_ref.set_angular_velocity(axis_angle(axis, angle));
            }
            common::lunasim::FromLunasim::DepthMap(depths) => {
                let Some(camera_transform) = camera_link.world_transform() else {
                    return;
                };
                let camera_transform: AlignedMatrix4<f32> =
                    camera_transform.to_homogeneous().cast::<f32>().into();
                let Some(mut pcl_storage) = pcl_storage_channel.get_finished() else {
                    return;
                };
                pcl_storage = depth_projecter.project(&depths, &camera_transform, pcl_storage, 0.01);
                pcl_storage.read(&mut point_cloud);
                pcl_storage_channel.set_projected(pcl_storage);
                let msg = FromLunasimbot::PointCloud(
                    point_cloud.iter().map(|p| [p.x, p.y, p.z]).collect(),
                );
                let lunasim_stdin2 = lunasim_stdin2.clone();
                rayon::spawn(move || {
                    let bytes = bitcode::encode(&msg);
                    lunasim_stdin2.write(&bytes);
                });
            }
            common::lunasim::FromLunasim::ExplicitApriltag {
                robot_origin,
                robot_axis,
                robot_angle,
            } => {
                let isometry = Isometry3::from_parts(
                    Vector3::new(
                        robot_origin[0] as f64,
                        robot_origin[1] as f64,
                        robot_origin[2] as f64,
                    )
                    .into(),
                    axis_angle(robot_axis, robot_angle),
                );
                localizer_ref.set_april_tag_isometry(isometry);
            }
        });

        let lunasim_stdin2 = lunasim_stdin.clone();
        let mut bitcode_buffer = bitcode::Buffer::new();
        heightmap_callbacks.add_dyn_fn_mut(Box::new(move |heightmap| {
            let bytes = bitcode_buffer.encode(&FromLunasimbot::HeightMap(
                heightmap.to_vec().into_boxed_slice(),
            ));
            lunasim_stdin2.write(bytes);
        }));

        let lunabot_stage = Arc::new(AtomicCell::new(LunabotStage::SoftStop));

        let (packet_builder, mut from_lunabase_rx, mut connected) = create_packet_builder(
            self.lunabase_address,
            lunabot_stage.clone(),
            self.max_pong_delay_ms,
        );

        let mut bitcode_buffer = bitcode::Buffer::new();

        std::thread::spawn(move || {
            run_ai(
                robot_chain,
                |action, inputs| match action {
                    Action::SetStage(stage) => {
                        lunabot_stage.store(stage);
                    }
                    Action::SetSteering(steering) => {
                        let (left, right) = steering.get_left_and_right();
                        let bytes = bitcode_buffer.encode(&FromLunasimbot::Drive {
                            left: left as f32,
                            right: right as f32,
                        });
                        lunasim_stdin.write(bytes);
                    }
                    Action::CalculatePath { from, to, mut into } => {
                        into.push(from);
                        into.push(to);
                        inputs.push(Input::PathCalculated(into));
                    }
                },
                |poll_when, inputs| {
                    let wait_disconnect = async {
                        if lunabot_stage.load() == LunabotStage::SoftStop {
                            std::future::pending::<()>().await;
                        } else {
                            connected.wait_disconnect().await;
                        }
                    };

                    match poll_when {
                        PollWhen::ReceivedLunabase => {
                            while let Ok(msg) = from_lunabase_rx.try_recv() {
                                inputs.push(Input::FromLunabase(msg));
                            }
                            if inputs.is_empty() {
                                async {
                                    tokio::select! {
                                        result = from_lunabase_rx.recv() => {
                                            let Some(msg) = result else {
                                                error!("Lunabase message channel closed");
                                                std::future::pending::<()>().await;
                                                unreachable!();
                                            };
                                            inputs.push(Input::FromLunabase(msg));
                                        }
                                        _ = wait_disconnect => {
                                            inputs.push(Input::LunabaseDisconnected);
                                        }
                                    }
                                }
                                .block_on();
                            }
                        }
                        PollWhen::Instant(deadline) => {
                            async {
                                tokio::select! {
                                    result = from_lunabase_rx.recv() => {
                                        let Some(msg) = result else {
                                            error!("Lunabase message channel closed");
                                            std::future::pending::<()>().await;
                                            unreachable!();
                                        };
                                        inputs.push(Input::FromLunabase(msg));
                                    }
                                    _ = tokio::time::sleep_until(deadline.into()) => {}
                                    _ = wait_disconnect => {
                                        inputs.push(Input::LunabaseDisconnected);
                                    }
                                }
                            }
                            .block_on();
                        }
                        PollWhen::NoDelay => {
                            // Helps prevent freezing when `NoDelay` is used frequently
                            std::thread::yield_now();
                        }
                    }
                },
            );
        });

        wait_for_ctrl_c();
    }
}
