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

mod clustering;
pub mod depth2pcl;
pub mod pcl2height;
pub use clustering::Clusterer;

/// 1. Depths
/// 2. Transform
/// 3. Depth Scale
type DepthBindGrp = (
    StorageBuffer<[u32], HostWriteOnly, ShaderReadOnly>,
    UniformBuffer<AlignedMatrix4<f32>>,
    UniformBuffer<f32>,
);

/// Points used by depth2pcl and pcl2height
type PointsBindGrp = (
    StorageBuffer<[AlignedVec4<f32>], HostReadOnly, ShaderReadWrite>,
    // Projection Width
    UniformBuffer<u32>,
);

/// Heightmap used by pcl2height and height2grad
type HeightMapBindGrp = (StorageBuffer<[u32], HostReadOnly, ShaderReadWrite>,);

/// Original heightmap used by pcl2height
type PclBindGrp = (StorageBuffer<[f32], HostWriteOnly, ShaderReadOnly>,);

type AlphaBindGroups = (GpuBufferSet<DepthBindGrp>, GpuBufferSet<PointsBindGrp>);

type BetaBindGroups = (
    GpuBufferSet<PointsBindGrp>,
    GpuBufferSet<HeightMapBindGrp>,
    GpuBufferSet<PclBindGrp>,
);

#[derive(Debug, Clone, Copy)]
pub struct DepthProjectorBuilder {
    pub image_size: Vector2<NonZeroU32>,
    pub focal_length_px: f32,
    pub principal_point_px: Vector2<f32>,
}

impl DepthProjectorBuilder {
    pub fn build(self) -> DepthProjector {
        let pixel_count = self.image_size.x.get() * self.image_size.y.get();
        let [depth_fn] = Depth2Pcl {
            depths: BufferGroupBinding::<_, AlphaBindGroups>::get::<0, 0>(),
            points: BufferGroupBinding::<_, AlphaBindGroups>::get::<1, 0>(),
            transform: BufferGroupBinding::<_, AlphaBindGroups>::get::<0, 1>(),
            depth_scale: BufferGroupBinding::<_, AlphaBindGroups>::get::<0, 2>(),
            image_width: self.image_size.x,
            focal_length_px: self.focal_length_px,
            principal_point_px: self.principal_point_px.into(),
            pixel_count: NonZeroU32::new(pixel_count).unwrap(),
        }
        .compile();

        let mut pipeline = ComputePipeline::new([&depth_fn]);
        pipeline.workgroups = [Vector3::new(
            self.image_size.x.get(),
            self.image_size.y.get(),
            1,
        )];
        DepthProjector {
            image_size: self.image_size,
            pipeline,
            bind_grp: Some(GpuBufferSet::from((
                StorageBuffer::new_dyn(pixel_count as usize).unwrap(),
                UniformBuffer::new(),
                UniformBuffer::new(),
            ))),
        }
    }

    pub fn make_points_storage(self) -> PointCloudStorage {
        PointCloudStorage {
            points_grp: GpuBufferSet::from((
                StorageBuffer::new_dyn(
                    self.image_size.x.get() as usize * self.image_size.y.get() as usize,
                )
                .unwrap(),
                UniformBuffer::new(),
            )),
            image_size: self.image_size,
        }
    }
}

pub struct PointCloudStorage {
    points_grp: GpuBufferSet<PointsBindGrp>,
    image_size: Vector2<NonZeroU32>,
}

impl PointCloudStorage {
    pub fn get_image_size(&self) -> Vector2<NonZeroU32> {
        self.image_size
    }

    pub fn read(&self, points: &mut [AlignedVec4<f32>]) {
        self.points_grp.buffers.0.read(points);
    }
}

pub struct DepthProjector {
    image_size: Vector2<NonZeroU32>,
    pipeline: ComputePipeline<AlphaBindGroups, 1>,
    bind_grp: Option<GpuBufferSet<DepthBindGrp>>,
}

impl DepthProjector {
    pub fn project(
        &mut self,
        depths: &[u16],
        camera_transform: &AlignedMatrix4<f32>,
        mut points_storage: PointCloudStorage,
        depth_scale: f32
    ) -> PointCloudStorage {
        let depths = bytemuck::cast_slice(depths);
        todo!("Fix the shader code to bitmask u32");
        debug_assert_eq!(self.image_size, points_storage.image_size);
        let depth_grp = self.bind_grp.take().unwrap();

        let mut bind_grps = (depth_grp, points_storage.points_grp);

        self.pipeline
            .new_pass(|mut lock| {
                bind_grps.0.write::<0, _>(depths, &mut lock);
                bind_grps.0.write::<1, _>(camera_transform, &mut lock);
                bind_grps.0.write::<2, _>(&depth_scale, &mut lock);
                bind_grps
                    .1
                    .write::<1, _>(&self.image_size.x.get(), &mut lock);
                &mut bind_grps
            })
            .finish();
        let (depth_grp, points_grp) = bind_grps;
        self.bind_grp = Some(depth_grp);
        points_storage.points_grp = points_grp;
        points_storage
    }

    pub fn get_image_size(&self) -> Vector2<NonZeroU32> {
        self.image_size
    }

    pub fn get_pixel_count(&self) -> NonZeroU32 {
        NonZeroU32::new(self.image_size.x.get() * self.image_size.y.get()).unwrap()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ThalassicBuilder {
    pub max_point_count: NonZeroU32,
    pub heightmap_width: NonZeroU32,
    pub cell_size: f32,
    pub cell_count: NonZeroU32,
}

impl ThalassicBuilder {
    pub fn build(self) -> ThalassicPipeline {
        let bind_grps = (
            GpuBufferSet::from((StorageBuffer::new_dyn(self.cell_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((StorageBuffer::new_dyn(self.cell_count.get() as usize).unwrap(),)),
        );

        let [height_fn] = Pcl2Height {
            points: BufferGroupBinding::<_, BetaBindGroups>::get::<0, 0>(),
            heightmap: BufferGroupBinding::<_, BetaBindGroups>::get::<1, 0>(),
            cell_size: self.cell_size,
            heightmap_width: self.heightmap_width,
            cell_count: self.cell_count,
            original_heightmap: BufferGroupBinding::<_, BetaBindGroups>::get::<2, 0>(),
            projection_width: BufferGroupBinding::<_, BetaBindGroups>::get::<0, 1>(),
            max_point_count: self.max_point_count,
        }
        .compile();

        let mut pipeline = ComputePipeline::new([&height_fn]);
        pipeline.workgroups = [Vector3::new(
            self.heightmap_width.get(),
            self.cell_count.get() / self.heightmap_width,
            0,
        )];
        ThalassicPipeline {
            pipeline,
            bind_grps: Some(bind_grps),
        }
    }
}

pub struct ThalassicPipeline {
    pipeline: ComputePipeline<BetaBindGroups, 1>,
    bind_grps: Option<(GpuBufferSet<HeightMapBindGrp>, GpuBufferSet<PclBindGrp>)>,
}

impl ThalassicPipeline {
    pub fn provide_points(
        &mut self,
        mut points_storage: PointCloudStorage,
        out_heightmap: &mut [f32],
    ) -> PointCloudStorage {
        let (height_grp, pcl_grp) = self.bind_grps.take().unwrap();

        let mut bind_grps = (points_storage.points_grp, height_grp, pcl_grp);

        let image_width = points_storage.image_size.x.get();
        let image_height = points_storage.image_size.y.get();
        self.pipeline.workgroups[0].z = 2 * (image_width - 1) * (image_height - 1);
        self.pipeline
            .new_pass(|mut lock| {
                bind_grps
                    .1
                    .buffers
                    .0
                    .copy_into_unchecked(&mut bind_grps.2.buffers.0, &mut lock);
                &mut bind_grps
            })
            .finish();
        bind_grps.1.buffers.0.read(cast_slice_mut(out_heightmap));

        let (points_grp, height_grp, pcl_grp) = bind_grps;
        self.bind_grps = Some((height_grp, pcl_grp));
        points_storage.points_grp = points_grp;
        points_storage
    }
}
