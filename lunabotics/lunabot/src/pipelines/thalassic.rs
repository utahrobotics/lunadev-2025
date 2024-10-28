use std::{num::NonZeroU32, sync::Arc};

use gputter::{is_gputter_initialized, types::{AlignedMatrix4, AlignedVec4}};
use k::Node;
use nalgebra::{Vector2, Vector4};
use thalassic::ThalassicBuilder;
use urobotics::{
    define_callbacks, fn_alias, parking_lot::{Condvar, Mutex}
};

use super::{PointCloudCallbacks, PointCloudCallbacksRef};


fn_alias! {
    pub type HeightMapCallbacksRef = CallbacksRef(&[f32]) + Send + Sync
}
define_callbacks!(HeightMapCallbacks => Fn(heightmap: &[f32]) + Send + Sync);

pub struct DepthMapBuffer {
    depth_map: Mutex<Box<[u32]>>,
    condvar: Condvar
}

impl DepthMapBuffer {
    pub fn write(&self, f: impl FnOnce(&mut [u32])) {
        let mut depth_map = self.depth_map.lock();
        f(&mut depth_map);
        self.condvar.notify_all();
    }
}

pub fn spawn_thalassic_pipeline(
    focal_length_px: f32,
    depth_scale: f32,
    projection_size: Vector2<u32>,
    camera_link: Node<f64>
) -> (Arc<DepthMapBuffer>, PointCloudCallbacksRef, HeightMapCallbacksRef) {
    const CELL_COUNT: u32 = 64 * 128;

    let mut heightmap_callbacks = HeightMapCallbacks::default();
    let heightmap_callbacks_ref = heightmap_callbacks.get_ref();
    let mut pcl_callbacks = PointCloudCallbacks::default();
    let pcl_callbacks_ref = pcl_callbacks.get_ref();
    let depth_map_buffer = Arc::new(
        DepthMapBuffer {
            depth_map: Mutex::new(vec![0; projection_size.x as usize * projection_size.y as usize].into_boxed_slice()),
            condvar: Condvar::new(),
        }
    );
    let depth_map_buffer2 = depth_map_buffer.clone();
    let mut pcl = vec![AlignedVec4::from(Vector4::default()); projection_size.x as usize * projection_size.y as usize].into_boxed_slice();
    let mut heightmap = [0.0; CELL_COUNT as usize];
    
    if is_gputter_initialized() {
        let mut pipeline = ThalassicBuilder {
            image_width: NonZeroU32::new(projection_size.x).unwrap(),
            focal_length_px,
            principal_point_px: (projection_size - Vector2::new(1, 1)).cast() / 2.0,
            depth_scale,
            pixel_count: NonZeroU32::new(projection_size.x * projection_size.y).unwrap(),
            heightmap_width: NonZeroU32::new(64).unwrap(),
            cell_size: -0.0625,
            cell_count: NonZeroU32::new(CELL_COUNT).unwrap(),
        }.build();
        
        std::thread::spawn(move || {
            loop {
                let mut depth_buffer = depth_map_buffer.depth_map.lock();
                depth_map_buffer.condvar.wait(&mut depth_buffer);
                let Some(camera_transform) = camera_link.world_transform() else {
                    continue;
                };
                let camera_transform = camera_transform.to_homogeneous().cast::<f32>();
                pipeline.provide_depths(
                    &depth_buffer,
                    &AlignedMatrix4::from(camera_transform),
                    &mut pcl,
                    &mut heightmap
                );
                pcl_callbacks.call(&pcl);
                heightmap_callbacks.call(&heightmap);
            }
        });
    }

    (depth_map_buffer2, pcl_callbacks_ref, heightmap_callbacks_ref)
}
