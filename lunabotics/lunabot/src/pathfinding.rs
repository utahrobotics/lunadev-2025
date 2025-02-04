use nalgebra::{Point3, Transform3};
use pathfinding::{grid::Grid, prelude::astar};
use tasker::shared::SharedDataReceiver;
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
        into: &mut Vec<Point3<f64>>,
    ) {
        shared_thalassic_data.try_get();
        set_observe_depth(true);
        let mut map_data = shared_thalassic_data.get();
        
        let RADIUS = 0.5; //TODO: original value was 0.5
        
        loop {
            if map_data.current_robot_radius == RADIUS {
                break;
            }
            map_data.set_robot_radius(RADIUS);
            drop(map_data);
            map_data = shared_thalassic_data.get();
        }
        set_observe_depth(false);

        into.clear();

        let mut append_path = |path: Vec<(usize, usize)>| {
            into.extend(path.into_iter().map(|(x, y)| {
                let mut p = Point3::new(x as f64, 0.0, y as f64);
                p = self.grid_to_world * p;
                p.y = map_data.get_height((x, y)) as f64;
                p
            }));
        };


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

        let from_grid = self.world_to_grid * from;
        let to_grid = self.world_to_grid * to;

        let robot_cell_pos = (from_grid.x as usize, from_grid.z as usize);

        let mut start = robot_cell_pos;
        let end = (to_grid.x as usize, to_grid.z as usize);

        
        let heuristic = move |p: &(usize, usize)| {
            (distance_between_tuples(*p, end) * 10000.0) as usize
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
                    append_path(path);
                } else {
                    error!("Failed to find path to safety");
                    return;
                }
            }
        }

        let mut path = astar(&start, |&p| neighbours!(p), heuristic, |p| p == &end)
            .expect("there should always be a possible path to the goal")
            .0;

        println!("path colors ===", );
        for pt in &path {
            println!("color at {:?}: {:?}", pt, map_data.get_cell_state(*pt));
        }

        // truncate path so that it ends before entering explored region
        // for (index, pt) in path.iter().enumerate() {
            
        //     if let Err(unknown_cell) = map_data.is_safe_for_robot(robot_cell_pos, *pt) {
        //         println!("truncate: keeping only up to before {}, unknown cell: {:?}", index, unknown_cell);
        //         path.truncate(index);
        //         break;
        //     }
        // }

        // add final path to `into`
        append_path(path);
        
    }
}