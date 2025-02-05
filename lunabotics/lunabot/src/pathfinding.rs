use common::{PathPoint, PathInstruction};
use nalgebra::{Point3, Transform3};
use pathfinding::{grid::Grid, prelude::astar};
use tasker::shared::SharedDataReceiver;
use tracing::error;
use tracing::error;
use crate::utils::distance_between_tuples;

use crate::pipelines::thalassic::{set_observe_depth, ThalassicData, CellState};

const REACH: usize = 10;

pub struct DefaultPathfinder {

    pub world_to_grid: Transform3<f64>,
    pub grid_to_world: Transform3<f64>,
    pub grid: Grid,
}


impl DefaultPathfinder {

    pub fn pathfind(
        &self,
        shared_thalassic_data: &SharedDataReceiver<ThalassicData>,
        from: Point3<f64>,
        to: Point3<f64>,
        into: &mut Vec<PathPoint>,
    ) {
        shared_thalassic_data.try_get();
        set_observe_depth(true);
        let mut map_data = shared_thalassic_data.get();
                
        loop {
            if map_data.current_robot_radius == 0.5 {
                break;
            }
            map_data.set_robot_radius(0.5);
            drop(map_data);
            map_data = shared_thalassic_data.get();
        }
        set_observe_depth(false);

        into.clear();


        // allows checking if position is known inside `move || {}` closures without moving `map_data`
        let is_known = |pos: (usize, usize)| {
            map_data.is_known(pos)
        };

        macro_rules! neighbours {
            ($p: ident) => {
                self.grid
                    .bfs_reachable($p, |potential_neighbor| {

                        let (x, y) = potential_neighbor;

                        // neighbors are: within reach AND not red 
                        x.abs_diff($p.0) <= REACH && y.abs_diff($p.1) <= REACH && 
                        map_data.get_cell_state(potential_neighbor) != CellState::RED
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

        let from_grid: nalgebra::OPoint<f64, nalgebra::Const<3>> = self.world_to_grid * from;
        let to_grid = self.world_to_grid * to;

        let robot_cell_pos = (from_grid.x as usize, from_grid.z as usize);

        let mut start = robot_cell_pos;
        let end = (to_grid.x as usize, to_grid.z as usize);

        let heuristic = move |p: &(usize, usize)| {
            (distance_between_tuples(*p, end) * 10000.0) as usize
        };

        let cell_pos_to_path_point = |(x, z): (usize, usize), instruction: PathInstruction| {
            let mut world_point = self.grid_to_world * Point3::new(x as f64, 0.0, z as f64);
            world_point.y = map_data.get_height((x, z)) as f64;
        
            PathPoint { point: world_point, instruction }
        };
        

        // if in red, prepend a path to safety
        {
            if map_data.get_cell_state(start) == CellState::RED {
                println!("Current cell is occupied, finding closest safe cell");
                if let Some((path, _)) = astar(
                    &start,
                    |&p| {
                        self.grid
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
                ) {
                    start = *path.last().unwrap();
                    into.extend(
                        path.iter().map(|pos| cell_pos_to_path_point(*pos, PathInstruction::MoveTo))
                    );
                } else {
                    error!("Failed to find path to safety");
                    return;
                }
            }
        }

        let path = astar(&start, |&p| neighbours!(p), heuristic, |p| p == &end)
            .expect("there should always be a possible path to the goal")
            .0;

        println!("path colors ===", );
        for pt in &path {
            println!("color at {:?}: {:?}", pt, map_data.get_cell_state(*pt));
        }

        
        
        // add points in final path to `into`, stopping upon seeing an unsafe point
        for pt in path {
            if let Err(unknown_cell) = map_data.is_safe_for_robot(robot_cell_pos, pt) {
                println!("truncated, unknown cell: {:?}", unknown_cell);
                into.push(cell_pos_to_path_point(unknown_cell, PathInstruction::FaceTowards));
                break;
            }
            else {
                into.push(cell_pos_to_path_point(pt, PathInstruction::MoveTo));
            }
        }
        
    }
}

