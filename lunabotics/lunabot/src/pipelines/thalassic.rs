use std::{
    collections::{HashSet, VecDeque},
    num::NonZeroU32,
    sync::atomic::{AtomicBool, Ordering},
};

use common::{THALASSIC_CELL_COUNT, THALASSIC_CELL_SIZE, THALASSIC_HEIGHT, THALASSIC_WIDTH};
use crossbeam::atomic::AtomicCell;
use gputter::is_gputter_initialized;
use nalgebra::Vector2;
use tasker::shared::OwnedData;
use thalassic::{Occupancy, ThalassicBuilder, ThalassicPipelineRef};

use crate::utils::distance_between_tuples;

static OBSERVE_DEPTH: AtomicBool = AtomicBool::new(false);

pub fn set_observe_depth(value: bool) {
    OBSERVE_DEPTH.store(value, Ordering::Release);
}

pub fn get_observe_depth() -> bool {
    OBSERVE_DEPTH.load(Ordering::Acquire)
}

pub struct ThalassicData {
    pub heightmap: [f32; THALASSIC_CELL_COUNT as usize],
    pub gradmap: [f32; THALASSIC_CELL_COUNT as usize],
    pub expanded_obstacle_map: [Occupancy; THALASSIC_CELL_COUNT as usize],
    pub current_robot_radius_meters: f32,
    new_robot_radius: AtomicCell<Option<f32>>,
    reset_heightmap: AtomicBool,
}

#[derive(PartialEq, Debug)]
pub enum CellState {
    RED,
    GREEN,
    UNKNOWN,
}

impl Default for ThalassicData {
    fn default() -> Self {
        Self {
            heightmap: [0.0; THALASSIC_CELL_COUNT as usize],
            gradmap: [0.0; THALASSIC_CELL_COUNT as usize],
            expanded_obstacle_map: [Occupancy::FREE; THALASSIC_CELL_COUNT as usize],
            new_robot_radius: AtomicCell::new(None),
            current_robot_radius_meters: 0.25,
            reset_heightmap: AtomicBool::new(false),
        }
    }
}

impl ThalassicData {
    const MAP_WIDTH: usize = THALASSIC_WIDTH as usize;
    const MAP_HEIGHT: usize = THALASSIC_HEIGHT as usize;

    #[cfg(feature="production")]
    fn index_to_xy(index: usize) -> (usize, usize) {
        (index % Self::MAP_WIDTH, index / Self::MAP_WIDTH)
    }

    fn xy_to_index((x, y): (usize, usize)) -> usize {
        y * Self::MAP_WIDTH + x
    }

    pub fn in_bounds((x, y): (i32, i32)) -> bool {
        x >= 0 && x < Self::MAP_WIDTH as i32 && y >= 0 && y < Self::MAP_HEIGHT as i32
    }

    pub fn set_robot_radius(&self, radius: f32) {
        self.new_robot_radius.store(Some(radius));
    }

    pub fn queue_reset_heightmap(&self) {
        self.reset_heightmap.store(true, Ordering::Relaxed);
    }

    /// whether a target position is safe for the robot to be
    /// - the target position must be green
    /// - every single cell within `robot radius` of the target must be known
    ///
    /// cells within `current_robot_radius` of `robot_cell_pos` are always safe
    ///
    /// if unsafe, returns `Err(closest unknown cell that makes the target unsafe)`
    pub fn is_safe_for_robot(
        &self,
        robot_cell_pos: (usize, usize),
        target_cell_pos: (usize, usize),
    ) -> Result<(), (usize, usize)> {
        if robot_cell_pos == target_cell_pos {
            return Ok(());
        }

        let robot_cell_radius = (self.current_robot_radius_meters / THALASSIC_CELL_SIZE).ceil();

        if distance_between_tuples(robot_cell_pos, target_cell_pos) <= robot_cell_radius {
            return Ok(());
        }

        // if the target cell is near the robot, its okay if its not green
        if self.get_cell_state(target_cell_pos) != CellState::GREEN
            && distance_between_tuples(target_cell_pos, robot_cell_pos) > robot_cell_radius
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

                if !Self::in_bounds(nearby_cell) {
                    continue;
                };
                if distance_between_tuples(nearby_cell_usize, target_cell_pos) > robot_cell_radius {
                    continue;
                };
                if !visited.insert(nearby_cell) {
                    continue;
                };

                let (x, y) = nearby_cell;
                q.push_back((x + 1, y));
                q.push_back((x - 1, y));
                q.push_back((x, y + 1));
                q.push_back((x, y - 1));

                // if a cell is not inside the robot AND and unknown then target cell is dangerous
                if distance_between_tuples(nearby_cell_usize, robot_cell_pos) > robot_cell_radius
                    && !self.is_known(nearby_cell_usize)
                {
                    return Err(nearby_cell_usize);
                }
            }
        }

        Ok(())
    }

    /// calls `is_safe_for_robot()` for each point in `path`
    ///
    /// if unsafe, returns `Err( (i, closest unknown cell that makes path[i] unsafe) )`
    pub fn is_path_safe_for_robot(
        &self,
        robot_cell_pos: (usize, usize),
        path: &Vec<(usize, usize)>,
    ) -> Result<(), (usize, (usize, usize))> {
        for (i, pt) in path.iter().enumerate() {
            if let Err(unknown_cell) = self.is_safe_for_robot(robot_cell_pos, *pt) {
                return Err((i, unknown_cell));
            }
        }

        Ok(())
    }

    pub fn get_cell_state(&self, pos: (usize, usize)) -> CellState {
        match self.expanded_obstacle_map[Self::xy_to_index(pos)].occupied() {
            true => CellState::RED,
            false => match self.get_height(pos) == 0.0 {
                true => CellState::UNKNOWN,
                false => CellState::GREEN,
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

pub fn spawn_thalassic_pipeline(
    buffer: OwnedData<ThalassicData>,
    max_point_count: u32,
) -> ThalassicPipelineRef {
    let mut buffer = buffer.pessimistic_share();

    if is_gputter_initialized() {
        let mut pipeline = ThalassicBuilder {
            heightmap_dimensions: Vector2::new(
                NonZeroU32::new(128).unwrap(),
                NonZeroU32::new(256).unwrap(),
            ),
            cell_size: THALASSIC_CELL_SIZE,
            max_point_count: NonZeroU32::new(max_point_count).unwrap(),
            feature_size_cells: 8,
            min_feature_count: 50,
        }
        .build();

        let reference = pipeline.get_ref();

        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(100));

            if !pipeline.will_process() {
                continue;
            }

            let mut owned = buffer.recall_or_replace_with(Default::default);
            let ThalassicData {
                heightmap,
                gradmap,
                expanded_obstacle_map,
                new_robot_radius,
                current_robot_radius_meters: current_robot_radius,
                reset_heightmap,
            } = &mut *owned;

            if let Some(radius) = new_robot_radius.take() {
                *current_robot_radius = radius;
                pipeline.set_radius(radius);
            }

            if *reset_heightmap.get_mut() {
                *reset_heightmap.get_mut() = false;
                pipeline.reset_heightmap();
            }

            pipeline.process(heightmap, gradmap, expanded_obstacle_map);

            #[cfg(feature = "production")]
            if let Some(recorder) = crate::apps::RECORDER.get() {
                if let Err(e) = recorder.recorder.log(
                    format!("{}/heightmap", crate::apps::ROBOT),
                    &rerun::Points3D::new(heightmap.iter().enumerate().map(|(i, &height)| {
                        rerun::Position3D::new(
                            (i % THALASSIC_WIDTH as usize) as f32 * THALASSIC_CELL_SIZE,
                            height,
                            (i / THALASSIC_WIDTH as usize) as f32 * THALASSIC_CELL_SIZE,
                        )
                    })),
                ) {
                    tracing::error!("Failed to log heightmap: {e}");
                }
            }

            #[cfg(feature="production")]
            if let Some(recorder) = crate::apps::RECORDER.get() {
                if let Err(e) = recorder.recorder.log(
                    format!("{}/expanded_obstacle_map",crate::apps::ROBOT),
                    &rerun::Points3D::new((0..THALASSIC_CELL_COUNT).map(|i| {
                        let i = i as usize;
                        rerun::Position3D::new(
                            (i % THALASSIC_WIDTH as usize) as f32 * THALASSIC_CELL_SIZE, 
                            0.0, 
                            (i / THALASSIC_WIDTH as usize) as f32 * THALASSIC_CELL_SIZE
                        )
                    })).with_colors((0..THALASSIC_CELL_COUNT).map(|i| {
                        let pos = ThalassicData::index_to_xy(i as usize);
                        let color = match (&owned).get_cell_state(pos) {
                            CellState::GREEN => {
                                rerun::Color::from_rgb(0,255,0)
                            }
                            CellState::RED => {
                                rerun::Color::from_rgb(255,0,0)
                            }
                            CellState::UNKNOWN => {
                                rerun::Color::from_rgb(77, 77, 77)
                            }
                        };
                        color
                    })).with_radii(
                        (0..THALASSIC_CELL_COUNT).map(|_| {
                            0.01
                        })
                    )
                ) {
                    tracing::error!("Failed to log expanded obstacle map: {e}");
                }
            }

            buffer = owned.pessimistic_share();
        });

        reference
    } else {
        ThalassicPipelineRef::noop()
    }
}
