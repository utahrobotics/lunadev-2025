use std::{num::NonZeroU32, sync::Arc};

use crossbeam::atomic::AtomicCell;
use gputter::is_gputter_initialized;
use nalgebra::Vector2;
use thalassic::{PointCloudStorage, ThalassicBuilder};
use urobotics::{define_callbacks, fn_alias};

fn_alias! {
    pub type HeightMapCallbacksRef = CallbacksRef(&[f32]) + Send + Sync
}
define_callbacks!(HeightMapCallbacks => Fn(heightmap: &[f32]) + Send + Sync);

pub struct PointsStorageChannel {
    projected: AtomicCell<Option<PointCloudStorage>>,
    finished: AtomicCell<Option<PointCloudStorage>>,
    image_size: Vector2<NonZeroU32>,
}

impl PointsStorageChannel {
    pub fn new_for(storage: &PointCloudStorage) -> Self {
        Self {
            projected: AtomicCell::new(None),
            image_size: storage.get_image_size(),
            finished: AtomicCell::new(None),
        }
    }

    pub fn set_projected(&self, projected: PointCloudStorage) {
        self.projected.store(Some(projected));
    }

    pub fn get_finished(&self) -> Option<PointCloudStorage> {
        self.finished.take()
    }
}

pub fn spawn_thalassic_pipeline(
    mut point_cloud_channels: Box<[Arc<PointsStorageChannel>]>,
) -> (HeightMapCallbacksRef,) {
    const CELL_COUNT: u32 = 64 * 128;

    let mut heightmap_callbacks = HeightMapCallbacks::default();
    let heightmap_callbacks_ref = heightmap_callbacks.get_ref();
    let Some(max_point_count) = point_cloud_channels
        .iter()
        .map(|channel| channel.image_size.x.get() * channel.image_size.y.get())
        .max()
    else {
        return (heightmap_callbacks_ref,);
    };

    if is_gputter_initialized() {
        let mut pipeline = ThalassicBuilder {
            heightmap_width: NonZeroU32::new(64).unwrap(),
            max_point_count: NonZeroU32::new(max_point_count).unwrap(),
            cell_size: -0.0625,
            cell_count: NonZeroU32::new(CELL_COUNT).unwrap(),
        }
        .build();

        std::thread::spawn(move || {
            let mut heightmap = [0.0; CELL_COUNT as usize];
            loop {
                for channel in &mut point_cloud_channels {
                    let Some(mut points) = channel.projected.take() else {
                        continue;
                    };
                    points = pipeline.provide_points(points, &mut heightmap);
                    channel.finished.store(Some(points));
                    heightmap_callbacks.call(&heightmap);
                }
            }
        });
    }

    (heightmap_callbacks_ref,)
}
