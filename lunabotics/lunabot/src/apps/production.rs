use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use camera::enumerate_cameras;
use common::LunabotStage;
use crossbeam::atomic::AtomicCell;
use depth::enumerate_depth_cameras;
use fxhash::FxHashMap;
use gputter::init_gputter_blocking;
use lunabot_ai::{run_ai, Action, Input, PollWhen};
use mio::{Events, Interest, Poll, Token};
use motors::enumerate_motors;
use nalgebra::{Scale3, Transform3};
use pathfinding::grid::Grid;
use serde::Deserialize;
use simple_motion::{ChainBuilder, NodeSerde};
use streaming::camera_streaming;
use tasker::{get_tokio_handle, shared::OwnedData, tokio, BlockOn};
use tracing::error;
use rp2040::*;
use udev::Event;

use crate::{
    apps::log_teleop_messages, localization::Localizer, pathfinding::DefaultPathfinder,
    pipelines::thalassic::ThalassicData,
};

use super::create_packet_builder;

mod apriltag;
mod camera;
mod depth;
mod motors;
mod streaming;

pub mod dataviz;
// mod audio_streaming;

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

fn subaddress_of(mut addr: SocketAddr, port_offset: u16) -> SocketAddr {
    let new_port = addr
        .port()
        .checked_add(port_offset)
        .unwrap_or_else(|| addr.port().wrapping_add(port_offset));
    addr.set_port(new_port);
    addr
}

pub struct LunabotApp {
    pub lunabase_address: SocketAddr,
    pub lunabase_streaming_address: Option<SocketAddr>,
    #[cfg(feature = "experimental")]
    pub lunabase_audio_streaming_address: Option<SocketAddr>,
    pub max_pong_delay_ms: u64,
    pub cameras: FxHashMap<String, CameraInfo>,
    pub depth_cameras: FxHashMap<String, DepthCameraInfo>,
    pub apriltags: FxHashMap<String, Apriltag>,
    pub robot_layout: String,
}

impl LunabotApp {
    pub fn run(self) {
        log_teleop_messages();
        if let Err(e) = init_gputter_blocking() {
            error!("Failed to initialize gputter: {e}");
        }
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

        let handle = get_tokio_handle();
        let _guard = handle.enter();

        let robot_chain = NodeSerde::from_reader(
            std::fs::File::open(self.robot_layout).expect("Failed to read robot chain"),
        )
        .expect("Failed to parse robot chain");
        let robot_chain = ChainBuilder::from(robot_chain).finish_static();

        let localizer = Localizer::new(robot_chain.clone());
        let localizer_ref = localizer.get_ref();
        std::thread::spawn(|| localizer.run());
        let camera_streaming_address = self
            .lunabase_streaming_address
            .unwrap_or_else(|| subaddress_of(self.lunabase_address, 1));

        camera_streaming(camera_streaming_address);

        #[cfg(feature = "experimental")]
        if let Err(e) = audio_streaming::audio_streaming(
            self.lunabase_audio_streaming_address
                .unwrap_or_else(|| subaddress_of(self.lunabase_address, 2)),
        ) {
            error!("Failed to start audio streaming: {e}");
        }

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
            Scale3::new(-0.03125, 1.0, -0.03125).to_homogeneous(),
        );
        let world_to_grid = grid_to_world.try_inverse().unwrap();
        let mut pathfinder = DefaultPathfinder {
            world_to_grid,
            grid_to_world,
            grid: Grid::new(128, 256),
        };
        pathfinder.grid.enable_diagonal_mode();
        pathfinder.grid.fill();

        let lunabot_stage = Arc::new(AtomicCell::new(LunabotStage::SoftStop));

        let (packet_builder, mut from_lunabase_rx, mut connected) = create_packet_builder(
            self.lunabase_address,
            lunabot_stage.clone(),
            self.max_pong_delay_ms,
        );

        let localizer_ref = localizer_ref.clone();
        let picos = PicoController::enumerate_picos();
        handle.spawn(async move { 'controller: {
            use tracing::{warn, info, error};
            use embedded_common::*;
            use nalgebra::Vector3;
            let pico_paths = match picos  {
                Ok(picos) => picos,
                Err(e) => {
                    error!("Couldn't enumerate connected picos. {}", e);
                    break 'controller;
                }
            };

            let Some(path) = pico_paths.get(0) else {
                error!("No Picos found");
                break 'controller;
            };

            
            let mut pico_controler = match PicoController::new(path).await {
                Ok(controller) => controller,
                Err(e) => {
                    error!("Couldn't find pico: {e}");
                    break 'controller;
                }
            };
            loop {
                match pico_controler.get_message_from_pico().await {
                    Ok(FromIMU::AngularRateReading(AngularRate{x,y,z})) => {
                        // info!("Got angular rate reading: {x}, {y}, {z}");
                        // TODO: set angular rate
                    }

                    Ok(FromIMU::AccellerationNormReading(AccelerationNorm{x,y,z})) => {
                        // info!("Got accel norm reading: {x}, {y}, {z}");

                        // TODO: set accel
                    }

                    Ok(FromIMU::NoDataReady) => {
                        warn!("No data ready");
                    }

                    Ok(FromIMU::Error) => {
                        warn!("Received error from IMU");
                    }

                    Err(e) => {
                        error!("Error getting readings from pico: {}",e);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }});

        let motor_ref = enumerate_motors(handle);

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
                Action::CalculatePath { from, to, mut into } => {
                    pathfinder.pathfind(&shared_thalassic_data, from, to, &mut into);
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

pub fn udev_poll(mut socket: udev::MonitorSocket) -> impl Iterator<Item=Event> {
    let mut poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(1024);

    poll.registry().register(
        &mut socket,
        Token(0),
        Interest::READABLE | Interest::WRITABLE,
    ).unwrap();

    std::iter::from_fn(move || {
        loop {
            poll.poll(&mut events, None).unwrap();
    
            for event in &events {
                if event.token() == Token(0) && event.is_writable() {
                    return Some(socket.iter().collect::<Vec<_>>());
                }
            }
        }
    }).flatten()
}