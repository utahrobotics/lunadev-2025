use std::{net::SocketAddr, sync::Arc};

use super::{depth::enumerate_depth_cameras, subaddress_of};
use anyhow::Context;
use common::LunabotStage;
use crossbeam::atomic::AtomicCell;
use fxhash::FxHashMap;
use gputter::init_gputter_blocking;
use simple_motion::{ChainBuilder, NodeSerde};
use tasker::shared::OwnedData;
use tracing::error;

use crate::{
    apps::log_teleop_messages,
    localization::Localizer,
    pipelines::thalassic::{set_observe_depth, ThalassicData},
};

use super::{create_packet_builder, DepthCameraInfo};

pub struct DatavizApp {
    pub lunabase_address: SocketAddr,
    pub lunabase_data_address: Option<SocketAddr>,
    pub max_pong_delay_ms: u64,
    pub depth_cameras: FxHashMap<String, DepthCameraInfo>,
    pub robot_layout: String,
}

impl DatavizApp {
    pub fn run(self) {
        log_teleop_messages();
        if let Err(e) = init_gputter_blocking() {
            error!("Failed to initialize gputter: {e}");
        }

        let robot_chain = NodeSerde::from_reader(
            std::fs::File::open(self.robot_layout).expect("Failed to read robot chain"),
        )
        .expect("Failed to parse robot chain");
        let robot_chain = ChainBuilder::from(robot_chain).finish_static();

        let localizer = Localizer::new(robot_chain.clone());
        let localizer_ref = localizer.get_ref();

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
                        super::depth::DepthCameraInfo {
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
            &[],
        );
        let data_address = self
            .lunabase_data_address
            .unwrap_or_else(|| subaddress_of(self.lunabase_address, 9400));
        common::thalassic::lunabot_task(data_address, move |data, _points| {
            set_observe_depth(true);
            let incoming_data = shared_thalassic_data.get();
            data.gradmap = incoming_data.gradmap;
            data.heightmap = incoming_data.heightmap;
            data.expanded_obstacle_map = std::array::from_fn(|i| {
                common::thalassic::Occupancy::new(data.expanded_obstacle_map[i].occupied())
            });
            set_observe_depth(false);
        });

        let lunabot_stage = Arc::new(AtomicCell::new(LunabotStage::SoftStop));

        let (_packet_builder, _from_lunabase_rx, _connected) =
            create_packet_builder(self.lunabase_address, lunabot_stage, self.max_pong_delay_ms);

        loop {
            std::thread::park();
        }
    }
}
