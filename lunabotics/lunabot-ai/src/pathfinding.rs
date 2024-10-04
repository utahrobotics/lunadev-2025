use nalgebra::Vector2;

pub trait PathfinderComponent {
    fn pathfind(&mut self, from: Vector2<f64>, to: Vector2<f64>) -> &[Vector2<f64>];
}
