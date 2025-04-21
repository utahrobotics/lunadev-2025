use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use anyhow::Context;
use camera::enumerate_cameras;
use common::LunabotStage;
use crossbeam::atomic::AtomicCell;
use depth::enumerate_depth_cameras;
use file_lock::FileLock;
use fxhash::FxHashMap;
use gputter::init_gputter_blocking;
use lumpur::set_on_exit;
use lunabot_ai::{run_ai, Action, Input, PollWhen};
use mio::{Events, Interest, Poll, Token};
use motors::{enumerate_motors, MotorMask, VescIDs};
use nalgebra::{Scale3, Transform3, UnitQuaternion};
use rerun_viz::init_rerun;
use imu_calib::*;
use rp2040::*;
use serde::Deserialize;
use simple_motion::{ChainBuilder, NodeSerde};
use streaming::start_streaming;
use tasker::{get_tokio_handle, shared::OwnedData, tokio, BlockOn};
use tracing::error;
use udev::Event;

pub use rerun_viz::{RerunViz, RECORDER, ROBOT, ROBOT_STRUCTURE};

use crate::{
    apps::log_teleop_messages, localization::Localizer, pathfinding::DefaultPathfinder,
    pipelines::thalassic::ThalassicData,
};

use super::create_packet_builder;

mod apriltag;
mod camera;
mod depth;
mod motors;
mod rerun_viz;
mod rp2040;
mod streaming;

pub use apriltag::Apriltag;

#[derive(Deserialize, Debug)]
pub struct CameraInfo {
    link_name: String,
    focal_length_x_px: f64,
    focal_length_y_px: f64,
    stream_index: usize,
}

#[derive(Deserialize, Debug)]
pub struct DepthCameraInfo {
    link_name: String,
    #[serde(default)]
    ignore_apriltags: bool,
    stream_index: usize,
}

#[derive(Deserialize, Debug)]
pub struct IMUInfo {
    link_name: String,
    #[serde(default)]
    correction: UnitQuaternion<f32>
}

#[derive(Deserialize, Debug)]
pub struct ActuatorControllerInfo {
    serial: String
}

#[derive(Deserialize, Debug)]
pub struct VescPair {
    id1: u8,
    id2: u8,
    mask1: MotorMask,
    mask2: MotorMask,
    #[serde(default = "default_command_both")]
    command_both: bool
}

fn default_command_both() -> bool {
    true
}

#[derive(Deserialize, Debug)]
pub struct SingleVesc {
    id: u8,
    mask: MotorMask,
}

#[derive(Deserialize, Debug, Default)]
pub struct Vesc {
    #[serde(default)]
    singles: Vec<SingleVesc>,
    #[serde(default)]
    pairs: Vec<VescPair>,
    speed_multiplier: Option<f32>,
}

pub struct LunabotApp {
    pub lunabase_address: Option<IpAddr>,
    pub max_pong_delay_ms: u64,
    pub cameras: FxHashMap<String, CameraInfo>,
    pub depth_cameras: FxHashMap<String, DepthCameraInfo>,
    pub apriltags: FxHashMap<String, Apriltag>,
    pub imus: FxHashMap<String, IMUInfo>,
    pub robot_layout: String,
    pub vesc: Vesc,
    pub rerun_viz: RerunViz,
    pub imu_correction: Option<CalibrationParameters>,
    pub actuator_controller_info: Option<ActuatorControllerInfo>
}

impl LunabotApp {
    pub fn run(self) {
        let filelock = match FileLock::lock(
            "/home/lock/lunabot.lock",
            false,
            file_lock::FileOptions::new().write(true),
        ) {
            Ok(x) => x,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    error!("Another instance of lunabot is already running");
                    return;
                } else {
                    error!("Failed to lock file: {e}");
                    return;
                }
            }
        };
        log_teleop_messages();
        if let Err(e) = init_gputter_blocking() {
            error!("Failed to initialize gputter: {e}");
        }

        init_rerun(self.rerun_viz);

        let apriltags = match self
            .apriltags
            .into_iter()
            .map(|(id_str, apriltag)| id_str.parse().map(|id| (id, apriltag)))
            .try_collect::<Vec<_>>()
        {
            Ok(apriltags) => Box::leak(apriltags.into_boxed_slice()),
            Err(e) => {
                error!("Failed to parse apriltags: {e}");
                return;
            }
        };
        set_on_exit(move || {
            drop(filelock);
            std::process::exit(0);
        });

        let handle = get_tokio_handle();
        let _guard = handle.enter();

        let robot_chain = NodeSerde::from_reader(
            std::fs::File::open(self.robot_layout).expect("Failed to read robot chain"),
        )
        .expect("Failed to parse robot chain");
        let robot_chain = ChainBuilder::from(robot_chain).finish_static();

        let lunabot_stage = Arc::new(AtomicCell::new(LunabotStage::SoftStop));

        let (packet_builder, mut from_lunabase_rx, mut connected) = create_packet_builder(
            self.lunabase_address
                .map(|ip| SocketAddr::new(ip, common::ports::TELEOP)),
            lunabot_stage.clone(),
            self.max_pong_delay_ms,
        );

        let localizer = Localizer::new(robot_chain.clone(), self.imus.len(), packet_builder);
        let localizer_ref = localizer.get_ref();
        std::thread::spawn(|| localizer.run());

        start_streaming(self.lunabase_address);

        enumerate_cameras(
            &localizer_ref,
            self.cameras.into_iter().map(
                |(
                    port,
                    CameraInfo {
                        link_name,
                        focal_length_x_px,
                        focal_length_y_px,
                        stream_index,
                    },
                )| {
                    (
                        port,
                        camera::CameraInfo {
                            node: robot_chain
                                .get_node_with_name(&link_name)
                                .context("Failed to find camera link")
                                .unwrap()
                                .into(),
                            focal_length_x_px,
                            focal_length_y_px,
                            stream_index,
                        },
                    )
                },
            ),
            apriltags,
        );

        let mut buffer = OwnedData::from(ThalassicData::default());
        let shared_thalassic_data = buffer.create_lendee();
        let shared_thalassic_data2 = buffer.create_lendee();

        common::lunabase_sync::lunabot_task(move |_path, thalassic_data| {
            let raw_data = shared_thalassic_data2.get();
            thalassic_data.heightmap.iter_mut().zip(&raw_data.heightmap).for_each(
                |(dst, &src)| {
                    *dst = src as f16;
                }
            );
            (false, true)
        });

        enumerate_depth_cameras(
            buffer,
            &localizer_ref,
            self.depth_cameras.into_iter().map(
                |(
                    serial,
                    DepthCameraInfo {
                        link_name,
                        ignore_apriltags: observe_apriltags,
                        stream_index,
                    },
                )| {
                    (
                        serial,
                        depth::DepthCameraInfo {
                            node: robot_chain
                                .get_node_with_name(&link_name)
                                .context("Failed to find camera link")
                                .unwrap()
                                .into(),
                            ignore_apriltags: observe_apriltags,
                            stream_index,
                        },
                    )
                },
            ),
            apriltags,
        );

        let grid_to_world = Transform3::from_matrix_unchecked(
            Scale3::new(0.03125, 1.0, 0.03125).to_homogeneous(),
        );
        let world_to_grid = grid_to_world.try_inverse().unwrap();
        let mut pathfinder = DefaultPathfinder::new(world_to_grid, grid_to_world);

        // correction parameters are defined in app-config.toml
        // corrections are applied in the localizer
        localizer_ref.set_imu_correction_parameters(self.imu_correction);

        enumerate_imus(
            &localizer_ref,
            self.imus.into_iter().map(|(port, IMUInfo { link_name, correction })| {
                (
                    port,
                    rp2040::IMUInfo {
                        correction,
                        node: robot_chain
                            .get_node_with_name(&link_name)
                            .context("Failed to find IMU link")
                            .unwrap()
                            .into(),
                        link_name,
                    },
                )
            }),
        );

        let mut vesc_ids = VescIDs::default();

        for SingleVesc { id, mask } in self.vesc.singles {
            if vesc_ids.add_single_vesc(id, mask) {
                error!("Motor {id} has already been added");
                return;
            }
        }
        for VescPair {
            id1,
            id2,
            mask1,
            mask2,
            command_both
        } in self.vesc.pairs
        {
            if vesc_ids.add_dual_vesc(id1, id2, mask1, mask2, command_both) {
                error!("Motors {id1} or {id2} have already been added");
                return;
            }
        }

        let motor_ref = enumerate_motors(vesc_ids, self.vesc.speed_multiplier.unwrap_or(1.0));

        let mut actuator_controller = enumerate_actuator_controllers(&localizer_ref, self.actuator_controller_info.map(
            |inner| {
                rp2040::ActuatorControllerInfo {
                    serial: inner.serial
                }
            }
        ).unwrap_or_default());

        run_ai(
            robot_chain.into(),
            |action, inputs| match action {
                Action::SetStage(stage) => {
                    lunabot_stage.store(stage);
                }
                Action::SetSteering(steering) => {
                    let (left, right) = steering.get_left_and_right();
                    motor_ref.set_speed(left as f32, right as f32);
                }
                Action::SetActuators(actuator_cmd) => {
                    let _ = actuator_controller.send_command(actuator_cmd);
                }
                Action::CalculatePath { from, to, mut into } => {
                    if pathfinder.push_path_into(&shared_thalassic_data, from, to, &mut into) {
                        inputs.push(Input::PathCalculated(into));
                    } else {
                        inputs.push(Input::FailedToCalculatePath(into));
                    }
                }
                Action::AvoidPoint(point) => {
                    pathfinder.avoid_point(point);
                }
                Action::ClearPointsToAvoid => {
                    pathfinder.clear_points_to_avoid();
                },
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
				    tracing::info!("msg: {:?}", msg);
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

pub fn udev_poll(mut socket: udev::MonitorSocket) -> impl Iterator<Item = Event> {
    let mut poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(1024);

    poll.registry()
        .register(
            &mut socket,
            Token(0),
            Interest::READABLE | Interest::WRITABLE,
        )
        .unwrap();

    std::iter::from_fn(move || loop {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            if event.token() == Token(0) && event.is_writable() {
                return Some(socket.iter().collect::<Vec<_>>());
            }
        }
    })
    .flatten()
}
