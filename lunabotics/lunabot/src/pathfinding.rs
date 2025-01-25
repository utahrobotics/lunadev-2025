use nalgebra::{Point3, Transform3};
use pathfinding::{grid::Grid, prelude::astar};
use tasker::shared::SharedDataReceiver;
use tracing::{error, warn};
use crate::utils::distance_between_tuples;

use crate::pipelines::thalassic::{set_observe_depth, ThalassicData};

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
        loop {
            if map_data.current_robot_radius == 0.5 {
                break;
            }
            map_data.set_robot_radius(0.5);
            drop(map_data);
            map_data = shared_thalassic_data.get();
        }
        set_observe_depth(false);

        /// allows checking if position is known inside `move || {}` closures without moving `map_data`
        let is_known = |pos: (usize, usize)| {
            map_data.is_known(pos)
        };

        macro_rules! neighbours {
            ($p: ident) => {
                self.grid
                    .bfs_reachable($p, |potential_neighbor| {

                        let (x, y) = potential_neighbor;

                        // neighbors are: within reach AND known AND unoccupied
                        x.abs_diff($p.0) <= REACH && y.abs_diff($p.1) <= REACH &&
                        map_data.is_known(potential_neighbor) && 
                        !map_data.is_occupied(potential_neighbor)
                    })
                    .into_iter()
                    .map(move |neighbor| {

                        // unknown cells have 2x the cost
                        let unknown_multiplier = match is_known(neighbor) {
                            true => 1,
                            false => 2,
                        };

                        (
                            neighbor,
                            
                            // the cost of moving from a to b is the distance between a to b
                            (distance_between_tuples($p, neighbor) * 10000.0) as usize * unknown_multiplier
                        )
                    })
            };
        }

        let from_grid = self.world_to_grid * from;
        let to_grid = self.world_to_grid * to;
        let mut start = (from_grid.x as usize, from_grid.z as usize);
        let end = (to_grid.x as usize, to_grid.z as usize);

        let heuristic = move |p: &(usize, usize)| {
            (distance_between_tuples(*p, end) * 10000.0) as usize
        };
        into.clear();

        // if in red, prepend a path to safety
        {
            if !map_data.is_known(start) || map_data.is_occupied(start) {
                warn!("Current cell is occupied, finding closest safe cell");
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
                    |&pos| {
                        map_data.get_height(pos) != 0.0 && !map_data.is_occupied(pos)
                    },
                ) {
                    start = *path.last().unwrap();
                    into.extend(path.into_iter().map(|(x, y)| {
                        let mut p = Point3::new(x as f64, 0.0, y as f64);
                        p = self.grid_to_world * p;
                        p.y = map_data.get_height((x, y)) as f64;
                        p
                    }));
                } else {
                    error!("Failed to find path to safety");
                    return;
                }
            }
        }

        let mut closest = start;
        let mut closest_distance = self.grid.distance(closest, end);
        let mut using_closest = false;

        let mut path = astar(
            &start,
            |&p| {
                let d = self.grid.distance(p, end);
                if d < closest_distance {
                    closest = p;
                    closest_distance = d;
                }
                neighbours!(p)
            },
            heuristic,
            |p| p == &end,
        )
        .unwrap_or_else(|| {
            warn!("Failed to find path, using closest...");
            using_closest = true;
            astar(&start, |&p| neighbours!(p), heuristic, |p| p == &closest).unwrap()
        })
        .0;

        // truncate path so that it ends before entering explored region
        for (index, pt) in path.iter().enumerate() {
            
            if !map_data.is_safe_for_robot(*pt) {
                warn!("truncated path");
                path.truncate(index-1);
                break;
            }
        }

        // add final path to `into`
        into.extend(path.into_iter().map(|(x, y)| {
            let mut p = Point3::new(x as f64, 0.0, y as f64);
            p = self.grid_to_world * p;
            p.y = map_data.get_height((x, y)) as f64;
            p
        }));
        if using_closest {
            into.push(to);
        }
    }
}
