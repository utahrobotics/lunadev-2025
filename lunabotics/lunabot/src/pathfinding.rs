use crate::utils::distance_between_tuples;
use common::{cell_to_world_point, world_point_to_cell, PathInstruction, PathPoint, THALASSIC_CELL_SIZE, THALASSIC_HEIGHT, THALASSIC_WIDTH};
use nalgebra::{Point3, Scale3, Transform3};
use pathfinding::{grid::Grid, prelude::astar};
use tasker::shared::{SharedData, SharedDataReceiver};
use tracing::error;

use crate::pipelines::thalassic::{set_observe_depth, CellState, ThalassicData};

const REACH: usize = 2;

const MAX_SCAN_ATTEMPTS: usize = 4;

#[derive(Debug)]
/// larger x = further left, so `left` should have a larger numeric value than `right`
pub struct Rect {
    top: f64,
    bottom: f64,
    left: f64,
    right: f64
}

#[derive(Debug)]
pub struct Ellipse {
    h: f64,
    k: f64,
    radius_x: f64,
    radius_y: f64
}

/// units are in cells
#[derive(Debug)]
pub enum Obstacle { Rect(Rect), Ellipse(Ellipse) }
impl Obstacle {
    
    /// width and height must be positive
    pub fn new_rect((left, bottom): (f64, f64), width_meters: f64, height_meters: f64) -> Obstacle {
        let left = left / THALASSIC_CELL_SIZE as f64;
        let bottom = bottom / THALASSIC_CELL_SIZE as f64;
        
        Obstacle::Rect(Rect{
            left,
            bottom,
            right: left - (width_meters / THALASSIC_CELL_SIZE as f64),
            top: bottom + (height_meters / THALASSIC_CELL_SIZE as f64),
        })
    }
    
    pub fn new_ellipse(center: (f64, f64), radius_x_meters: f64, radius_y_meters: f64) -> Obstacle {
        Obstacle::Ellipse(Ellipse { 
            h: center.0 / THALASSIC_CELL_SIZE as f64, 
            k: center.1 / THALASSIC_CELL_SIZE as f64, 
            radius_x: radius_x_meters / THALASSIC_CELL_SIZE as f64, 
            radius_y: radius_y_meters / THALASSIC_CELL_SIZE as f64, 
        })
    }
    
    pub fn new_circle(center: (f64, f64), radius_meters: f64) -> Obstacle {
        Self::new_ellipse(center, radius_meters, radius_meters)
    }
    
    fn contains_cell(&self, cell: (usize, usize)) -> bool {
        let x = cell.0 as f64;
        let y = cell.1 as f64;
        
        match self {
            Obstacle::Rect(Rect{top, bottom, left, right}) => {
                *right <= x && x <= *left && *bottom <= y && y <= *top  // larger x = further left
            },
            Obstacle::Ellipse(Ellipse{h, k, radius_x, radius_y}) => {
                ( ((x - h) * (x - h))  / (radius_x * radius_x) ) + 
                ( ((y - k) * (y - k))  / (radius_y * radius_y) )
                <= 1.0
            },
        }
    }
}


pub struct DefaultPathfinder {
    pub cell_grid: Grid,

    current_robot_radius: f32,
    
    additional_obstacles: Vec<Obstacle>,
    
    /// - cleared once destination is reached
    cells_to_avoid: Vec<(usize, usize)>,

    /// these cells have been scanned `MAX_SCAN_ATTEMPTS` times and yet still unknown.
    /// - cleared once they are scanned
    unscannable_cells: Vec<(usize, usize)>,
    last_unknown_cell: (usize, usize),
    times_blocked_here_in_a_row: usize,
}

impl DefaultPathfinder {
    pub fn new(hardcoded_obstacles: Vec<Obstacle>) -> Self {
        Self {
            cell_grid: Grid::new(THALASSIC_WIDTH as usize, THALASSIC_HEIGHT as usize),

            current_robot_radius: 0.5,
            
            additional_obstacles: hardcoded_obstacles,
            
            cells_to_avoid: vec![],
            
            unscannable_cells: vec![],
            last_unknown_cell: (0, 0),
            times_blocked_here_in_a_row: 0,
        }
    }
    
    /// iterator of `unscannable_cells` and `cells_to_avoid`
    fn all_unsafe_cells(&self) -> impl Iterator<Item = &(usize, usize)> {
        self.unscannable_cells.iter().chain(self.cells_to_avoid.iter())
    }
    
    pub fn avoid_point(&mut self, point: Point3<f64>) {
        self.cells_to_avoid.push(world_point_to_cell(point));
    }
    
    pub fn clear_points_to_avoid(&mut self) {
        self.cells_to_avoid.clear();
    }

    fn get_map_data(
        &mut self,
        shared_thalassic_data: &SharedDataReceiver<ThalassicData>,
        robot_radius: f32,
    ) -> SharedData<ThalassicData> {
        shared_thalassic_data.try_get(); // clear out a previous observation if it exists

        set_observe_depth(true);
        let mut map_data = shared_thalassic_data.get();
        loop {
            if map_data.current_robot_radius_meters == robot_radius {
                break;
            }
            map_data.set_robot_radius(robot_radius);
            drop(map_data);
            map_data = shared_thalassic_data.get();
        }
        set_observe_depth(false);

        // if a previously unscannable cell was scanned, its no longer unscannable
        self.unscannable_cells = self
            .unscannable_cells
            .iter()
            .filter_map(|unknown_cell| match map_data.is_known(*unknown_cell) {
                true => None, // known cells are removed from unscannable_cells
                false => Some(*unknown_cell),
            })
            .collect();

        map_data
    }
    
    fn find_path(
        &self,
        mut start_cell: (usize, usize),
        end_cell: (usize, usize),
        map_data: &SharedData<ThalassicData>,
    ) -> Option<Vec<(usize, usize)>> {
        
        // allows checking if position is known inside `move || {}` closures without moving `map_data`
        let is_known = |pos: (usize, usize)| map_data.is_known(pos);
        
        // a cell is valid if:
        //  - not red 
        //  - far away from any of the cells in `all_unsafe_cells()`
        //  - not in any `hardcoded_obstacles`
        let is_valid_path_point = |pos: (usize, usize)| {
            if map_data.get_cell_state(pos) == CellState::RED { return false; }
                        
            let robot_radius_in_cells = map_data.current_robot_radius_meters / common::THALASSIC_CELL_SIZE;
            
            for unsafe_cell in self.all_unsafe_cells() {
                if distance_between_tuples(pos, *unsafe_cell) < robot_radius_in_cells {
                    return false;
                }
            }
            
            for obstacle in &self.additional_obstacles {
                if obstacle.contains_cell(pos) {
                    return false;
                }
            }
            
            true
        };
        
        
        
        macro_rules! neighbours {
            ($p: ident) => {
                self.cell_grid
                    .dfs_reachable($p, |potential_neighbor| {
                        let (x, y) = potential_neighbor;

                        if x.abs_diff($p.0) > REACH || y.abs_diff($p.1) > REACH {
                            return false;
                        }

                        return is_valid_path_point(potential_neighbor);
                    })
                    .into_iter()
                    .map(move |neighbor| {
                        // unknown cells have 2x the cost
                        let unknown_multiplier = match is_known(neighbor) {
                            true => 1.0,
                            false => 2.0,
                        };

                        (
                            neighbor,
                            // the cost of moving from a to b is the distance between a to b
                            (distance_between_tuples($p, neighbor) * unknown_multiplier * 10000.0)
                                as usize,
                        )
                    })
            };
        }

        let heuristic = move |p: &(usize, usize)| (distance_between_tuples(*p, end_cell) * 10000.0) as usize;

        let mut path = vec![];
        
        println!("{:?} {:?}", start_cell, self.additional_obstacles);

        // prepend a path to safety
        if !is_valid_path_point(start_cell) {
            println!("Current cell is occupied, finding closest safe cell");
            let Some((mut path_to_safety, _)) = astar(
                &start_cell,
                |&p| {
                    self.cell_grid
                        .bfs_reachable(p, |(x, y)| {
                            x.abs_diff(p.0) <= REACH && y.abs_diff(p.1) <= REACH
                        })
                        .into_iter()
                        .map(move |neighbor| {
                            (
                                neighbor,
                                // the cost of moving from a to b is the distance between a to b
                                (distance_between_tuples(p, neighbor) * 10000.0) as usize,
                            )
                        })
                },
                |_| 0,
                |&pos| is_valid_path_point(pos),
            ) else {
                error!("Failed to find path to safety");
                return None;
            };

            start_cell = *path_to_safety.last().unwrap();
            path.append(&mut path_to_safety);
        }

        let Some((mut path_to_goal, _)) = astar(
            &start_cell,
            |&p| neighbours!(p),
            heuristic,
            |p| p == &end_cell,
        ) else {
            return None;
        };

        path.append(&mut path_to_goal);
        
        Some(path)
    }

    pub fn push_path_into(
        &mut self,
        shared_thalassic_data: &SharedDataReceiver<ThalassicData>,
        from: Point3<f64>,
        to: Point3<f64>,
        into: &mut Vec<PathPoint>,
    ) -> bool {
        into.clear();
        let start_cell = world_point_to_cell(from);
        let end_cell = world_point_to_cell(to);

        loop {
            let map_data = self.get_map_data(shared_thalassic_data, self.current_robot_radius);

            let Some(mut path) = self.find_path(start_cell, end_cell, &map_data) else {
                self.current_robot_radius -= 0.1;

                if self.current_robot_radius <= 0.1 {
                    into.clear();
                    tracing::error!(
                        "pathfinder: couldnt find a path even with a robot radius of 0.1"
                    );
                    self.current_robot_radius = 0.5;
                    map_data.set_robot_radius(self.current_robot_radius);
                    map_data.queue_reset_heightmap();
                    return false;
                }

                map_data.set_robot_radius(self.current_robot_radius);

                tracing::warn!(
                    "pathfinder: couldnt find a path, shrinking radius to {}",
                    self.current_robot_radius
                );

                continue;
            };
            
            let cell_to_path_pt = |cell: (usize, usize), instruction: PathInstruction| {
                PathPoint {
                    point: cell_to_world_point(cell, map_data.get_height(cell) as f64),
                    instruction,
                }
            };

            if let Err((i, unknown_cell)) = map_data.is_path_safe_for_robot(start_cell, &mut path) {
                // path got blocked due to this unknown point again
                if self.last_unknown_cell == unknown_cell {
                    self.times_blocked_here_in_a_row += 1;

                    if self.times_blocked_here_in_a_row >= MAX_SCAN_ATTEMPTS {
                        println!(
                            "pathfinder: giving up trying to scan {:?}, trying another path",
                            unknown_cell
                        );
                        self.unscannable_cells.push(unknown_cell);
                    }
                }
                // path got stuck due to this unknown point for the 1st time
                else {
                    self.last_unknown_cell = unknown_cell;
                    self.times_blocked_here_in_a_row = 1;

                    path.truncate(i);

                    into.extend(
                        path.iter()
                            .map(|pos| cell_to_path_pt(*pos, PathInstruction::MoveTo)),
                    );
                    into.push(cell_to_path_pt(unknown_cell, PathInstruction::FaceTowards));
                    break;
                }
            }
            // path is safe
            else {
                self.times_blocked_here_in_a_row = 0;
                into.extend(
                    path.iter()
                        .map(|pos| cell_to_path_pt(*pos, PathInstruction::MoveTo)),
                );
                break;
            }
        }

        true
    }
}
