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
    app::Application, callbacks::caller::CallbacksStorage, log::error, task::SyncTask, BlockOn,
};

use crate::{
    apps::log_teleop_messages,
    localization::{Localizer, LocalizerRef},
    obstacles::heightmap::heightmap_strategy,
};

use super::{create_packet_builder, create_robot_chain, wait_for_ctrl_c, PointCloudCallbacks};

#[derive(Serialize, Deserialize)]
pub struct LunabotApp {
    pub lunabase_address: SocketAddr,
}

const PROJECTION_SIZE: Vector2<u32> = Vector2::new(36, 24);

impl Application for LunabotApp {
    const APP_NAME: &'static str = "main";

    const DESCRIPTION: &'static str = "The lunabot application";

    fn run(self) {
        log_teleop_messages();

        let robot_chain = create_robot_chain();
        let localizer_ref = LocalizerRef::default();
        Localizer {
            robot_chain: robot_chain.clone(),
            lunasim_stdin: None,
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

        let raw_pcl_callbacks = Arc::new(PointCloudCallbacks::default());
        let raw_pcl_callbacks_ref = raw_pcl_callbacks.get_ref();

        let heightmap_ref = heightmap_strategy(PROJECTION_SIZE, &raw_pcl_callbacks_ref);

        let lunabot_stage = Arc::new(AtomicCell::new(LunabotStage::SoftStop));

        let (packet_builder, from_lunabase_rx) =
            create_packet_builder(self.lunabase_address, lunabot_stage.clone());

        std::thread::spawn(move || {
            run_ai(robot_chain, |action, inputs| {
                debug_assert!(inputs.is_empty());
                match action {
                    Action::WaitForLunabase => {
                        while let Ok(msg) = from_lunabase_rx.try_recv() {
                            inputs.push(Input::FromLunabase(msg));
                        }
                        if inputs.is_empty() {
                            let Ok(msg) = from_lunabase_rx.recv() else {
                                error!("Lunabase message channel closed");
                                loop {
                                    std::thread::park();
                                }
                            };
                            inputs.push(Input::FromLunabase(msg));
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
                    Action::WaitUntil(deadline) => match from_lunabase_rx.recv_deadline(deadline) {
                        Ok(msg) => inputs.push(Input::FromLunabase(msg)),
                        Err(e) => match e {
                            mpsc::RecvTimeoutError::Timeout => {}
                            mpsc::RecvTimeoutError::Disconnected => {
                                error!("Lunabase message channel closed");
                                std::thread::sleep_until(deadline);
                            }
                        },
                    },
                }
            });
        });

        wait_for_ctrl_c();
    }
}
