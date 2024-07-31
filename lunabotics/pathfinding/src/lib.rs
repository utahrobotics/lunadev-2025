#![feature(map_try_insert)]

use nalgebra::Vector2;

mod astar;
mod decimate;

pub fn pathfind(
    start: Vector2<f64>,
    goal: Vector2<f64>,
    step_size: f64,
    mut is_safe: impl FnMut(Vector2<f64>) -> bool,
) -> Vec<Vector2<f64>> {
    let mut path = astar::astar(start, goal, step_size, &mut is_safe);
    decimate::decimate(&mut path, step_size, is_safe);
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connected_pathfind() {
        let path = pathfind(Vector2::new(0.0, 0.0), Vector2::new(10.0, 0.0), 1.0, |_| {
            true
        });
        assert_eq!(path, vec![Vector2::new(0.0, 0.0), Vector2::new(10.0, 0.0)]);
    }

    #[test]
    fn test_disconnected_pathfind() {
        let path = pathfind(Vector2::new(0.0, 0.0), Vector2::new(2.0, 0.0), 1.0, |_| {
            false
        });
        assert_eq!(path, [Vector2::new(0.0, 0.0)]);
    }

    #[test]
    fn test_diagonal_pathfind() {
        let path = pathfind(
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 10.0),
            1.0,
            |_| true,
        );
        assert_eq!(path, [Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0)]);
    }

    #[test]
    fn test_centered_pathfind() {
        let path = pathfind(
            Vector2::new(5.0, 5.0),
            Vector2::new(1.12, 0.83),
            1.0,
            |_| true,
        );
        assert_eq!(path, [Vector2::new(5.0, 5.0), Vector2::new(1.12, 0.83)]);
    }
}
