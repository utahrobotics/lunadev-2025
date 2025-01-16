use std::{
    num::NonZeroU32,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use arc_swap::ArcSwapOption;
use crossbeam::{
    atomic::AtomicCell,
    sync::{Parker, Unparker},
};
use gputter::is_gputter_initialized;
use nalgebra::Vector2;
use tasker::shared::OwnedData;
use thalassic::{Occupancy, PointCloudStorage, ThalassicBuilder};

static OBSERVE_DEPTH: AtomicBool = AtomicBool::new(false);
static DEPTH_UNPARKER: ArcSwapOption<Unparker> = ArcSwapOption::const_empty();

pub fn set_observe_depth(value: bool) {
    OBSERVE_DEPTH.store(value, Ordering::Release);
    if value {
        if let Some(inner) = &*DEPTH_UNPARKER.load() {
            inner.unpark();
        }
    }
}

pub fn get_observe_depth() -> bool {
    OBSERVE_DEPTH.load(Ordering::Acquire)
}

const CELL_COUNT: u32 = 128 * 256;

pub struct ThalassicData {
    pub heightmap: [f32; CELL_COUNT as usize],
    pub gradmap: [f32; CELL_COUNT as usize],
    pub expanded_obstacle_map: [Occupancy; CELL_COUNT as usize],
    pub current_robot_radius: f32,
    new_robot_radius: AtomicCell<Option<f32>>,
}

impl Default for ThalassicData {
    fn default() -> Self {
        Self {
            heightmap: [0.0; CELL_COUNT as usize],
            gradmap: [0.0; CELL_COUNT as usize],
            expanded_obstacle_map: [Occupancy::FREE; CELL_COUNT as usize],
            new_robot_radius: AtomicCell::new(None),
            current_robot_radius: 0.25,
        }
    }
}

impl ThalassicData {
    pub fn set_robot_radius(&self, radius: f32) {
        self.new_robot_radius.store(Some(radius));
    }
}

pub struct PointsStorageChannel {
    projected: AtomicCell<Option<PointCloudStorage>>,
    finished: AtomicCell<Option<PointCloudStorage>>,
    image_size: Vector2<NonZeroU32>,
}

impl PointsStorageChannel {
    pub fn new_for(storage: &PointCloudStorage) -> Self {
        Self {
            projected: AtomicCell::new(None),
            image_size: storage.get_image_size(),
            finished: AtomicCell::new(None),
        }
    }

    pub fn set_projected(&self, projected: PointCloudStorage) {
        self.projected.store(Some(projected));
    }

    pub fn get_finished(&self) -> Option<PointCloudStorage> {
        self.finished.take()
    }
}

pub fn spawn_thalassic_pipeline(
    buffer: OwnedData<ThalassicData>,
    point_cloud_channels: Box<[Arc<PointsStorageChannel>]>,
) {
    let mut buffer = buffer.pessimistic_share();
    let Some(max_point_count) = point_cloud_channels
        .iter()
        .map(|channel| channel.image_size.x.get() * channel.image_size.y.get())
        .max()
    else {
        return;
    };

    let max_triangle_count = point_cloud_channels
        .iter()
        .map(|channel| (channel.image_size.x.get() - 1) * (channel.image_size.y.get() - 1) * 2)
        .max()
        .unwrap();

    if is_gputter_initialized() {
        let mut pipeline = ThalassicBuilder {
            heightmap_dimensions: Vector2::new(
                NonZeroU32::new(128).unwrap(),
                NonZeroU32::new(256).unwrap(),
            ),
            cell_size: 0.03125,
            max_point_count: NonZeroU32::new(max_point_count).unwrap(),
            max_triangle_count: NonZeroU32::new(max_triangle_count).unwrap(),
        }
        .build();

        let parker = Parker::new();
        DEPTH_UNPARKER.store(Some(parker.unparker().clone().into()));

        std::thread::spawn(move || loop {
            if !get_observe_depth() {
                parker.park();
            }
            let mut points_vec = vec![];

            for channel in &point_cloud_channels {
                let Some(points) = channel.projected.take() else {
                    continue;
                };
                points_vec.push((channel, points));
            }

            if !points_vec.is_empty() {
                let mut owned = buffer.recall_or_replace_with(Default::default);
                let ThalassicData {
                    heightmap,
                    gradmap,
                    expanded_obstacle_map,
                    new_robot_radius,
                    current_robot_radius,
                } = &mut *owned;

                if let Some(radius) = new_robot_radius.take() {
                    *current_robot_radius = radius;
                    pipeline.set_radius(radius);
                }

                for (channel, mut points) in points_vec {
                    points =
                        pipeline.provide_points(points, heightmap, gradmap, expanded_obstacle_map);
                    channel.finished.store(Some(points));
                }

                buffer = owned.pessimistic_share();
            }
        });
    }
}
