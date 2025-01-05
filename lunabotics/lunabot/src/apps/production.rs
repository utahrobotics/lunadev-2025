use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use apriltag::Apriltag;
use camera::enumerate_cameras;
use common::LunabotStage;
use crossbeam::atomic::AtomicCell;
use depth::enumerate_depth_cameras;
use fxhash::FxHashMap;
use gputter::init_gputter_blocking;
use lunabot_ai::{run_ai, Action, Input, PollWhen};
use nalgebra::Vector2;
use pathfinding::Pathfinder;
use serde::Deserialize;
use streaming::camera_streaming;
use urobotics::{
    app::{define_app, Runnable},
    get_tokio_handle,
    log::{error, log_to_console, Level},
    shared::OwnedData,
    tokio, BlockOn,
};

use crate::{
    apps::log_teleop_messages, localization::Localizer, pipelines::thalassic::ThalassicData,
};

use super::{create_packet_builder, create_robot_chain, wait_for_ctrl_c};

mod apriltag;
mod camera;
mod depth;
mod streaming;

#[derive(Deserialize, Debug)]
struct CameraInfo {
    link_name: String,
    focal_length_x_px: f64,
    focal_length_y_px: f64,
    stream_index: usize,
}

#[derive(Deserialize, Debug)]
struct DepthCameraInfo {
    link_name: String,
    #[serde(default)]
    ignore_apriltags: bool,
    stream_index: usize,
}

#[derive(Deserialize)]
pub struct LunabotApp {
    lunabase_address: SocketAddr,
    lunabase_streaming_address: Option<SocketAddr>,
    #[serde(default = "super::default_max_pong_delay_ms")]
    max_pong_delay_ms: u64,
    #[serde(default)]
    cameras: FxHashMap<String, CameraInfo>,
    #[serde(default)]
    depth_cameras: FxHashMap<String, DepthCameraInfo>,
    #[serde(default)]
    apriltags: FxHashMap<String, Apriltag>,
}

impl Runnable for LunabotApp {
    fn run(self) {
        log_to_console([
            ("wgpu_hal::vulkan::instance", Level::Info),
            ("wgpu_core::device::resource", Level::Info),
        ]);
        log_teleop_messages();
        if let Err(e) = init_gputter_blocking() {
            error!("Failed to initialize gputter: {e}");
        }
        let apriltags = match self
            .apriltags
            .into_iter()
            .map(|(id_str, apriltag)| id_str.parse().map(|id| (id, apriltag)))
            .try_collect::<FxHashMap<_, _>>()
        {
            Ok(apriltags) => apriltags,
            Err(e) => {
                error!("Failed to parse apriltags: {e}");
                return;
            }
        };

        let _guard = get_tokio_handle().enter();

        let robot_chain = create_robot_chain();
        let localizer = Localizer::new(robot_chain.clone(), None);
        let localizer_ref = localizer.get_ref();
        std::thread::spawn(|| localizer.run());

        if let Err(e) = camera_streaming(self.lunabase_streaming_address.unwrap_or_else(|| {
            let mut addr = self.lunabase_address;
            if addr.port() == u16::MAX {
                addr.set_port(65534);
            } else {
                addr.set_port(addr.port() + 1);
            }
            addr
        })) {
            error!("Failed to start camera streaming: {e}");
        }

        if let Err(e) = enumerate_cameras(
            localizer_ref.clone(),
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
                            k_node: robot_chain
                                .find_link(&link_name)
                                .context("Failed to find camera link")
                                .unwrap()
                                .clone(),
                            focal_length_x_px,
                            focal_length_y_px,
                            stream_index,
                        },
                    )
                },
            ),
            &apriltags,
        ) {
            error!("Failed to enumerate cameras: {e}");
        }

        let mut buffer = OwnedData::from(ThalassicData::default());
        let shared_thalassic_data = buffer.create_lendee();
        // buffer.add_callback(|ThalassicData { heightmap, .. }| {
        //     debug_assert_eq!(heightmap.len(), 128 * 64);
        //     let max = heightmap.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        //     let min = heightmap.iter().copied().fold(f32::INFINITY, f32::min);
        //     println!("min: {}, max: {}", min, max);
        //     let rgb: Vec<_> = heightmap
        //         .iter()
        //         .map(|&h| ((h - min) / (max - min) * 255.0) as u8)
        //         .collect();
        //     let _ = DynamicImage::ImageLuma8(ImageBuffer::from_raw(64, 128, rgb).unwrap())
        //         .save("heights.png");
        // });

        if let Err(e) = enumerate_depth_cameras(
            buffer,
            localizer_ref,
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
                            k_node: robot_chain
                                .find_link(&link_name)
                                .context("Failed to find camera link")
                                .unwrap()
                                .clone(),
                            ignore_apriltags: observe_apriltags,
                            stream_index,
                        },
                    )
                },
            ),
            &apriltags,
        ) {
            error!("Failed to enumerate depth cameras: {e}");
        }

        let mut finder = Pathfinder::new(Vector2::new(32.0, 16.0), 0.03125);

        let lunabot_stage = Arc::new(AtomicCell::new(LunabotStage::SoftStop));

        let (packet_builder, mut from_lunabase_rx, mut connected) = create_packet_builder(
            self.lunabase_address,
            lunabot_stage.clone(),
            self.max_pong_delay_ms,
        );

        std::thread::spawn(move || {
            run_ai(
                robot_chain,
                |action, inputs| match action {
                    Action::SetStage(stage) => {
                        lunabot_stage.store(stage);
                    }
                    Action::SetSteering(steering) => {
                        let (left, right) = steering.get_left_and_right();
                        // TODO
                    }
                    Action::CalculatePath { from, to, mut into } => {
                        let data = shared_thalassic_data.get();
                        finder.append_path(
                            from,
                            to,
                            &data.heightmap,
                            &data.gradmap,
                            1.0,
                            &mut into,
                        );
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

define_app!(pub Main(LunabotApp):  "The lunabot application");
