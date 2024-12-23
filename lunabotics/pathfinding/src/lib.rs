#![feature(map_try_insert)]

use nalgebra::{Point3, Vector2};

mod astar;
mod decimate;
pub mod obstacles;

/// Used to find an optimal path between two points on a grid. Each cell is a square with
/// the same length and height as step_size, such that these cells perfectly fit into the
/// dimensions of the map as provided. Although the
#[derive(Clone, Copy, Debug)]
pub struct Pathfinder {
    /// All points used during pathfinding are bounded to within the map dimensions.
    pub map_dimension: Vector2<f64>,
    /// The distance between points in the path. Also describes the density of points
    /// within the map dimensions.
    pub step_size: f64,
}

impl Pathfinder {
    /// Creates a new instance of Pathfinder with the given dimensions and step_size.
    pub fn new(map_dimension: Vector2<f64>, step_size: f64) -> Self {
        Self {
            map_dimension,
            step_size,
        }
    }

    /// Private helper method that calculates the shortest path between two points on the
    /// grid this pathfinder represents, using a gradient map and a threshold. The height
    /// map data is added to the path to make it 3d. The height
    /// and gradient maps must equal the total number of cells in this Pathfinder
    /// instance.
    fn pathfind(
        &mut self,
        start: Point3<f64>,
        goal: Point3<f64>,
        height_map: &[f32],
        gradient_map: &[f32],
        threshold: f32,
    ) -> Vec<Point3<f64>> {
        let length = ((self.map_dimension.x / self.step_size.abs()) + 0.0).round() as usize;
        let height = ((self.map_dimension.y / self.step_size.abs()) + 0.0).round() as usize;

        if gradient_map.len() != length * height {
            panic!(
                "Gradient map doesn't fit to this Pathfinder! Expected size {} but found {}.",
                length * height,
                gradient_map.len()
            );
        }
        if height_map.len() != length * height {
            panic!(
                "Height map doesn't fit to this Pathfinder! Expected size {} but found {}.",
                length * height,
                gradient_map.len()
            );
        }

        let start = Vector2::new(start.x, start.z);
        let goal = Vector2::new(goal.x, goal.z);

        // Find valid path from start to goal (if it exists)
        let mut path = astar::astar(
            start,
            goal,
            self.map_dimension,
            self.step_size,
            |begin, end| self.check_safe(begin, end, gradient_map, threshold),
        );

        // Optimize path where possible
        decimate::decimate(&mut path, |begin, end| {
            self.check_safe(begin, end, gradient_map, threshold)
        });

        // Add height data to path
        path.iter()
            .map(|point2| {
                Point3::new(
                    point2.x,
                    height_map[(point2.x / self.step_size).round() as usize
                        + (point2.y / self.step_size).round() as usize * length]
                        as f64,
                    point2.y,
                )
            })
            .collect()
    }

    /// Clears and appends an optimal path from start to goal using the given
    /// height map, gradient map, threshold, and point buffer (Vec). The height
    /// and gradient maps must equal the total number of cells in this Pathfinder
    /// instance.
    /// Note: for proper pathfinding the gradient map must be adjusted beforehand
    /// to "expand" the gradients to the radius of the robot. This ensures that
    /// paths that are too narrow won't be determined to be valid.
    pub fn append_path(
        &mut self,
        start: Point3<f64>,
        goal: Point3<f64>,
        height_map: &[f32],
        gradient_map: &[f32],
        threshold: f32,
        path: &mut Vec<Point3<f64>>,
    ) {
        let mut new_path = self.pathfind(start, goal, height_map, gradient_map, threshold);
        path.clear();
        path.append(&mut new_path);
    }

    /// Helper method for determining if path between two points is safe. Only returns true if
    /// the gradient at every cell that the line drawn from start to goal intersects with is
    /// less than the threshold provided. Otherwise, returns false.
    fn check_safe(
        &mut self,
        start: Vector2<f64>,
        goal: Vector2<f64>,
        gradient_map: &[f32],
        threshold: f32,
    ) -> bool {
        let mut start = start;
        let mut goal = goal;

        if start.x > goal.x {
            let temp = start;
            start = goal;
            goal = temp;
        }

        let gradient_length = ((self.map_dimension.x / self.step_size.abs()) + 1.0).round() as usize;
        let dy = goal.y - start.y;
        let dx = goal.x - start.x;

        let mut index = (start.x / self.step_size).round() as usize
            + (start.y / self.step_size).round() as usize * gradient_length;
        let mut x = start.x / self.step_size;
        let mut y = start.y / self.step_size;

        // While the overall length traversed is less than or equal to the length of
        // the line between start and goal, check this cell and traverse.
        while (start.x / self.step_size - x).abs()
            <= (goal.x / self.step_size - start.x / self.step_size).abs()
            && (start.y / self.step_size - y).abs()
                <= (goal.y / self.step_size - start.y / self.step_size).abs()
        {
            if index >= gradient_map.len() {
                return true;
            }
            if gradient_map[index] > threshold {
                return false;
            }
            (x, y, index) = self.find_next(dy, dx, x, y, gradient_length, index);
        }

        return true;
    }

    /// Private helper method
    fn find_next(
        &mut self,
        dy: f64,
        dx: f64,
        x: f64,
        y: f64,
        gradient_length: usize,
        index: usize,
    ) -> (f64, f64, usize) {
        let cost_x = ((x + 1.0).ceil() - 0.5 - x) / dx;

        // Up-Right
        if dy < 0.0 {
            let cost_y = (y - ((y - 1.0).floor() + 0.5)) / dy;
            // Prevents y pointer from moving out of bounds into overflow error.
            if y - cost_y * dy < 0.0 {
                return (x, y, usize::MAX);
            }

            if dx == 0.0 {
                return (x, y - 1.0, index - gradient_length);
            }

            return if cost_x > cost_y {
                (x, y - cost_y * dy, index - gradient_length)
            } else if cost_x < cost_y {
                (x + cost_x * dx, y, index + 1)
            } else {
                (
                    x + cost_x * dx,
                    y - cost_y * dy,
                    index - gradient_length + 1,
                )
            };
        }
        // Down-Right
        else if dy > 0.0 {
            if dx == 0.0 {
                return (x, y + 1.0, index + gradient_length);
            }

            let cost_y = ((y + 1.0).floor() - 0.5 - y) / dy;
            return if cost_x > cost_y {
                (x, y + cost_y * dy, index + gradient_length)
            } else if cost_x < cost_y {
                (x + cost_x * dx, y, index + 1)
            } else {
                (
                    x + cost_x * dx,
                    y + cost_y * dy,
                    index + gradient_length + 1,
                )
            };
        }
        // Right
        else {
            if dx == 0.0 {
                panic!("Cannot have both dy and dx as zero!")
            }
            return (x + 1.0, y, index + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_obstacles_pathfind() {
        let mut finder = Pathfinder::new(Vector2::new(1.0, 1.0), 1.0);
        let height_map = vec![1.0, 1.0, 1.0, 1.0];
        let gradient_map = vec![0.0, 0.0, 0.0, 0.0];

        let path = finder.pathfind(
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(1.0, 1.0, 1.0),
            &height_map,
            &gradient_map,
            1.0,
        );
        assert_eq!(
            path,
            [Point3::new(0.0, 0.0, 1.0), Point3::new(1.0, 1.0, 1.0)]
        );
    }

    #[test]
    fn test_obstacle_pathfind() {
        let mut finder = Pathfinder::new(Vector2::new(2.0, 2.0), 1.0);
        let height_map = vec![1.0, 1.0, 1.0, 1.0, 99.0, 1.0, 1.0, 1.0, 1.0];
        let gradient_map = vec![0.0, 0.0, 0.0, 0.0, 5.0, 0.0, 0.0, 0.0, 0.0];

        let path = finder.pathfind(
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(2.0, 2.0, 1.0),
            &height_map,
            &gradient_map,
            1.0,
        );
        assert_eq!(
            path,
            [
                Point3::new(0.0, 0.0, 1.0),
                Point3::new(2.0, 1.0, 1.0),
                Point3::new(2.0, 2.0, 1.0)
            ]
        )
    }

    #[test]
    fn test_less_than_zero_step_size() {
        let mut finder = Pathfinder::new(Vector2::new(2.0, 2.0), 0.5);
        #[rustfmt::skip]
        let height_map = vec![
            1.0,  1.0,  1.0,  1.0,  1.0,
            1.0,  1.0,  99.0, 1.0,  1.0,
            1.0,  99.0, 99.0, 99.0, 1.0,
            1.0,  1.0,  99.0, 1.0,  1.0,
            1.0,  1.0,  1.0,  1.0,  1.0,
        ];
        #[rustfmt::skip]
        let gradient_map = vec![
            0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 5.0, 0.0, 0.0,
            0.0, 5.0, 5.0, 5.0, 0.0,
            0.0, 0.0, 5.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0,
        ];

        let path = finder.pathfind(
            Point3::new(0.5, 0.25, 1.0),
            Point3::new(1.75, 1.5, 1.0),
            &height_map,
            &gradient_map,
            1.0,
        );
        assert_eq!(
            path,
            [
                Point3::new(0.5, 0.25, 1.0),
                Point3::new(1.5, 0.0, 1.0),
                Point3::new(1.5, 0.5, 1.0),
                Point3::new(2.0, 1.0, 1.0),
                Point3::new(1.75, 1.5, 1.0),
            ]
        )
    }

    #[test]
    fn test_no_path() {
        let mut finder = Pathfinder::new(Vector2::new(2.0, 2.0), 1.0);
        let height_map = vec![1.0, 1.0, 1.0, 1.0, 99.0, 1.0, 1.0, 1.0, 1.0];
        let gradient_map = vec![0.0, 0.0, 0.0, 5.0, 5.0, 5.0, 0.0, 0.0, 0.0];

        let path = finder.pathfind(
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(2.0, 2.0, 1.0),
            &height_map,
            &gradient_map,
            1.0,
        );

        assert_eq!(path, [Point3::new(0.0, 0.0, 1.0)]);
    }
}
