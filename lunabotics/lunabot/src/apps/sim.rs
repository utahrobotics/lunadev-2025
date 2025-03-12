use std::{
    cmp::Ordering,
    collections::VecDeque,
    net::{IpAddr, SocketAddr},
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
use lumpur::set_on_exit;
use lunabot_ai::{run_ai, Action, Input, PollWhen};
use nalgebra::{
    Isometry3, Scale3, Transform3, UnitQuaternion, UnitVector3, Vector2, Vector3, Vector4,
};
use simple_motion::{ChainBuilder, NodeSerde};
use tasker::shared::OwnedData;
use tasker::tokio;
use tasker::{
    callbacks::caller::CallbacksStorage,
    define_callbacks, fn_alias, get_tokio_handle,
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        process::{ChildStdin, Command},
        runtime::Handle,
    },
    BlockOn,
};
use thalassic::DepthProjectorBuilder;
use tracing::{error, info, warn};

use crate::{
    localization::{IMUReading, Localizer},
    pipelines::thalassic::{get_observe_depth, spawn_thalassic_pipeline},
};
use crate::{pathfinding::DefaultPathfinder, pipelines::thalassic::ThalassicData};

use super::{create_packet_builder, log_teleop_messages};

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

pub struct LunasimbotApp {
    pub lunabase_address: Option<IpAddr>,
    pub max_pong_delay_ms: u64,
}

const DEPTH_BASE_WIDTH: u32 = 36;
const DEPTH_BASE_HEIGHT: u32 = 24;
const SCALE: u32 = 3;

impl LunasimbotApp {
    pub fn run(self) {
        log_teleop_messages();
        if let Err(e) = init_gputter_blocking() {
            error!("Failed to initialize gputter: {e}");
        }

        async fn import() -> std::io::Result<std::process::Output> {
            tokio::select! {
                x = {
                    info!("Importing godot project...");
                    Command::new("godot")
                        .args(["--path", "godot/lunasim", "--import"])
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .output()
                } => x,
                _ = async {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    warn!("NOTE: If the godot editor is still open and idle, close it now");
                    std::future::pending::<()>().await;
                } => {
                    unreachable!()
                }
            }
        }

        match import().block_on() {
            Ok(output) => {
                if !output.status.success() {
                    error!("Failed to import godot project: {output:?}");
                    return;
                }
            }
            Err(e) => {
                error!("Failed to import godot project: {e}");
                return;
            }
        }
        info!("Godot project imported");

        let _guard = get_tokio_handle().enter();
        let mut cmd = Command::new("godot");

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .args(["--path", "godot/lunasim", "-d"]);

        let (lunasim_stdin, from_lunasim_ref) = match cmd.spawn() {
            Ok(tmp) => {
                let child = tmp;
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
        let lunasim_stdin2 = lunasim_stdin.clone();
        set_on_exit(move || {
            lunasim_stdin2.write(&bitcode::encode(&FromLunasimbot::Quit));
            std::process::exit(0);
        });
        let robot_chain = NodeSerde::from_reader(
            std::fs::File::open("robot-layout/sim.json").expect("Failed to read robot chain"),
        )
        .expect("Failed to parse robot chain");
        let robot_chain = ChainBuilder::from(robot_chain).finish_static();

        let localizer = Localizer::new(robot_chain, Some(lunasim_stdin.clone()), 1);
        let localizer_ref = localizer.get_ref();
        std::thread::spawn(|| localizer.run());

        let camera_link = robot_chain.get_node_with_name("depth_camera").unwrap();

        let depth_projecter_builder = DepthProjectorBuilder {
            image_size: Vector2::new(NonZeroU32::new(DEPTH_BASE_WIDTH * SCALE).unwrap(), NonZeroU32::new(DEPTH_BASE_HEIGHT * SCALE).unwrap()),
            focal_length_px: 10.392 * SCALE as f32,
            principal_point_px: Vector2::new((DEPTH_BASE_WIDTH * SCALE - 1) as f32 / 2.0, (DEPTH_BASE_HEIGHT * SCALE - 1) as f32 / 2.0),
            max_depth: 1.0,
        };

        let mut buffer = OwnedData::from(ThalassicData::default());
        let shared_thalassic_data = buffer.create_lendee();

        let lunasim_stdin2 = lunasim_stdin.clone();
        let mut bitcode_buffer = bitcode::Buffer::new();
        buffer.add_callback(
            move |ThalassicData {
                      heightmap,
                      gradmap,
                      expanded_obstacle_map,
                      ..
                  }| {
                let bytes = bitcode_buffer.encode(&FromLunasimbot::Thalassic {
                    heightmap: heightmap.to_vec().into_boxed_slice(),
                    gradmap: gradmap.to_vec().into_boxed_slice(),
                    obstaclemap: expanded_obstacle_map.iter().map(|o| o.occupied()).collect(),
                });
                lunasim_stdin2.write(bytes);
            },
        );

        let thalassic_ref = spawn_thalassic_pipeline(buffer, 72 * 48);
        let mut depth_projecter = depth_projecter_builder.build(thalassic_ref);

        let axis_angle = |axis: [f32; 3], angle: f32| {
            let axis = UnitVector3::new_normalize(Vector3::new(
                axis[0] as f64,
                axis[1] as f64,
                axis[2] as f64,
            ));

            UnitQuaternion::from_axis_angle(&axis, angle as f64)
        };

        let lunasim_stdin2 = lunasim_stdin.clone();
        let mut point_cloud: Box<[_]> =
            std::iter::repeat_n(AlignedVec4::from(Vector4::default()), DEPTH_BASE_WIDTH as usize * DEPTH_BASE_HEIGHT as usize * SCALE as usize * SCALE as usize).collect();
        from_lunasim_ref.add_fn_mut(move |msg| match msg {
            FromLunasim::Accelerometer {
                id: _,
                acceleration,
            } => {
                let acceleration = Vector3::new(
                    acceleration[0] as f64,
                    acceleration[1] as f64,
                    acceleration[2] as f64,
                );
                localizer_ref.set_imu_reading(
                    0,
                    IMUReading {
                        acceleration,
                        ..Default::default()
                    },
                );
            }
            FromLunasim::Gyroscope { id: _, .. } => {}
            FromLunasim::DepthMap(depths) => {
                if !get_observe_depth() {
                    return;
                }
                let camera_transform = camera_link.get_global_isometry();
                let camera_transform: AlignedMatrix4<f32> =
                    camera_transform.to_homogeneous().cast::<f32>().into();

                depth_projecter.project(&depths, &camera_transform, 0.001, Some(&mut point_cloud));
                let msg = FromLunasimbot::PointCloud(
                    point_cloud
                        .iter()
                        .filter(|p| p.w != 0.0)
                        .map(|p| [p.x, p.y, p.z])
                        .collect(),
                );
                let lunasim_stdin2 = lunasim_stdin2.clone();
                let bytes = bitcode::encode(&msg);
                rayon::spawn(move || {
                    lunasim_stdin2.write(&bytes);
                });
            }
            FromLunasim::ExplicitApriltag {
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

        let grid_to_world = Transform3::from_matrix_unchecked(
            Scale3::new(-0.03125, 1.0, -0.03125).to_homogeneous(),
        );
        let world_to_grid = grid_to_world.try_inverse().unwrap();
        let mut pathfinder = DefaultPathfinder::new(world_to_grid, grid_to_world);
        pathfinder.cell_grid.enable_diagonal_mode();
        pathfinder.cell_grid.fill();

        let lunabot_stage = Arc::new(AtomicCell::new(LunabotStage::SoftStop));

        let (_packet_builder, mut from_lunabase_rx, mut connected) = create_packet_builder(
            self.lunabase_address
                .map(|ip| SocketAddr::new(ip, common::ports::LUNABASE_SIM_TELEOP)),
            lunabot_stage.clone(),
            self.max_pong_delay_ms,
        );

        let mut bitcode_buffer = bitcode::Buffer::new();

        run_ai(
            robot_chain.into(),
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
                    pathfinder.push_path_into(&shared_thalassic_data, from, to, &mut into);
                    let bytes = bitcode_buffer.encode(&FromLunasimbot::Path(
                        into.iter()
                            .map(|p| p.point.coords.cast::<f32>().data.0[0])
                            .collect(),
                    ));
                    lunasim_stdin.write(bytes);
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
    }
}
