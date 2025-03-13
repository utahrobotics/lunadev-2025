use std::{num::NonZeroU32, sync::Arc};

use bytemuck::{Pod, Zeroable};
use depth2pcl::Depth2Pcl;
use gputter::{
    buffers::{
        storage::{
            HostHidden, HostReadOnly, HostReadWrite, HostWriteOnly, ShaderReadOnly, ShaderReadWrite, StorageBuffer
        },
        uniform::UniformBuffer,
        GpuBufferSet,
    },
    compute::ComputePipeline,
    shader::BufferGroupBinding,
    types::{AlignedMatrix4, AlignedVec2, AlignedVec4},
};
use grad2obstacle::Grad2Obstacle;
use height2grad::Height2Grad;
use nalgebra::{Vector2, Vector3};

// mod clustering;
mod depth2pcl;
mod grad2obstacle;
mod height2grad;
// mod pcl2height;
mod pcl2sum;
mod sum2height;
mod obstaclefilter;
// pub use clustering::Clusterer;

mod expand_obstacles;
use expand_obstacles::ExpandObstacles;
use obstaclefilter::ObstacleFilter;
use parking_lot::Mutex;
use pcl2sum::Pcl2Sum;
use sum2height::Sum2Height;

/// 1. Depths in arbitrary units
/// 2. Global Transform of the camera
/// 3. Depth Scale (meters per depth unit)
/// 4. Points in global space
///
/// This bind group serves as the input for all DepthProjectors
type DepthBindGrp = (
    StorageBuffer<[u32], HostWriteOnly, ShaderReadOnly>,
    UniformBuffer<AlignedMatrix4<f32>>,
    UniformBuffer<f32>,
    StorageBuffer<[AlignedVec4<f32>], HostReadOnly, ShaderReadWrite>,
    UniformBuffer<u32>,
);

/// 1. Sum vectors for each cell in the heightmap
type SumBindGrp = (StorageBuffer<[AlignedVec2<f32>], HostHidden, ShaderReadWrite>,);

/// The set of bind groups used by the DepthProjector
type AlphaBindGroups = (GpuBufferSet<DepthBindGrp>, GpuBufferSet<SumBindGrp>);

/// 1. The height of each cell in the heightmap.
/// The actual type is `f32`, but it is stored as `u32` in the shader to allow for atomic operations, with conversion being a bitwise cast.
/// The units are meters.
///
/// This bind group is the output of the heightmapper ([`pcl2height`]) and the input for the gradientmapper.
type HeightMapBindGrp = (StorageBuffer<[f32], HostReadWrite, ShaderReadWrite>,);

/// 1. The gradient of each cell in the heightmap expressed as an angle in radians.
///
/// This bind group is the output of the gradientmapper ([`height2grad`]) and the input for the obstaclemapper.
type GradMapBindGrp = (StorageBuffer<[f32], HostReadOnly, ShaderReadWrite>,);

/// 1. Whether or not each cell is an obstacle, denoted with `1` or `0`.
///
/// This bind group is the output of the obstaclemapper and input to the obstacle filter.
type ObstacleMapBindGrp = (StorageBuffer<[u32], HostReadOnly, ShaderReadWrite>,);

/// 1. Whether or not each cell is an obstacle, denoted with `1` or `0`.
///
/// This bind group is the output of the obstacle filter and input to the obstacle expander.
type FilteredObstacleMapBindGrp = (StorageBuffer<[u32], HostReadOnly, ShaderReadWrite>,);

/// 1. The maximum safe gradient.
///
/// This bind group is the input to the obstaclemapper.
type ObstacleMapperInputBindGrp = (UniformBuffer<f32>,);

/// 1. The radius of the robot in meters
///
/// This bind group is the input to the expander.
type ExpanderBindGrp = (UniformBuffer<f32>,);

/// The set of bind groups used by the rest of the thalassic pipeline
type BetaBindGroups = (
    GpuBufferSet<SumBindGrp>,
    GpuBufferSet<HeightMapBindGrp>,
    GpuBufferSet<GradMapBindGrp>,
    GpuBufferSet<ObstacleMapBindGrp>,
    GpuBufferSet<FilteredObstacleMapBindGrp>,
    GpuBufferSet<ObstacleMapperInputBindGrp>,
    GpuBufferSet<ExpanderBindGrp>,
);

#[derive(Debug, Clone, Copy)]
pub struct DepthProjectorBuilder {
    pub image_size: Vector2<NonZeroU32>,
    pub focal_length_px: f32,
    pub principal_point_px: Vector2<f32>,
    pub max_depth: f32,
}

impl DepthProjectorBuilder {
    pub fn build(self, thalassic_ref: ThalassicPipelineRef) -> DepthProjector {
        let pixel_count = self.image_size.x.get() * self.image_size.y.get();
        let [depth_fn] = Depth2Pcl {
            depths: BufferGroupBinding::<_, AlphaBindGroups>::get::<0, 0>(),
            points: BufferGroupBinding::<_, AlphaBindGroups>::get::<0, 3>(),
            transform: BufferGroupBinding::<_, AlphaBindGroups>::get::<0, 1>(),
            depth_scale: BufferGroupBinding::<_, AlphaBindGroups>::get::<0, 2>(),
            max_depth: self.max_depth,
            image_width: self.image_size.x,
            focal_length_px: self.focal_length_px,
            principal_point_px: self.principal_point_px.into(),
            pixel_count: NonZeroU32::new(pixel_count).unwrap(),
            half_pixel_count: NonZeroU32::new(pixel_count.div_ceil(2)).unwrap(),
        }
        .compile();
        let [pcl_fn] = Pcl2Sum {
            sum: BufferGroupBinding::<_, AlphaBindGroups>::get::<1, 0>(),
            points: BufferGroupBinding::<_, AlphaBindGroups>::get::<0, 3>(),
            heightmap_width: thalassic_ref.heightmap_dimensions.x,
            cell_size: thalassic_ref.cell_size,
            cell_count: NonZeroU32::new(
                thalassic_ref.heightmap_dimensions.x.get()
                    * thalassic_ref.heightmap_dimensions.y.get(),
            )
            .unwrap(),
            point_count: BufferGroupBinding::<_, AlphaBindGroups>::get::<0, 4>(),
        }
        .compile();

        let mut pipeline = ComputePipeline::new([&depth_fn, &pcl_fn]);
        pipeline.workgroups = [
            Vector3::new(self.image_size.x.get() / 8, self.image_size.y.get() / 8, 1),
            Vector3::new(
                thalassic_ref.heightmap_dimensions.x.get() / 8,
                thalassic_ref.heightmap_dimensions.y.get() / 8,
                1,
            ),
        ];
        DepthProjector {
            image_size: self.image_size,
            pipeline,
            depth_bind_grp: Some(GpuBufferSet::from((
                StorageBuffer::new_dyn(pixel_count.div_ceil(2) as usize).unwrap(),
                UniformBuffer::new(),
                UniformBuffer::new(),
                StorageBuffer::new_dyn(pixel_count as usize).unwrap(),
                UniformBuffer::new(),
            ))),
            sum_bind_grp: thalassic_ref,
        }
    }
}

pub struct DepthProjector {
    image_size: Vector2<NonZeroU32>,
    pipeline: ComputePipeline<AlphaBindGroups, 2>,
    depth_bind_grp: Option<GpuBufferSet<DepthBindGrp>>,
    sum_bind_grp: ThalassicPipelineRef,
}

impl DepthProjector {
    pub fn project(
        &mut self,
        depths: &[u16],
        camera_transform: &AlignedMatrix4<f32>,
        depth_scale: f32,
        point_cloud: Option<&mut [AlignedVec4<f32>]>,
    ) {
        let point_count = self.image_size.x.get() * self.image_size.y.get();
        debug_assert_eq!(depths.len(), point_count as usize);

        let depth_grp = self.depth_bind_grp.take().unwrap();
        let mut sum_bind_grp_lock = self.sum_bind_grp.sum_bind_grp.lock();

        let mut bind_grps = (depth_grp, sum_bind_grp_lock.0.take().unwrap());

        self.pipeline
            .new_pass(|mut lock| {
                // We have to write raw bytes because we can only cast to [u32] if the number of
                // depth pixels is even
                bind_grps
                    .0
                    .write_raw::<0>(bytemuck::cast_slice(depths), &mut lock);
                bind_grps.0.write::<1, _>(camera_transform, &mut lock);
                bind_grps.0.write::<2, _>(&depth_scale, &mut lock);
                bind_grps.0.write::<4, _>(&point_count, &mut lock);
                &mut bind_grps
            })
            .finish();
        let (depth_grp, sum_bind_grp) = bind_grps;
        if let Some(point_cloud) = point_cloud {
            depth_grp.buffers.3.read(point_cloud);
        }
        self.depth_bind_grp = Some(depth_grp);
        sum_bind_grp_lock.0.replace(sum_bind_grp);
        sum_bind_grp_lock.1 = true;
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
    pub heightmap_dimensions: Vector2<NonZeroU32>,
    pub cell_size: f32,
    pub max_point_count: NonZeroU32,
    pub feature_size_cells: u32,
    pub min_feature_count: u32,
}

impl ThalassicBuilder {
    pub fn build(self) -> ThalassicPipeline {
        let cell_count = self.heightmap_dimensions.x.get() * self.heightmap_dimensions.y.get();
        let cell_count = NonZeroU32::new(cell_count).unwrap();

        let [sum_fn] = Sum2Height {
            heightmap: BufferGroupBinding::<_, BetaBindGroups>::get::<1, 0>(),
            heightmap_width: self.heightmap_dimensions.x,
            cell_count,
            min_count: 1.0,
            sum: BufferGroupBinding::<_, BetaBindGroups>::get::<0, 0>(),
        }
        .compile();

        let [grad_fn] = Height2Grad {
            heightmap: BufferGroupBinding::<_, BetaBindGroups>::get::<1, 0>(),
            gradient_map: BufferGroupBinding::<_, BetaBindGroups>::get::<2, 0>(),
            cell_size: self.cell_size,
            heightmap_width: self.heightmap_dimensions.x,
            cell_count,
        }
        .compile();

        let [obstacle_fn] = Grad2Obstacle {
            obstacle_map: BufferGroupBinding::<_, BetaBindGroups>::get::<3, 0>(),
            gradient_map: BufferGroupBinding::<_, BetaBindGroups>::get::<2, 0>(),
            max_gradient: BufferGroupBinding::<_, BetaBindGroups>::get::<6, 0>(),
            height_map: BufferGroupBinding::<_, BetaBindGroups>::get::<1, 0>(),
            heightmap_width: self.heightmap_dimensions.x,
            cell_count,
        }
        .compile();

        let [obstacle_filter] = ObstacleFilter {
            in_obstacles: BufferGroupBinding::<_, BetaBindGroups>::get::<3, 0>(),
            filtered_obstacles: BufferGroupBinding::<_, BetaBindGroups>::get::<4, 0>(),
            feature_size_cells: self.feature_size_cells,
            min_count: self.min_feature_count,
            cell_size: self.cell_size,
            grid_width: self.heightmap_dimensions.x,
            grid_height: self.heightmap_dimensions.y,
        }
        .compile();

        let [expand_fn] = ExpandObstacles {
            obstacles: BufferGroupBinding::<_, BetaBindGroups>::get::<4, 0>(),
            radius: BufferGroupBinding::<_, BetaBindGroups>::get::<6, 0>(),
            cell_size: self.cell_size,
            grid_width: self.heightmap_dimensions.x,
            grid_height: self.heightmap_dimensions.y,
        }
        .compile();

        let mut pipeline = ComputePipeline::new([&sum_fn, &grad_fn, &obstacle_fn, &obstacle_filter, &expand_fn]);
        pipeline.workgroups = [Vector3::new(
            self.heightmap_dimensions.x.get() / 8,
            self.heightmap_dimensions.y.get() / 8,
            1,
        ); 5];

        let bind_grps = (
            GpuBufferSet::from((StorageBuffer::new_dyn(cell_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((StorageBuffer::new_dyn(cell_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((StorageBuffer::new_dyn(cell_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((StorageBuffer::new_dyn(cell_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((UniformBuffer::new(),)),
            GpuBufferSet::from((UniformBuffer::new(),)),
        );

        ThalassicPipeline {
            pipeline,
            bind_grps: Some(bind_grps),
            new_radius: Some(0.25),
            new_max_gradient: Some(45.0f32.to_radians()),
            sum_bind_grp: ThalassicPipelineRef {
                sum_bind_grp: Arc::new(Mutex::new((
                    Some(GpuBufferSet::from((StorageBuffer::new_dyn(
                        cell_count.get() as usize,
                    )
                    .unwrap(),))),
                    false,
                ))),
                heightmap_dimensions: self.heightmap_dimensions,
                cell_size: self.cell_size,
            },
            cell_count
        }
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Occupancy(u32);

impl Occupancy {
    pub const FREE: Self = Self(0);

    pub fn occupied(self) -> bool {
        // True iff the cell is not empty *before* expansion
        // self.0 == 1

        // True iff the cell is not empty
        self.0 != 0
    }
}

#[derive(Clone)]
pub struct ThalassicPipelineRef {
    sum_bind_grp: Arc<Mutex<(Option<GpuBufferSet<SumBindGrp>>, bool)>>,
    heightmap_dimensions: Vector2<NonZeroU32>,
    cell_size: f32,
}

impl ThalassicPipelineRef {
    pub fn noop() -> Self {
        Self {
            sum_bind_grp: Arc::new(Mutex::new((None, false))),
            heightmap_dimensions: Vector2::new(
                NonZeroU32::new(128).unwrap(),
                NonZeroU32::new(128).unwrap(),
            ),
            cell_size: 0.1,
        }
    }
}

pub struct ThalassicPipeline {
    pipeline: ComputePipeline<BetaBindGroups, 5>,
    bind_grps: Option<(
        GpuBufferSet<HeightMapBindGrp>,
        GpuBufferSet<GradMapBindGrp>,
        GpuBufferSet<ObstacleMapBindGrp>,
        GpuBufferSet<FilteredObstacleMapBindGrp>,
        GpuBufferSet<ObstacleMapperInputBindGrp>,
        GpuBufferSet<ExpanderBindGrp>,
    )>,
    new_radius: Option<f32>,
    new_max_gradient: Option<f32>,
    sum_bind_grp: ThalassicPipelineRef,
    cell_count: NonZeroU32,
}

impl ThalassicPipeline {
    pub fn will_process(&self) -> bool {
        self.sum_bind_grp.sum_bind_grp.lock().1
    }

    pub fn process(
        &mut self,
        out_heightmap: &mut [f32],
        out_gradient: &mut [f32],
        out_expanded_obstacles: &mut [Occupancy],
    ) {
        let mut sum_bind_grp_lock = self.sum_bind_grp.sum_bind_grp.lock();

        if !sum_bind_grp_lock.1 {
            return;
        }

        let (height_grp, grad_grp, obstacle_map, filtered_obstacle_map, obstacle_mapper_input_grp, expander_input_grp) =
            self.bind_grps.take().unwrap();

        let mut bind_grps: BetaBindGroups = (
            sum_bind_grp_lock.0.take().unwrap(),
            height_grp,
            grad_grp,
            obstacle_map,
            filtered_obstacle_map,
            obstacle_mapper_input_grp,
            expander_input_grp,
        );

        self.pipeline
            .new_pass(|mut lock| {
                if let Some(new_radius) = self.new_radius.take() {
                    bind_grps.6.write::<0, _>(&new_radius, &mut lock);
                }
                if let Some(new_max_gradient) = self.new_max_gradient.take() {
                    bind_grps.5.write::<0, _>(&new_max_gradient, &mut lock);
                }
                &mut bind_grps
            })
            .finish();

        let (
            points_grp,
            height_grp,
            grad_grp,
            obstacle_map,
            filtered_obstacle_map,
            obstacle_mapper_input_grp,
            expander_input_grp,
        ) = bind_grps;
        height_grp.buffers.0.read(out_heightmap);
        grad_grp.buffers.0.read(out_gradient);
        filtered_obstacle_map
            .buffers
            .0
            .read(bytemuck::cast_slice_mut(out_expanded_obstacles));

        self.bind_grps.replace((
            height_grp,
            grad_grp,
            obstacle_map,
            filtered_obstacle_map,
            obstacle_mapper_input_grp,
            expander_input_grp,
        ));
        sum_bind_grp_lock.0.replace(points_grp);
        sum_bind_grp_lock.1 = false;
    }

    pub fn reset_heightmap(&mut self) {
        let bind_grps = self.bind_grps.as_mut().unwrap();
        bind_grps.0 = GpuBufferSet::from((StorageBuffer::new_dyn(self.cell_count.get() as usize).unwrap(),));
        // bind_grps.1.buffers.0 = StorageBuffer::new_dyn(self.cell_count.get() as usize).unwrap();
    }

    pub fn set_radius(&mut self, radius: f32) {
        self.new_radius = Some(radius);
    }

    pub fn get_ref(&self) -> ThalassicPipelineRef {
        self.sum_bind_grp.clone()
    }
}
