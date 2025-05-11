use crate::utils::distance_between_tuples;
use common::{Obstacle, PathInstruction, PathKind, PathPoint, THALASSIC_CELL_SIZE, THALASSIC_HEIGHT, THALASSIC_WIDTH};
use pathfinding::{grid::Grid, prelude::astar};
use tasker::shared::{SharedData, SharedDataReceiver};
use tracing::error;

use crate::pipelines::thalassic::{set_observe_depth, CellState, ThalassicData};

const REACH: usize = 2;

const MAX_SCAN_ATTEMPTS: usize = 4;

pub struct DefaultPathfinder {
    pub cell_grid: Grid,

    current_robot_radius_meters: f32,
    
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

            current_robot_radius_meters: 0.5,
            
            additional_obstacles: hardcoded_obstacles,
            
            cells_to_avoid: vec![],
            
            unscannable_cells: vec![],
            last_unknown_cell: (0, 0),
            times_blocked_here_in_a_row: 0,
        }
    }
    
    pub fn current_robot_radius_cells(&self) -> f32 {
        self.current_robot_radius_meters / common::THALASSIC_CELL_SIZE
    }
    
    /// iterator of `unscannable_cells` and `cells_to_avoid`
    fn all_unsafe_cells(&self) -> impl Iterator<Item = &(usize, usize)> {
        self.unscannable_cells.iter().chain(self.cells_to_avoid.iter())
    }
    
    pub fn avoid_cell(&mut self, cell: (usize, usize)) {
        self.cells_to_avoid.push(cell);
    }
    
    pub fn clear_cells_to_avoid(&mut self) {
        self.cells_to_avoid.clear();
    }
    
    pub fn add_additional_obstacle(&mut self, obstacle: Obstacle) {
        self.additional_obstacles.push(obstacle);
    }
    
    pub fn within_additional_obstacle(&self, cell: (usize, usize)) -> bool {
        for obstacle in &self.additional_obstacles {
            if obstacle.contains_cell(cell) {
                return true
            }
        }
        false
    }

    pub fn get_map_data(
        &mut self,
        shared_thalassic_data: &SharedDataReceiver<ThalassicData>,
    ) -> SharedData<ThalassicData> {
        shared_thalassic_data.try_get(); // clear out a previous observation if it exists

        set_observe_depth(true);
        let mut map_data = shared_thalassic_data.get();
        loop {
            if map_data.current_robot_radius_meters == self.current_robot_radius_meters {
                break;
            }
            map_data.set_robot_radius(self.current_robot_radius_meters);
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
    
    fn find_raw_path(
        &self,
        mut start_cell: (usize, usize),
        end_cell: (usize, usize),
        map_data: &SharedData<ThalassicData>,
    ) -> Option<Vec<(usize, usize)>> {
        tracing::info!("finding path {:?} -> {:?}", start_cell, end_cell);
        
        // allows checking if position is known inside `move || {}` closures without moving `map_data`
        let is_known = |pos: (usize, usize)| map_data.is_known(pos);
        
        // a cell is valid if:
        //  - not red 
        //  - far away from any of the cells in `all_unsafe_cells()`
        //  - not in any `additional_obstacles`
        let is_valid_path_point = |pos: (usize, usize)| {
            if map_data.get_cell_state(pos) == CellState::RED { return false; }
                        
            let robot_radius_in_cells = self.current_robot_radius_cells();
            
            for unsafe_cell in self.all_unsafe_cells() {
                if distance_between_tuples(pos, *unsafe_cell) < robot_radius_in_cells {
                    return false;
                }
            }
            
            if self.within_additional_obstacle(pos) { return false }
            
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

    pub fn find_path(
        &mut self,
        shared_thalassic_data: &SharedDataReceiver<ThalassicData>,
        start_cell: (usize, usize),
        end_cell: (usize, usize),
        path_kind: PathKind,
    ) -> Result<Vec<PathPoint>, ()> {
        
        let mut res: Vec<PathPoint> = vec![];
        
        loop {
            let map_data = self.get_map_data(shared_thalassic_data);

            let Some(mut raw_path) = self.find_raw_path(start_cell, end_cell, &map_data) else {
                self.current_robot_radius_meters -= 0.1;

                if self.current_robot_radius_meters <= 0.1 {
                    tracing::error!(
                        "pathfinder: couldnt find a path even with a robot radius of 0.1"
                    );
                    self.current_robot_radius_meters = 0.5;
                    map_data.set_robot_radius(self.current_robot_radius_meters);
                    map_data.queue_reset_heightmap();
                    return Err(());
                }

                map_data.set_robot_radius(self.current_robot_radius_meters);

                tracing::warn!(
                    "pathfinder: couldnt find a path, shrinking radius to {}",
                    self.current_robot_radius_meters
                );

                continue;
            };
            
            if path_kind == PathKind::StopInFrontOfTarget {
                let mut i = raw_path.len();
                for cell in raw_path.iter().rev() {
                    if distance_between_tuples(end_cell, *cell) > self.current_robot_radius_cells() {
                        break;
                    }
                    i -= 1;
                }
                raw_path.truncate(i);
            }
            

            if let Err((i, unknown_cell)) = map_data.is_path_safe_for_robot(start_cell, &mut raw_path) {
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

                    raw_path.truncate(i);

                    res.extend(
                        raw_path.iter()
                            .map(|pos| PathPoint {cell: *pos, instruction: PathInstruction::MoveTo}),
                    );
                    res.push(PathPoint {cell: unknown_cell, instruction: PathInstruction::FaceTowards});
                    break;
                }
            }
            // path is safe
            else {
                self.times_blocked_here_in_a_row = 0;
                res.extend(
                    raw_path.iter()
                        .map(|pos| PathPoint {cell: *pos, instruction: PathInstruction::MoveTo}),
                );
                
                if path_kind == PathKind::StopInFrontOfTarget {
                    res.push(PathPoint {cell: end_cell, instruction: PathInstruction::FaceTowards});
                }
                
                break;
            }
        }
        
        Ok(res)
    }
}
