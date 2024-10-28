use gputter::types::AlignedVec4;
use urobotics::{define_callbacks, fn_alias};

pub mod thalassic;

fn_alias! {
    pub type PointCloudCallbacksRef = CallbacksRef(&[AlignedVec4<f32>]) + Send + Sync
}
define_callbacks!(pub PointCloudCallbacks => Fn(point_cloud: &[AlignedVec4<f32>]) + Send + Sync);
