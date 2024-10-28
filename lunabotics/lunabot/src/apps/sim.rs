use std::{
    cmp::Ordering,
    collections::VecDeque,
    process::Stdio,
    sync::{Arc, Mutex},
};

use common::{
    lunasim::{FromLunasim, FromLunasimbot},
    LunabotStage,
};
use crossbeam::atomic::AtomicCell;
use gputter::init_gputter_blocking;
use lunabot_ai::{run_ai, Action, Input};
use nalgebra::{Isometry3, UnitQuaternion, UnitVector3, Vector2, Vector3};
use serde::{Deserialize, Serialize};
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

use crate::{localization::Localizer, pipelines::thalassic::spawn_thalassic_pipeline};

use super::{
    create_packet_builder, create_robot_chain, log_teleop_messages, wait_for_ctrl_c, LunabotApp,
};

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

#[derive(Serialize, Deserialize)]
pub struct LunasimbotApp {
    #[serde(flatten)]
    app: LunabotApp,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    simulation_command: Vec<String>,
}

const PROJECTION_SIZE: Vector2<u32> = Vector2::new(36, 24);

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
                        let mut bytes = VecDeque::with_capacity(7);
                        let mut buf = [0u8];
                        loop {
                            match stdout.read_exact(&mut buf).await {
                                Ok(0) => unreachable!("godot program should not exit"),
                                Ok(_) => {
                                    bytes.push_back(buf[0]);
                                }
                                Err(e) => error!(target: "lunasim", "Faced the following error while reading stdout: {e}"),
                            }
                            match bytes.len().cmp(&6) {
                                Ordering::Equal => {}
                                Ordering::Greater => {bytes.pop_front();}
                                Ordering::Less => continue,
                            }
                            if bytes == b"READY\n" {
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
        let (depth_map_buffer, pcl_callbacks, heightmap_callbacks) = spawn_thalassic_pipeline(10.392, 0.01, PROJECTION_SIZE, camera_link);

        let axis_angle = |axis: [f32; 3], angle: f32| {
            let axis = UnitVector3::new_normalize(Vector3::new(
                axis[0] as f64,
                axis[1] as f64,
                axis[2] as f64,
            ));

            UnitQuaternion::from_axis_angle(&axis, angle as f64)
        };

        let lunasim_stdin2 = lunasim_stdin.clone();
        let mut bitcode_buffer = bitcode::Buffer::new();
        pcl_callbacks.add_dyn_fn_mut(Box::new(move |point_cloud| {
            let msg =
                FromLunasimbot::PointCloud(point_cloud.iter().map(|p| [p.x, p.y, p.z]).collect());
            let bytes = bitcode_buffer.encode(&msg);
            lunasim_stdin2.write(bytes);
        }));

        from_lunasim_ref.add_fn(move |msg| match msg {
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
                depth_map_buffer.write(|buffer| {
                    buffer.copy_from_slice(&depths);
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
            self.app.lunabase_address,
            lunabot_stage.clone(),
            self.app.max_pong_delay_ms,
        );

        let mut bitcode_buffer = bitcode::Buffer::new();

        std::thread::spawn(move || {
            run_ai(robot_chain, |action, inputs| {
                debug_assert!(inputs.is_empty());
                let wait_disconnect = async {
                    if lunabot_stage.load() == LunabotStage::SoftStop {
                        std::future::pending::<()>().await;
                    } else {
                        connected.wait_disconnect().await;
                    }
                };

                match action {
                    Action::WaitForLunabase => {
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
                    Action::WaitUntil(deadline) => {
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
                    Action::PollAgain => {}
                }
            });
        });

        wait_for_ctrl_c();
    }
}
