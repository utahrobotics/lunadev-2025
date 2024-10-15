// #![allow(unused_variables)]

use std::{
    net::SocketAddr,
    sync::{mpsc, Arc},
};

use common::LunabotStage;
use crossbeam::atomic::AtomicCell;
use fitter::utils::CameraProjection;
use lunabot_ai::{run_ai, Action, Input};
use nalgebra::{Vector2, Vector4};
use recycler::Recycler;
use serde::{Deserialize, Serialize};
use urobotics::{
    app::Application, callbacks::caller::CallbacksStorage, log::error, tokio, BlockOn,
};

use crate::{
    apps::log_teleop_messages, localization::Localizer, obstacles::heightmap::heightmap_strategy,
};

use super::{create_packet_builder, create_robot_chain, wait_for_ctrl_c, PointCloudCallbacks};

#[derive(Serialize, Deserialize)]
pub struct LunabotApp {
    pub lunabase_address: SocketAddr,
    #[serde(default = "default_max_pong_delay_ms")]
    pub max_pong_delay_ms: u64
}

fn default_max_pong_delay_ms() -> u64 {
    1500
}

const PROJECTION_SIZE: Vector2<u32> = Vector2::new(36, 24);

impl Application for LunabotApp {
    const APP_NAME: &'static str = "main";

    const DESCRIPTION: &'static str = "The lunabot application";

    fn run(self) {
        log_teleop_messages();

        let robot_chain = create_robot_chain();
        let localizer = Localizer::new(robot_chain.clone(), None);
        let localizer_ref = localizer.get_ref();
        std::thread::spawn(|| localizer.run());

        let depth_project = match CameraProjection::new(10.392, PROJECTION_SIZE, 0.01).block_on() {
            Ok(x) => Some(Arc::new(x)),
            Err(e) => {
                error!("Failed to create camera projector: {e}");
                None
            }
        };

        let camera_link = robot_chain.find_link("depth_camera_link").unwrap().clone();
        let points_buffer_recycler = Recycler::<Box<[Vector4<f32>]>>::default();

        let raw_pcl_callbacks = Arc::new(PointCloudCallbacks::default());
        let raw_pcl_callbacks_ref = raw_pcl_callbacks.get_ref();

        let heightmap_ref = heightmap_strategy(PROJECTION_SIZE, &raw_pcl_callbacks_ref);

        let lunabot_stage = Arc::new(AtomicCell::new(LunabotStage::SoftStop));

        let (packet_builder, mut from_lunabase_rx, mut connected) =
            create_packet_builder(self.lunabase_address, lunabot_stage.clone(), self.max_pong_delay_ms);

        std::thread::spawn(move || {
            run_ai(robot_chain, |action, inputs| {
                debug_assert!(inputs.is_empty());
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
                                    _ = connected.wait_disconnect() => {
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
                        todo!()
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
                                _ = connected.wait_disconnect() => {
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
