use std::{num::NonZeroU32, sync::Arc};

use crossbeam::atomic::AtomicCell;
use gputter::is_gputter_initialized;
use nalgebra::Vector2;
use thalassic::{PointCloudStorage, ThalassicBuilder};
use urobotics::shared::OwnedData;

const CELL_COUNT: u32 = 128 * 256;

pub struct ThalassicData {
    pub heightmap: [f32; CELL_COUNT as usize],
    pub gradmap: [f32; CELL_COUNT as usize],
}

impl Default for ThalassicData {
    fn default() -> Self {
        Self {
            heightmap: [0.0; CELL_COUNT as usize],
            gradmap: [0.0; CELL_COUNT as usize],
        }
    }
}

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
    buffer: OwnedData<ThalassicData>,
    mut point_cloud_channels: Box<[Arc<PointsStorageChannel>]>,
) {
    let mut buffer = buffer.pessimistic_share();
    let Some(max_point_count) = point_cloud_channels
        .iter()
        .map(|channel| channel.image_size.x.get() * channel.image_size.y.get())
        .max()
    else {
        return;
    };

    if is_gputter_initialized() {
        let mut pipeline = ThalassicBuilder {
            heightmap_dimensions: Vector2::new(NonZeroU32::new(128).unwrap(), NonZeroU32::new(256).unwrap()),
            cell_size: 0.03125,
            max_point_count: NonZeroU32::new(max_point_count).unwrap(),
        }
        .build();

        std::thread::spawn(move || loop {
            let mut points_vec = vec![];

            for channel in &mut point_cloud_channels {
                let Some(points) = channel.projected.take() else {
                    continue;
                };
                points_vec.push((channel, points));
            }

            if !points_vec.is_empty() {
                let mut owned = buffer.recall_or_replace_with(Default::default);
                let ThalassicData { heightmap, gradmap } = &mut *owned;

                for (channel, mut points) in points_vec {
                    points = pipeline.provide_points(points, heightmap, gradmap);
                    channel.finished.store(Some(points));
                }

                buffer = owned.pessimistic_share();
            }
        });
    }
}
