use nalgebra::Vector2;

pub trait Obstacles {
    // fn add_
    fn fast_is_safe<'a>(&mut self) -> impl FnMut(Vector2<f64>, Vector2<f64>) -> bool + 'a;
    fn precise_is_safe<'a>(&mut self) -> impl FnMut(Vector2<f64>, Vector2<f64>) -> bool + 'a;
}