use std::{
    collections::{HashSet, VecDeque}, num::NonZeroU32, sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    }
};

use arc_swap::ArcSwapOption;
use common::{THALASSIC_CELL_COUNT, THALASSIC_CELL_SIZE, THALASSIC_HEIGHT, THALASSIC_WIDTH};
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

#[derive(PartialEq, Debug)]
pub enum CellState { RED, GREEN, UNKNOWN }

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

    // fn index_to_xy(index: usize) -> (usize, usize) {
    //     (index % Self::MAP_WIDTH, index / Self::MAP_WIDTH)
    // }

    fn xy_to_index((x, y): (usize, usize)) -> usize {
        y * Self::MAP_WIDTH + x
    }
    
    pub fn in_bounds((x, y): (i32, i32)) -> bool {
        x >= 0 && x < Self::MAP_WIDTH as i32 && y >= 0 && y < Self::MAP_HEIGHT as i32
    }

    pub fn set_robot_radius(&self, radius: f32) {
        self.new_robot_radius.store(Some(radius));
    }


    /// whether a target position is safe for the robot to be
    /// - the target position must be green
    /// - every single cell within `robot radius` of the target must be known
    /// 
    /// if unsafe, returns `Err(the closest unknown cell that makes the target unsafe)`
    pub fn is_safe_for_robot(&self, robot_cell_pos: (usize, usize), target_cell_pos: (usize, usize)) -> Result<(), (usize, usize)> {
        
        if robot_cell_pos == target_cell_pos {
            return Ok(());
        }
        
        let robot_cell_radius = (self.current_robot_radius / THALASSIC_CELL_SIZE).ceil();

        // if the target cell is near the robot, its okay if its not green
        if 
            self.get_cell_state(target_cell_pos) != CellState::GREEN && 
            distance_between_tuples(target_cell_pos, robot_cell_pos) > robot_cell_radius 
        {
            return Err(target_cell_pos);
        }

        let (pos_x, pos_y) = (target_cell_pos.0 as i32, target_cell_pos.1 as i32);


        // using BFS so that the first unknown cell found when scanning the area 
        // around `target_cell_pos` is also the closest unknown cell to `target_cell_pos` 

        let mut q: VecDeque<(i32, i32)> = VecDeque::new();
        let mut visited: HashSet<(i32, i32)> = HashSet::new();

        q.push_front((pos_x, pos_y));
        
        while !q.is_empty() {
            for _ in 0..q.len() {
                let nearby_cell = q.pop_front().unwrap();
                let nearby_cell_usize = (nearby_cell.0 as usize, nearby_cell.1 as usize);
                
                if !Self::in_bounds(nearby_cell) { continue };
                if distance_between_tuples(nearby_cell_usize, target_cell_pos) > robot_cell_radius { continue };
                if !visited.insert(nearby_cell) { continue };
                
                let (x, y) = nearby_cell;
                q.push_back((x + 1, y));
                q.push_back((x - 1, y));
                q.push_back((x, y + 1));
                q.push_back((x, y - 1));

                // if a cell is not inside the robot AND and unknown then target cell is dangerous                
                if 
                    distance_between_tuples(nearby_cell_usize, robot_cell_pos) > robot_cell_radius &&
                    !self.is_known(nearby_cell_usize) 
                {
                    return Err(nearby_cell_usize);
                }
            }
        }

        Ok(())
    }

    pub fn get_cell_state(&self, pos: (usize, usize)) -> CellState {
        
        match self.expanded_obstacle_map[Self::xy_to_index(pos)].occupied() {
            true => CellState::RED,
            false => {
                match self.get_height(pos) == 0.0 {
                    true => CellState::UNKNOWN,
                    false => CellState::GREEN,
                }
            },
        }
    }

    pub fn is_known(&self, pos: (usize, usize)) -> bool {
        self.get_cell_state(pos) != CellState::UNKNOWN
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
            cell_size: THALASSIC_CELL_SIZE,
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
