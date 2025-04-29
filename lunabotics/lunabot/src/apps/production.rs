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
}

#[derive(Deserialize, Debug)]
pub struct V3PicoInfo {
    serial: String,
    imus: [IMUInfo; 4]
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
    pub robot_layout: String,
    pub vesc: Vesc,
    pub rerun_viz: RerunViz,
    pub imu_correction: Option<CalibrationParameters>,
    pub v3pico: V3PicoInfo
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

        let localizer = Localizer::new(robot_chain.clone(), self.v3pico.imus.len(), packet_builder);
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

        // correction parameters are defined in app-config.toml
        // corrections are applied in the localizer
        localizer_ref.set_imu_correction_parameters(self.imu_correction);

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
        
        let hinge_node = robot_chain.get_node_with_name("lift_hinge").expect("lift_hinge not defined in robot layout");
        
        let mut actuator_controller = enumerate_v3picos(hinge_node, localizer_ref.clone(), {
            rp2040::V3PicoInfo{
                serial: self.v3pico.serial,
                imus: [
                    rp2040::IMUInfo {
                        node: robot_chain.get_node_with_name(&self.v3pico.imus[0].link_name)
                            .context("Failed to find IMU link")
                                .unwrap()
                                .into(),
                        link_name: self.v3pico.imus[0].link_name.clone(),
                    },
                    rp2040::IMUInfo {
                        node: robot_chain.get_node_with_name(&self.v3pico.imus[1].link_name)
                            .context("Failed to find IMU link")
                                .unwrap()
                                .into(),
                        link_name: self.v3pico.imus[1].link_name.clone(),
                    },
                    rp2040::IMUInfo {
                        node: robot_chain.get_node_with_name(&self.v3pico.imus[2].link_name)
                            .context("Failed to find IMU link")
                                .unwrap()
                                .into(),
                        link_name: self.v3pico.imus[2].link_name.clone(),
                    },
                    rp2040::IMUInfo {
                        node: robot_chain.get_node_with_name(&self.v3pico.imus[3].link_name)
                            .context("Failed to find IMU link")
                                .unwrap()
                                .into(),
                        link_name: self.v3pico.imus[3].link_name.clone(),
                    },
                ]
            }
        });

        let mut pathfinder = DefaultPathfinder::new(vec![]);
        
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
                Action::CalculatePath { from, to, kind, fail_if_dest_is_known } => {
                    
                    if 
                        fail_if_dest_is_known && 
                        pathfinder.get_map_data(&shared_thalassic_data).is_known(to) 
                    {
                        return inputs.push(Input::PathDestIsKnown);
                    }
                    
                    
                    if let Ok(path) = pathfinder.find_path(&shared_thalassic_data, from, to, kind) {
                        inputs.push(Input::PathCalculated(path));
                    } else {
                        inputs.push(Input::FailedToCalculatePath);
                    }
                }
                
                Action::ClearPointsToAvoid => {
                    pathfinder.clear_cells_to_avoid();
                },
                
                Action::CheckIfExplored { area, robot_cell_pos } => {
                    let x_lo = area.right as usize;
                    let x_hi = area.left as usize;
                    let y_lo = area.bottom as usize;
                    let y_hi = area.top as usize;
                    
                    let map_data = pathfinder.get_map_data(&shared_thalassic_data);
                    
                    for x in x_lo..x_hi {
                        for y in y_lo..y_hi {
                            
                            // cells need to be explored if theyre unknown AND the robot isn't on top of it
                            if 
                                !map_data.is_known((x, y)) && 
                                crate::utils::distance_between_tuples(robot_cell_pos, (x, y)) > pathfinder.current_robot_radius_cells() 
                            {
                                return inputs.push(Input::NotDoneExploring((x, y)));
                            }
                        }
                    }
                    
                    inputs.push(Input::DoneExploring);
                }
                Action::AvoidCell(_) => todo!(),
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
