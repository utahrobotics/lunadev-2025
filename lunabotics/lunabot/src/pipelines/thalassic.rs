use std::{
    num::NonZeroU32,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use arc_swap::ArcSwapOption;
use common::{THALASSIC_CELL_COUNT, THALASSIC_WIDTH, THALASSIC_HEIGHT};
use crossbeam::{
    atomic::AtomicCell,
    sync::{Parker, Unparker},
};
use gputter::is_gputter_initialized;
use nalgebra::Vector2;
use tasker::shared::OwnedData;
use thalassic::{Occupancy, PointCloudStorage, ThalassicBuilder};

use crate::utils::distance_between_tuples;

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

pub struct ThalassicData {
    pub heightmap: [f32; THALASSIC_CELL_COUNT as usize],
    pub gradmap: [f32; THALASSIC_CELL_COUNT as usize],
    pub expanded_obstacle_map: [Occupancy; THALASSIC_CELL_COUNT as usize],
    pub current_robot_radius: f32,
    new_robot_radius: AtomicCell<Option<f32>>,
}

impl Default for ThalassicData {
    fn default() -> Self {
        Self {
            heightmap: [0.0; THALASSIC_CELL_COUNT as usize],
            gradmap: [0.0; THALASSIC_CELL_COUNT as usize],
            expanded_obstacle_map: [Occupancy::FREE; THALASSIC_CELL_COUNT as usize],
            new_robot_radius: AtomicCell::new(None),
            current_robot_radius: 0.25,
        }
    }
}

impl ThalassicData {

    const MAP_WIDTH: usize = THALASSIC_WIDTH as usize;
    const MAP_HEIGHT: usize = THALASSIC_HEIGHT as usize;

    fn index_to_xy(index: usize) -> (usize, usize) {
        (index % Self::MAP_WIDTH, index / Self::MAP_WIDTH)
    }

    fn xy_to_index((x, y): (usize, usize)) -> usize {
        y * Self::MAP_WIDTH + x
    }
    
    fn in_bounds(&self, (x, y): (i32, i32)) -> bool {
        x >= 0 && x < Self::MAP_WIDTH as i32 && y >= 0 && y < Self::MAP_HEIGHT as i32
    }

    pub fn set_robot_radius(&self, radius: f32) {
        self.new_robot_radius.store(Some(radius));
    }


    /// whether this position is safe for the robot to be
    /// - this position must be green
    /// - every single cell within `robot radius` of this position must be known
    pub fn is_safe_for_robot(&self, pos: (usize, usize)) -> bool {

        if self.is_occupied(pos) {
            return false;
        }

        let radius = self.current_robot_radius.ceil() as i32;
        let (pos_x, pos_y) = (pos.0 as i32, pos.1 as i32);

        for x in (pos_x - radius)..(pos_x + radius) {
            for y in (pos_y - radius)..(pos_y + radius) {

                if !self.in_bounds((x, y)) { continue; }

                let nearby_cell = (x as usize, y as usize);

                if 
                    distance_between_tuples(nearby_cell, pos) <= self.current_robot_radius &&
                    !self.is_known(nearby_cell) 
                {
                    return false;
                }
            }
        }

        true
    }

    pub fn is_known(&self, pos: (usize, usize)) -> bool {
        self.get_height(pos) != 0.0
    }

    pub fn is_occupied(&self, pos: (usize, usize)) -> bool {
        self.expanded_obstacle_map[Self::xy_to_index(pos)].occupied()
    }
    
    pub fn get_height(&self, pos: (usize, usize)) -> f32 {
        self.heightmap[Self::xy_to_index(pos)]
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
