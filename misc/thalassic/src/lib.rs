use std::num::NonZeroU32;

use bytemuck::cast_slice_mut;
use depth2pcl::Depth2Pcl;
use gputter::{
    buffers::{
        storage::{HostReadOnly, HostWriteOnly, ShaderReadOnly, ShaderReadWrite, StorageBuffer},
        uniform::UniformBuffer,
        GpuBufferSet,
    },
    compute::ComputePipeline,
    shader::BufferGroupBinding,
    types::{AlignedMatrix4, AlignedVec4},
};
use nalgebra::{Vector2, Vector3};
use pcl2height::Pcl2Height;

pub mod depth2pcl;
pub mod pcl2height;

/// 1. Depths
/// 2. Transform
type DepthBindGrp = (
    StorageBuffer<[u32], HostWriteOnly, ShaderReadOnly>,
    UniformBuffer<AlignedMatrix4<f32>>,
);

/// Points used by depth2pcl and pcl2height
type PointsBindGrp = (StorageBuffer<[AlignedVec4<f32>], HostReadOnly, ShaderReadWrite>,);

/// Heightmap used by pcl2height and height2grad
type HeightMapBindGrp = (StorageBuffer<[u32], HostReadOnly, ShaderReadWrite>,);

/// Original heightmap used by pcl2height
type PclBindGrp = (StorageBuffer<[f32], HostWriteOnly, ShaderReadOnly>,);

type BindGroups = (
    GpuBufferSet<DepthBindGrp>,
    GpuBufferSet<PointsBindGrp>,
    GpuBufferSet<HeightMapBindGrp>,
    GpuBufferSet<PclBindGrp>,
);

#[derive(Debug, Clone, Copy)]
pub struct ThalassicBuilder {
    pub image_width: NonZeroU32,
    pub focal_length_px: f32,
    pub principal_point_px: Vector2<f32>,
    pub depth_scale: f32,
    pub pixel_count: NonZeroU32,
    pub heightmap_width: NonZeroU32,
    pub cell_size: f32,
    pub cell_count: NonZeroU32,
}

impl ThalassicBuilder {
    pub fn build(self) -> ThalassicPipeline {
        let bind_grps = (
            GpuBufferSet::from((
                StorageBuffer::new_dyn(self.pixel_count.get() as usize).unwrap(),
                UniformBuffer::new(),
            )),
            GpuBufferSet::from((StorageBuffer::new_dyn(self.pixel_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((StorageBuffer::new_dyn(self.cell_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((StorageBuffer::new_dyn(self.cell_count.get() as usize).unwrap(),)),
        );

        let [depth_fn] = Depth2Pcl {
            depths: BufferGroupBinding::<_, BindGroups>::get::<0, 0>(),
            points: BufferGroupBinding::<_, BindGroups>::get::<1, 0>(),
            transform: BufferGroupBinding::<_, BindGroups>::get::<0, 1>(),
            image_width: self.image_width,
            focal_length_px: self.focal_length_px,
            principal_point_px: self.principal_point_px.into(),
            depth_scale: self.depth_scale,
            pixel_count: self.pixel_count,
        }
        .compile();

        let [height_fn] = Pcl2Height {
            points: BufferGroupBinding::<_, BindGroups>::get::<1, 0>(),
            heightmap: BufferGroupBinding::<_, BindGroups>::get::<2, 0>(),
            cell_size: self.cell_size,
            heightmap_width: self.heightmap_width,
            cell_count: self.cell_count,
            original_heightmap: BufferGroupBinding::<_, BindGroups>::get::<3, 0>(),
            projection_width: self.image_width,
            point_count: self.pixel_count,
        }
        .compile();
        
        let mut pipeline = ComputePipeline::new([&depth_fn, &height_fn]);
        pipeline.workgroups = [
            Vector3::new(
                self.image_width.get(),
                self.pixel_count.get() / self.image_width,
                1,
            ),
            Vector3::new(
                self.heightmap_width.get(),
                self.cell_count.get() / self.heightmap_width,
                2 * (self.image_width.get() - 1) * (self.pixel_count.get() / self.image_width - 1)
            )
        ];
        ThalassicPipeline {
            pipeline,
            bind_grps,
        }
    }
}

pub struct ThalassicPipeline {
    pipeline: ComputePipeline<BindGroups, 2>,
    bind_grps: BindGroups,
}

impl ThalassicPipeline {
    pub fn provide_depths(
        &mut self,
        depths: &[u32],
        camera_transform: &AlignedMatrix4<f32>,
        out_pcl: &mut [AlignedVec4<f32>],
        out_heightmap: &mut [f32],
    ) {
        self.pipeline
            .new_pass(|mut lock| {
                self.bind_grps.0.write::<0, _>(depths, &mut lock);
                self.bind_grps.0.write::<1, _>(camera_transform, &mut lock);
                self.bind_grps
                    .2
                    .buffers
                    .0
                    .copy_into(&mut self.bind_grps.3.buffers.0, &mut lock);
                &mut self.bind_grps
            })
            .finish();
        self.bind_grps.1.buffers.0.read(out_pcl);
        self.bind_grps
            .2
            .buffers
            .0
            .read(cast_slice_mut(out_heightmap));
    }
}
