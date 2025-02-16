use common::{PathInstruction, PathPoint, THALASSIC_HEIGHT, THALASSIC_WIDTH};
use nalgebra::{Point3, Transform3};
use pathfinding::{grid::Grid, prelude::astar};
use tasker::shared::{SharedData, SharedDataReceiver};
use tracing::error;
use crate::utils::distance_between_tuples;

use crate::pipelines::thalassic::{set_observe_depth, ThalassicData, CellState};

const REACH: usize = 10;

const MAX_SCAN_ATTEMPTS: usize = 4;


pub struct DefaultPathfinder {

    pub world_to_cells: Transform3<f64>,
    pub cells_to_world: Transform3<f64>,
    pub cell_grid: Grid,

    current_robot_radius: f32,

    /// these cells have been scanned `MAX_SCAN_ATTEMPTS` times and yet still unknown.
    /// 
    /// avoid them in the future.
    unscannable_cells: Vec<(usize, usize)>, 
    last_unknown_cell: (usize, usize),
    times_blocked_here_in_a_row: usize
}


impl DefaultPathfinder {

    pub fn new(world_to_cells: Transform3<f64>, cells_to_world: Transform3<f64>) -> Self {
        Self {
            world_to_cells, 
            cells_to_world,
            cell_grid: Grid::new(THALASSIC_WIDTH as usize, THALASSIC_HEIGHT as usize),
            
            current_robot_radius: 0.5,

            unscannable_cells: vec![],
            last_unknown_cell: (0, 0),
            times_blocked_here_in_a_row: 0
        }
    }

    fn get_map_data(&mut self, shared_thalassic_data: &SharedDataReceiver<ThalassicData>, robot_radius: f32) -> SharedData<ThalassicData> {
        shared_thalassic_data.try_get(); // clear out a previous observation if it exists
    
        set_observe_depth(true);
        let mut map_data = shared_thalassic_data.get();
        loop {
            if map_data.current_robot_radius == robot_radius {
                break;
            }
            map_data.set_robot_radius(robot_radius);
            drop(map_data);
            map_data = shared_thalassic_data.get();
        }
        set_observe_depth(false);


         // if a previously unscannable cell was scanned, its no longer unscannable
        self.unscannable_cells = self.unscannable_cells.iter()
            .filter_map(|unknown_cell| 
                match map_data.is_known(*unknown_cell) {
                    true => None,                           // known cells are removed from unscannable_cells
                    false => Some(*unknown_cell),
                }
            ).collect();
    

        map_data
    }

    fn find_path(&self, mut start_cell: (usize, usize), end_cell: (usize, usize), map_data: &SharedData<ThalassicData>) -> Option<Vec<(usize, usize)>> {
        // allows checking if position is known inside `move || {}` closures without moving `map_data`
        let is_known = |pos: (usize, usize)| {
            map_data.is_known(pos)
        };

        macro_rules! neighbours {
            ($p: ident) => {
                self.cell_grid
                    .dfs_reachable($p, |potential_neighbor| {

                        let (x, y) = potential_neighbor;

                        if x.abs_diff($p.0) > REACH || y.abs_diff($p.1) > REACH {
                            return false;
                        }
                        if map_data.get_cell_state(potential_neighbor) == CellState::RED {
                            return false;
                        }

                        let robot_radius_in_cells = map_data.current_robot_radius / common::THALASSIC_CELL_SIZE;

                        // avoid going near unscannable cells
                        for unknown_cell in &self.unscannable_cells {
                            if distance_between_tuples(potential_neighbor, *unknown_cell) < robot_radius_in_cells {
                                return false;
                            }
                        }

                        return true;
                        
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
                            (distance_between_tuples($p, neighbor) * unknown_multiplier * 10000.0) as usize
                        )
                    })
            };
        }

        let heuristic = move |p: &(usize, usize)| {
            (distance_between_tuples(*p, end_cell) * 10000.0) as usize
        };

        let mut path = vec![];

        // if in red, prepend a path to safety
        if map_data.get_cell_state(start_cell) == CellState::RED {
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
                                (distance_between_tuples(p, neighbor) * 10000.0) as usize
                            )
                        })
                },
                |_| 0,
                |&pos| map_data.get_cell_state(pos) == CellState::GREEN,
            ) else {
                error!("Failed to find path to safety");
                return None;
            };

            start_cell = *path_to_safety.last().unwrap();
            path.append(&mut path_to_safety);
        }
        
        let Some((mut path_to_goal, _)) = astar(&start_cell, |&p| neighbours!(p), heuristic, |p| p == &end_cell)
        else { return None }; 

        path.append(&mut path_to_goal);

        Some(path)
    }

    pub fn pathfind(
        &mut self,
        shared_thalassic_data: &SharedDataReceiver<ThalassicData>,
        from: Point3<f64>,
        to: Point3<f64>,
        into: &mut Vec<PathPoint>,
    ) {
        into.clear();
        

        let start_cell = self.world_to_cells * from;
        let start_cell = (start_cell.x as usize, start_cell.z as usize);

        let end_cell = self.world_to_cells * to;
        let end_cell = (end_cell.x as usize, end_cell.z as usize);
        
        
        loop {
            let map_data = self.get_map_data(shared_thalassic_data, self.current_robot_radius);
            
            let Some(mut path) = self.find_path(start_cell, end_cell, &map_data)
            else {
                if self.current_robot_radius == 0.0 {
                    panic!("pathfinder: couldnt find a path even with a robot radius of 0");
                }

                self.current_robot_radius -= 0.1;
                println!("pathfinder: couldnt find a path, shrinking radius to {}", self.current_robot_radius);

                continue;
            };
            

            let cell_to_path_pt = |(x, z): (usize, usize), instruction: PathInstruction| {
                let mut world_point = self.cells_to_world * Point3::new(x as f64, 0.0, z as f64);
                world_point.y = map_data.get_height((x, z)) as f64;
            
                PathPoint { point: world_point, instruction }
            };

            if let Err((i, unknown_cell)) = map_data.is_path_safe_for_robot(start_cell, &mut path) {
                // path got blocked due to this unknown point again
                if self.last_unknown_cell == unknown_cell { 
                    self.times_blocked_here_in_a_row += 1;
                    
                    if self.times_blocked_here_in_a_row >= MAX_SCAN_ATTEMPTS {
                        println!("pathfinder: giving up trying to scan {:?}, trying another path", unknown_cell);
                        self.unscannable_cells.push(unknown_cell);
                    }
                }
                
                // path got stuck due to this unknown point for the 1st time
                else { 
                    self.last_unknown_cell = unknown_cell;
                    self.times_blocked_here_in_a_row = 1; 

                    path.truncate(i);

                    into.extend(path.iter().map(|pos| 
                        cell_to_path_pt(*pos, PathInstruction::MoveTo))
                    );
                    into.push(cell_to_path_pt(unknown_cell, PathInstruction::FaceTowards));
                    return;
                }
            }

            // path is safe
            else { 
                self.times_blocked_here_in_a_row = 0;
                into.extend(path.iter().map(|pos| 
                    cell_to_path_pt(*pos, PathInstruction::MoveTo))
                );
                return;
            }
            
        }
        

    }
}

