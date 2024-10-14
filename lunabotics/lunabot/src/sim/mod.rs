use std::{
    cmp::Ordering,
    collections::VecDeque,
    process::Stdio,
    sync::{mpsc, Arc, Mutex},
};

use common::{
    lunasim::{FromLunasim, FromLunasimbot},
    LunabotStage,
};
use crossbeam::atomic::AtomicCell;
use fitter::utils::CameraProjection;
use lunabot_ai::{run_ai, Action, Input};
use nalgebra::{Isometry3, UnitQuaternion, UnitVector3, Vector2, Vector3, Vector4};
use recycler::Recycler;
use serde::{Deserialize, Serialize};
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
use urobotics::{task::SyncTask, tokio::task::block_in_place};

use crate::{
    create_robot_chain,
    localization::{Localizer, LocalizerRef},
    log_teleop_messages,
    obstacles::heightmap::heightmap_strategy,
    teleop::LunabaseConn,
    wait_for_ctrl_c, LunabotApp, PointCloudCallbacks,
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
            error!("Failed to send to lunasim: {e}");
            return;
        }
        if let Err(e) = stdin.write_all(bytes).block_on() {
            error!("Failed to send to lunasim: {e}");
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
        let localizer_ref = LocalizerRef::default();
        Localizer {
            robot_chain: robot_chain.clone(),
            lunasim_stdin: Some(lunasim_stdin.clone()),
            localizer_ref: localizer_ref.clone(),
        }
        .spawn();

        let depth_project = match CameraProjection::new(10.392, PROJECTION_SIZE, 0.01).block_on() {
            Ok(x) => Some(Arc::new(x)),
            Err(e) => {
                error!("Failed to create camera projector: {e}");
                None
            }
        };

        let camera_link = robot_chain.find_link("depth_camera_link").unwrap().clone();
        let points_buffer_recycler = Recycler::<Box<[Vector4<f32>]>>::default();

        let axis_angle = |axis: [f32; 3], angle: f32| {
            let axis = UnitVector3::new_normalize(Vector3::new(
                axis[0] as f64,
                axis[1] as f64,
                axis[2] as f64,
            ));

            UnitQuaternion::from_axis_angle(&axis, angle as f64)
        };

        let raw_pcl_callbacks = Arc::new(PointCloudCallbacks::default());
        let raw_pcl_callbacks_ref = raw_pcl_callbacks.get_ref();

        let lunasim_stdin2 = lunasim_stdin.clone();
        let mut bitcode_buffer = bitcode::Buffer::new();
        raw_pcl_callbacks_ref.add_dyn_fn_mut(Box::new(move |point_cloud| {
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
                let Some(depth_project) = &depth_project else {
                    return;
                };
                let Some(camera_transform) = camera_link.world_transform() else {
                    return;
                };
                let mut points_buffer = points_buffer_recycler
                    .get_or_else(|| vec![Vector4::default(); 36 * 24].into_boxed_slice());
                let depth_project = depth_project.clone();
                let raw_pcl_callbacks = raw_pcl_callbacks.clone();

                get_tokio_handle().spawn(async move {
                    depth_project
                        .project_buffer(&depths, camera_transform.cast(), &mut **points_buffer)
                        .await;
                    block_in_place(|| {
                        raw_pcl_callbacks.call_immut(&points_buffer);
                    });
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

        let heightmap_ref = heightmap_strategy(PROJECTION_SIZE, &raw_pcl_callbacks_ref);
        let lunasim_stdin2 = lunasim_stdin.clone();
        let mut bitcode_buffer = bitcode::Buffer::new();
        heightmap_ref.add_dyn_fn_mut(Box::new(move |heightmap| {
            let bytes = bitcode_buffer.encode(&FromLunasimbot::HeightMap(
                heightmap.to_vec().into_boxed_slice(),
            ));
            lunasim_stdin2.write(bytes);
        }));

        let lunabot_stage = Arc::new(AtomicCell::new(LunabotStage::SoftStop));
        let (from_lunabase_tx, from_lunabase_rx) = mpsc::channel();
        let mut bitcode_buffer = bitcode::Buffer::new();

        let packet_builder = LunabaseConn {
            lunabase_address: self.app.lunabase_address,
            on_msg: move |bytes: &[u8]| match bitcode_buffer.decode(bytes) {
                Ok(msg) => {
                    let _ = from_lunabase_tx.send(msg);
                    true
                }
                Err(e) => {
                    error!("Failed to decode from lunabase: {e}");
                    false
                }
            },
            lunabot_stage: lunabot_stage.clone(),
        }
        .connect_to_lunabase();
        let mut bitcode_buffer = bitcode::Buffer::new();

        std::thread::spawn(move || {
            run_ai(|action| match action {
                Action::WaitForLunabase => {
                    let Ok(msg) = from_lunabase_rx.recv() else {
                        error!("Lunabase message channel closed");
                        loop {
                            std::thread::park();
                        }
                    };
                    Input::FromLunabase(msg)
                }
                Action::SetStage(stage) => {
                    lunabot_stage.store(stage);
                    Input::NoInput
                }
                Action::SetSteering(steering) => {
                    let (left, right) = steering.get_left_and_right();
                    let bytes = bitcode_buffer.encode(&FromLunasimbot::Drive {
                        left: left as f32,
                        right: right as f32,
                    });
                    lunasim_stdin.write(bytes);
                    Input::NoInput
                }
            });
        });

        wait_for_ctrl_c();
    }
}
