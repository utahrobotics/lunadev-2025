#![feature(map_try_insert)]

use nalgebra::Vector2;

mod astar;
mod decimate;

#[derive(Clone, Copy, Debug)]
pub struct Pathfinder<F = ()> {
    /// All points used during pathfinding are bounded to within the map dimensions, after being offset.
    pub map_dimension: Vector2<f64>,
    /// The offset is subtracted from all points used during pathfinding before being bounded by the map dimensions.
    ///
    /// By default, this is `(0.0, 0.0)`.
    pub offset: Vector2<f64>,
    /// The distance between points in the path.
    pub step_size: f64,
    /// A closure that returns whether a point is safe to traverse.
    ///
    /// If this is `()`, a function must be provided when calling `pathfind`.
    pub is_safe: F,
}

impl<F: FnMut(Vector2<f64>) -> bool> Pathfinder<F> {
    pub fn new(map_dimension: Vector2<f64>, step_size: f64, is_safe: F) -> Self {
        Self {
            map_dimension,
            offset: Vector2::new(0.0, 0.0),
            step_size,
            is_safe,
        }
    }

    pub fn pathfind(&mut self, start: Vector2<f64>, goal: Vector2<f64>) -> Vec<Vector2<f64>> {
        let mut path = astar::astar(
            start,
            goal,
            self.map_dimension,
            self.offset,
            self.step_size,
            &mut self.is_safe,
        );
        decimate::decimate(&mut path, self.step_size, &mut self.is_safe);
        path
    }
}

impl Pathfinder<()> {
    pub fn new(map_dimension: Vector2<f64>, step_size: f64) -> Self {
        Self {
            map_dimension,
            offset: Vector2::new(0.0, 0.0),
            step_size,
            is_safe: (),
        }
    }

    pub fn pathfind(
        &mut self,
        start: Vector2<f64>,
        goal: Vector2<f64>,
        mut is_safe: impl FnMut(Vector2<f64>) -> bool,
    ) -> Vec<Vector2<f64>> {
        let mut path = astar::astar(
            start,
            goal,
            self.map_dimension,
            self.offset,
            self.step_size,
            &mut is_safe,
        );
        decimate::decimate(&mut path, self.step_size, &mut is_safe);
        path
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_connected_pathfind() {
//         let path = pathfind(Vector2::new(0.0, 0.0), Vector2::new(10.0, 0.0), 1.0, |_| {
//             true
//         });
//         assert_eq!(path, vec![Vector2::new(0.0, 0.0), Vector2::new(10.0, 0.0)]);
//     }

//     #[test]
//     fn test_disconnected_pathfind() {
//         let path = pathfind(Vector2::new(0.0, 0.0), Vector2::new(2.0, 0.0), 1.0, |_| {
//             false
//         });
//         assert_eq!(path, [Vector2::new(0.0, 0.0)]);
//     }

//     #[test]
//     fn test_diagonal_pathfind() {
//         let path = pathfind(
//             Vector2::new(0.0, 0.0),
//             Vector2::new(10.0, 10.0),
//             1.0,
//             |_| true,
//         );
//         assert_eq!(path, [Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0)]);
//     }

//     #[test]
//     fn test_centered_pathfind() {
//         let path = pathfind(
//             Vector2::new(5.0, 5.0),
//             Vector2::new(1.12, 0.83),
//             1.0,
//             |_| true,
//         );
//         assert_eq!(path, [Vector2::new(5.0, 5.0), Vector2::new(1.12, 0.83)]);
//     }

//     #[test]
//     fn test_1_obstacle_pathfind() {
//         let path = pathfind(Vector2::new(0.0, 0.0), Vector2::new(5.0, 0.0), 1.0, |p| {
//             p.x != 2.0 || p.y != 0.0
//         });
//         assert_eq!(
//             path,
//             vec![
//                 Vector2::new(0.0, 0.0),
//                 Vector2::new(4.0, 1.0),
//                 Vector2::new(5.0, 0.0)
//             ]
//         );
//     }
// }
