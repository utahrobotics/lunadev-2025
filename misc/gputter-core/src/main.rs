use gputter::types::{AlignedMatrix3, AlignedVec3};
use nalgebra::{Matrix3, Vector3};

fn main() {
    let tmp = Vector3::<f32>::identity().data.0[0];
    println!("{}", std::mem::align_of::<AlignedMatrix3<f32>>());
}