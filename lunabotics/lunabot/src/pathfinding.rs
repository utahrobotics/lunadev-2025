use nalgebra::{Point3, Transform3, Vector2};
use pathfinding::{grid::Grid, prelude::astar};
use tasker::shared::SharedDataReceiver;
use tracing::{error, warn};

use crate::pipelines::thalassic::ThalassicData;

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
        mut from: Point3<f64>,
        mut to: Point3<f64>,
        into: &mut Vec<Point3<f64>>,
    ) {
        let mut data = shared_thalassic_data.get();
        loop {
            if data.current_robot_radius == 0.5 {
                break;
            }
            data.set_robot_radius(0.5);
            drop(data);
            data = shared_thalassic_data.get();
        }

        macro_rules! neighbours {
            ($p: ident) => {
                self.grid
                    .bfs_reachable($p, |(x, y)| {
                        if x.abs_diff($p.0) <= REACH && y.abs_diff($p.1) <= REACH {
                            let index = y * 128 + x;
                            data.heightmap[y * 128 + x] != 0.0
                                && !data.expanded_obstacle_map[index].occupied()
                        } else {
                            false
                        }
                    })
                    .into_iter()
                    .map(move |(x, y)| {
                        (
                            (x, y),
                            (Vector2::new(x.abs_diff($p.0) as f32, y.abs_diff($p.1) as f32)
                                .magnitude()
                                * 10000.0) as usize,
                        )
                    })
            };
        }

        let from_grid = self.world_to_grid * from;
        let to_grid = self.world_to_grid * to;
        let mut start = (from_grid.x as usize, from_grid.z as usize);
        let end = (to_grid.x as usize, to_grid.z as usize);
        let heuristic = move |p: &(usize, usize)| {
            (Vector2::new(p.0.abs_diff(end.0) as f32, p.1.abs_diff(end.1) as f32).magnitude()
                * 10000.0) as usize
        };
        into.clear();

        {
            let index = start.1 * 128 + start.0;
            if data.heightmap[index] == 0.0 || data.expanded_obstacle_map[index].occupied() {
                warn!("Current cell is occupied, finding closest safe cell");
                if let Some((path, _)) = astar(
                    &start,
                    |&p| {
                        self.grid
                            .bfs_reachable(p, |(x, y)| {
                                x.abs_diff(p.0) <= REACH && y.abs_diff(p.1) <= REACH
                            })
                            .into_iter()
                            .map(move |(x, y)| {
                                (
                                    (x, y),
                                    (Vector2::new(x.abs_diff(p.0) as f32, y.abs_diff(p.1) as f32)
                                        .magnitude()
                                        * 10000.0) as usize,
                                )
                            })
                    },
                    |_| 0,
                    |&(x, y)| {
                        let index = y * 128 + x;
                        data.heightmap[index] != 0.0
                            && !data.expanded_obstacle_map[index].occupied()
                    },
                ) {
                    start = *path.last().unwrap();
                    into.extend(path.into_iter().map(|(x, y)| {
                        let mut p = Point3::new(x as f64, 0.0, y as f64);
                        p = self.grid_to_world * p;
                        p.y = data.heightmap[y * 128 + x] as f64;
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

        let path = astar(
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
        into.extend(path.into_iter().map(|(x, y)| {
            let mut p = Point3::new(x as f64, 0.0, y as f64);
            p = self.grid_to_world * p;
            p.y = data.heightmap[y * 128 + x] as f64;
            p
        }));
        if using_closest {
            into.push(to);
        }
    }
}
