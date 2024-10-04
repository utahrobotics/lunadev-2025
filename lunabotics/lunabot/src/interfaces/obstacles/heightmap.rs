use std::sync::Arc;

use byteable::IntoBytesSlice;
use common::lunasim::FromLunasimbot;
use crossbeam::atomic::AtomicCell;
use heightmap::HeightMapper;
use lunabot_ai::PathfinderComponent;
use nalgebra::Vector2;
use urobotics::{
    define_callbacks, fn_alias,
    tokio::{self, task::block_in_place},
    BlockOn,
};

use crate::{sim::LunasimStdin, PointCloudCallbacksRef};

fn_alias! {
    pub type HeightMapCallbacksRef = CallbacksRef(&[f32]) + Send + Sync
}
define_callbacks!(HeightMapCallbacks => Fn(heightmap: &[f32]) + Send + Sync);

pub struct HeightMapPathfinder {
    heightmap: HeightMapCallbacksRef,
}

impl HeightMapPathfinder {
    pub fn new(heightmap: HeightMapCallbacksRef) -> Self {
        Self { heightmap }
    }
}

impl PathfinderComponent for HeightMapPathfinder {
    fn pathfind(
        &mut self,
        _from: nalgebra::Vector2<f64>,
        _to: nalgebra::Vector2<f64>,
    ) -> &[nalgebra::Vector2<f64>] {
        &[]
    }
}

pub fn heightmap_strategy(
    projection_size: Vector2<u32>,
    raw_pcl_callbacks_ref: &PointCloudCallbacksRef,
    lunasim_stdin: LunasimStdin,
) -> HeightMapCallbacksRef {
    let heightmapper = HeightMapper::new(Vector2::new(64, 128), -0.0625, projection_size)
        .block_on()
        .unwrap();

    let heightmap_callbacks = HeightMapCallbacks::default();
    let heightmap_callbacks_ref = heightmap_callbacks.get_ref();
    let heightmapper_cell = Arc::new(AtomicCell::new(Some((
        heightmapper,
        Vec::new(),
        heightmap_callbacks,
    ))));

    raw_pcl_callbacks_ref.add_dyn_fn(Box::new(move |point_cloud| {
        if let Some((mut heightmapper, mut point_cloud_buffer, mut heightmap_callbacks)) =
            heightmapper_cell.take()
        {
            point_cloud_buffer.clear();
            point_cloud_buffer.extend_from_slice(point_cloud);
            let heightmapper_cell = heightmapper_cell.clone();

            tokio::spawn(async move {
                heightmapper.call(&*point_cloud_buffer).await;
                {
                    let heightmap = heightmapper.read_heightmap().await;
                    block_in_place(|| {
                        heightmap_callbacks.call(&heightmap);
                    });
                }
                heightmapper_cell.store(Some((
                    heightmapper,
                    point_cloud_buffer,
                    heightmap_callbacks,
                )));
            });
        }
    }));

    heightmap_callbacks_ref.add_dyn_fn(Box::new(move |heightmap| {
        FromLunasimbot::HeightMap(heightmap.to_vec().into_boxed_slice()).into_bytes_slice(
            |bytes| {
                lunasim_stdin.write(bytes);
            },
        );
    }));

    heightmap_callbacks_ref
}
