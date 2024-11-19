#![allow(unused_imports)]

use std::{
    net::SocketAddr,
    sync::{mpsc, Arc},
};

use anyhow::Context;
use camera::enumerate_cameras;
use common::LunabotStage;
use crossbeam::atomic::AtomicCell;
use depth::enumerate_depth_cameras;
use fxhash::FxHashMap;
use gputter::init_gputter_blocking;
use lunabot_ai::{run_ai, Action, Input, PollWhen};
use nalgebra::{UnitVector3, Vector2, Vector4};
use recycler::Recycler;
use serde::{Deserialize, Serialize};
use urobotics::{
    app::Application, callbacks::caller::CallbacksStorage, get_tokio_handle, log::error, tokio,
    BlockOn,
};

use crate::{
    apps::log_teleop_messages, localization::Localizer,
    pipelines::thalassic::spawn_thalassic_pipeline,
};

use super::{create_packet_builder, create_robot_chain, wait_for_ctrl_c};

mod camera;
mod depth;

#[derive(Serialize, Deserialize, Debug)]
struct CameraInfo {
    link_name: String,
    focal_length_px: f64,
}

#[derive(Serialize, Deserialize, Debug)]
struct DepthCameraInfo {
    link_name: String,
    observe_apriltags: bool
}

#[derive(Serialize, Deserialize)]
pub struct LunabotApp {
    lunabase_address: SocketAddr,
    #[serde(default = "super::default_max_pong_delay_ms")]
    max_pong_delay_ms: u64,
    #[serde(default)]
    cameras: FxHashMap<String, CameraInfo>,
    #[serde(default)]
    depth_cameras: FxHashMap<String, DepthCameraInfo>,
}

// const PROJECTION_SIZE: Vector2<u32> = Vector2::new(36, 24);

impl Application for LunabotApp {
    const APP_NAME: &'static str = "main";

    const DESCRIPTION: &'static str = "The lunabot application";

    fn run(self) {
        log_teleop_messages();
        if let Err(e) = init_gputter_blocking() {
            error!("Failed to initialize gputter: {e}");
        }

        let _guard = get_tokio_handle().enter();

        let robot_chain = create_robot_chain();
        let localizer = Localizer::new(robot_chain.clone(), None);
        let localizer_ref = localizer.get_ref();
        std::thread::spawn(|| localizer.run());

        if let Err(e) = enumerate_cameras(
            localizer_ref.clone(),
            self.cameras.into_iter().map(
                |(
                    serial,
                    CameraInfo {
                        link_name,
                        focal_length_px,
                    },
                )| {
                    (
                        serial,
                        camera::CameraInfo {
                            k_node: robot_chain
                                .find_link(&link_name)
                                .context("Failed to find camera link")
                                .unwrap()
                                .clone(),
                            focal_length_px,
                        },
                    )
                },
            ),
        ) {
            error!("Failed to enumerate cameras: {e}");
        }

        let (heightmap_callbacks, result) = enumerate_depth_cameras(
            localizer_ref,
            self.depth_cameras.into_iter().map(
                |(
                    serial,
                    DepthCameraInfo {
                        link_name,
                        observe_apriltags,
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
                            observe_apriltags,
                        },
                    )
                },
            ),
        );

        if let Err(e) = result {
            error!("Failed to enumerate depth cameras: {e}");
        }

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
