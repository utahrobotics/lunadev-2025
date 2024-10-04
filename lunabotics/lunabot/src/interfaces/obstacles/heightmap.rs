use lunabot_ai::PathfinderComponent;
use urobotics::{define_callbacks, fn_alias};

fn_alias! {
    pub type HeightMapCallbacksRef = CallbacksRef(&[f32]) + Send + Sync
}
define_callbacks!(pub HeightMapCallbacks => Fn(heightmap: &[f32]) + Send + Sync);

pub struct HeightMapPathfinder {

}

impl HeightMapPathfinder {
    pub fn new() -> Self {
        Self {}
    }
}

impl PathfinderComponent for HeightMapPathfinder {
    fn pathfind(&mut self, _from: nalgebra::Vector2<f64>, _to: nalgebra::Vector2<f64>) -> &[nalgebra::Vector2<f64>] {
        &[]
    }
}