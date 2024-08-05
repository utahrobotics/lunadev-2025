use std::{sync::atomic::{AtomicBool, Ordering}, time::Duration};

use crossbeam::{atomic::AtomicCell, queue::SegQueue};
use nalgebra::{Rotation2, Vector2};
use serde::{Deserialize, Serialize};
use urobotics::parking_lot::RwLock;

pub mod linear_maze;

const REFRESH_RATE: Duration = Duration::from_millis(20);
static SIMBOT_ORIGIN: AtomicCell<Vector2<f64>> = AtomicCell::new(Vector2::new(0.0, 0.0));
static SIMBOT_DIRECTION: AtomicCell<f64> = AtomicCell::new(0.0);
static OBSTACLES: RwLock<Obstacles> = RwLock::new(Obstacles { vertices: Vec::new(), edges: Vec::new() });
static COLLIDED: AtomicBool = AtomicBool::new(false);
static END_POINT: AtomicCell<Vector2<f64>> = AtomicCell::new(Vector2::new(0.0, 0.0));
static DRIVE_HISTORY: SegQueue<Vector2<f64>> = SegQueue::new();


pub struct Drive {
    origin: Vector2<f64>,
    direction: f64,
}

impl Default for Drive {
    fn default() -> Self {
        COLLIDED.store(false, Ordering::Relaxed);
        Self {
            origin: SIMBOT_ORIGIN.load(),
            direction: SIMBOT_DIRECTION.load(),
        }
    }
}

impl Drive {
    /// Gets the direction in radians.
    pub fn get_direction(&self) -> f64 {
        self.direction
    }

    /// Gets the origin in meters.
    pub fn get_origin(&self) -> Vector2<f64> {
        self.origin
    }

    /// Sets the direction in radians.
    pub fn set_direction(&mut self, direction: f64) {
        self.direction = direction;
        SIMBOT_DIRECTION.store(direction);
    }

    /// Drives in the current direction by the given distance in meters.
    pub fn drive(&mut self, mut distance: f64) {
        if COLLIDED.load(Ordering::Relaxed) {
            return;
        } else if let Some(raycast_distance) = OBSTACLES.read().raycast::<f64>(self.origin, self.direction) {
            if raycast_distance <= distance {
                COLLIDED.store(true, Ordering::Relaxed);
                distance = raycast_distance;
            }
        }
        let rot = Rotation2::new(self.direction);
        self.origin += rot * Vector2::new(distance, 0.0);
        DRIVE_HISTORY.push(self.origin);
        SIMBOT_ORIGIN.store(self.origin);
    }
}


trait RaycastMetric {
    fn from(raycast_origin: Vector2<f64>, distance: f64, rotation_matrix: Rotation2<f64>) -> Self;
}

impl RaycastMetric for Vector2<f64> {
    fn from(raycast_origin: Vector2<f64>, distance: f64, rotation_matrix: Rotation2<f64>) -> Self {
        rotation_matrix * Vector2::new(distance, 0.0) + raycast_origin
    }
}

impl RaycastMetric for f64 {
    fn from(_raycast_origin: Vector2<f64>, distance: f64, _rotation_matrix: Rotation2<f64>) -> Self {
        distance
    }
}

impl RaycastMetric for (Vector2<f64>, f64) {
    fn from(raycast_origin: Vector2<f64>, distance: f64, rotation_matrix: Rotation2<f64>) -> Self {
        (RaycastMetric::from(raycast_origin, distance, rotation_matrix), distance)
    }
}

#[derive(Default, Deserialize, Serialize)]
struct Obstacles {
    pub(crate) vertices: Vec<Vector2<f64>>,
    pub(crate) edges: Vec<(usize, usize)>,
}

impl Obstacles {
    fn raycast<T: RaycastMetric>(&self, raycast_origin: Vector2<f64>, raycast_direction: f64) -> Option<T> {
        let raycast_rot = Rotation2::new(raycast_direction);
        let inv_raycast_rot = Rotation2::new(-raycast_direction);

        self.edges
            .iter()
            .filter_map(|&(from, to)| {
                let mut from_vector = self.vertices[from] - raycast_origin;
                let mut to_vector = self.vertices[to] - raycast_origin;

                from_vector = inv_raycast_rot * from_vector;
                to_vector = inv_raycast_rot * to_vector;

                if from_vector.x > 0.0 || to_vector.x > 0.0 {
                    // Part of the edge is in front of the raycast
                    if from_vector.y.signum() == to_vector.y.signum() {
                        // The edge is either entirely above or below the raycast, or exactly on the raycast
                        if from_vector.y == to_vector.y {
                            // The edge is parallel to the raycast
                            if from_vector.y == 0.0 {
                                // The edge is on the raycast
                                Some((
                                    T::from(raycast_origin, from_vector.x.min(to_vector.x), raycast_rot),
                                    from_vector.x.min(to_vector.x)
                                ))
                            } else {
                                // The edge is not on the raycast
                                None
                            }
                        } else {
                            // The edge is not parallel to the raycast
                            None
                        }

                    } else if from_vector.x == to_vector.x {
                        // The edge is perpendicular to the raycast, but both vertices are on opposite sides of the raycast
                        Some((
                            T::from(raycast_origin, from_vector.x, raycast_rot),
                            from_vector.x
                        ))
                    } else {
                        // The edge is at an angle to the raycast, but both vertices are on opposite sides of the raycast
                        let gradient = (to_vector.y - from_vector.y) / (to_vector.x - from_vector.x);
                        let x_intercept = from_vector.x - from_vector.y / gradient;
                        if x_intercept < 0.0 {
                            None
                        } else {
                            Some((
                                T::from(raycast_origin, x_intercept, raycast_rot),
                                x_intercept
                            ))
                        }
                    }
                } else {
                    None
                }
            })
            .min_by(|(_, distance1), (_, distance2)| distance1.total_cmp(distance2))
            .map(|(point, _)| point)
    }
}
