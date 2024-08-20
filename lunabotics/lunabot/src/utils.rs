use std::ops::{Add, Mul, Sub};

pub fn lerp_value(delta: f64, speed: f64) -> f64 {
    0.5f64.powf(speed * delta)
}

#[allow(dead_code)]
pub fn lerp<T>(from: T, to: T, delta: f64, speed: f64) -> T
where 
    T: Sub<Output=T> + Add<Output=T> + Mul<f64, Output=T> + Copy
{
    let diff = to - from;
    from + diff * lerp_value(delta, speed)
}