use std::num::NonZeroU32;

use depth2pcl::Depth2Pcl;
use gputter::{
    buffers::{
        storage::{HostReadOnly, HostReadWrite, HostWriteOnly, ShaderReadOnly, ShaderReadWrite, StorageBuffer},
        uniform::UniformBuffer,
        GpuBufferSet,
    },
    compute::ComputePipeline,
    shader::BufferGroupBinding,
    types::{AlignedMatrix4, AlignedVec3, AlignedVec4},
};
use height2grad::Height2Grad;
use nalgebra::{Vector2, Vector3, Vector4};
use pcl2height::Pcl2HeightV2;

mod clustering;
mod depth2pcl;
mod height2grad;
mod pcl2height;
mod grad2obstacle;
pub use clustering::Clusterer;

mod expand_obstacles;
use expand_obstacles::ExpandObstacles;

/// 1. Depths in arbitrary units
/// 2. Global Transform of the camera
/// 3. Depth Scale (meters per depth unit)
///
/// This bind group serves as the input for all DepthProjectors
type DepthBindGrp = (
    StorageBuffer<[u32], HostWriteOnly, ShaderReadOnly>,
    UniformBuffer<AlignedMatrix4<f32>>,
    UniformBuffer<f32>,
);

/// 1. Points in global space
/// 2. Projection Width (the width of the depth camera/image in pixels)
///
/// This bind group is the output of DepthProjectors and the input for the heightmapper ([`pcl2height`])
type PointsBindGrp = (
    StorageBuffer<[AlignedVec4<f32>], HostReadOnly, ShaderReadWrite>,
    UniformBuffer<u32>,
);

/// 1. The height of each cell in the heightmap.
/// The actual type is `f32`, but it is stored as `u32` in the shader to allow for atomic operations, with conversion being a bitwise cast.
/// The units are meters.
///
/// This bind group is the output of the heightmapper ([`pcl2height`]) and the input for the gradientmapper.
type HeightMapBindGrp = (StorageBuffer<[f32], HostReadOnly, ShaderReadWrite>,);

/// 1. A list of triangle indices sorted by height
/// 2. The number of triangles in the list
///
/// This bind group is the input for the heightmapper ([`pcl2height`]) and that is its only usage.
type PclBindGrp = (
    StorageBuffer<[AlignedVec3<u32>], HostWriteOnly, ShaderReadOnly>,
    UniformBuffer<u32>,
);

/// 1. The gradient of each cell in the heightmap expressed as an angle in radians.
///
/// This bind group is the output of the gradientmapper ([`height2grad`]) and the input for the obstaclemapper.
type GradMapBindGrp = (StorageBuffer<[f32], HostReadOnly, ShaderReadWrite>,);

/// The set of bind groups used by the DepthProjector
type AlphaBindGroups = (GpuBufferSet<DepthBindGrp>, GpuBufferSet<PointsBindGrp>);

/// The set of bind groups used by the rest of the thalassic pipeline
type BetaBindGroups = (
    GpuBufferSet<PointsBindGrp>,
    GpuBufferSet<HeightMapBindGrp>,
    GpuBufferSet<PclBindGrp>,
    GpuBufferSet<GradMapBindGrp>,
);

/// 1. Whether or not each cell is an obstacle, denoted with `1` or `0`.
/// 2. The position of the closest obstacle to each position. (should be filled with `0`, used during calculation)
/// 
/// This bind group is the input to the expander.
type ExpanderInputBindGrp = (
    StorageBuffer<[u32], HostReadWrite, ShaderReadWrite>,
    StorageBuffer<[u32], HostReadOnly, ShaderReadWrite>,
);

/// 1. Whether or not each cell is an obstacle, denoted with `1` or `0`.
/// 
/// This bind group is the output of the expander and input to the pathfinder?.
type ExpanderOutputBindGrp = (
    StorageBuffer<[u32], HostReadOnly, ShaderReadWrite>,
);

/// The set of bind groups used by the expander
type ExpanderBindGroups = (
    GpuBufferSet<ExpanderInputBindGrp>,
    GpuBufferSet<ExpanderOutputBindGrp>,
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
            half_pixel_count: NonZeroU32::new(pixel_count.div_ceil(2)).unwrap(),
        }
        .compile();

        let mut pipeline = ComputePipeline::new([&depth_fn]);
        pipeline.workgroups = [Vector3::new(
            self.image_size.x.get() / 8,
            self.image_size.y.get() / 8,
            1,
        )];
        DepthProjector {
            image_size: self.image_size,
            pipeline,
            bind_grp: Some(GpuBufferSet::from((
                StorageBuffer::new_dyn(pixel_count as usize / 2).unwrap(),
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
        depth_scale: f32,
    ) -> PointCloudStorage {
        debug_assert_eq!(self.image_size, points_storage.image_size);
        let depth_grp = self.bind_grp.take().unwrap();

        let mut bind_grps = (depth_grp, points_storage.points_grp);

        self.pipeline
            .new_pass(|mut lock| {
                // We have to write raw bytes because we can only cast to [u32] if the number of
                // depth pixels is even
                bind_grps
                    .0
                    .write_raw::<0>(bytemuck::cast_slice(depths), &mut lock);
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
    pub heightmap_dimensions: Vector2<NonZeroU32>,
    pub cell_size: f32,
    pub max_point_count: NonZeroU32,
}

impl ThalassicBuilder {
    pub fn build(self) -> ThalassicPipeline {
        let max_triangle_count = (self.heightmap_dimensions.x.get() - 1) * (self.heightmap_dimensions.y.get() - 1) * 2;
        let max_triangle_count = NonZeroU32::new(max_triangle_count).expect("max triangle count is 0, each projection dimension must be at least 2");
        let cell_count = self.heightmap_dimensions.x.get() * self.heightmap_dimensions.y.get();
        let cell_count = NonZeroU32::new(cell_count).unwrap();

        let bind_grps = (
            GpuBufferSet::from((StorageBuffer::new_dyn(cell_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((StorageBuffer::new_dyn(max_triangle_count.get() as usize).unwrap(), UniformBuffer::new())),
            GpuBufferSet::from((StorageBuffer::new_dyn(cell_count.get() as usize).unwrap(),)),
        );

        let [height_fn] = Pcl2HeightV2 {
            points: BufferGroupBinding::<_, BetaBindGroups>::get::<0, 0>().cast_hidden().unchecked_cast(),
            heightmap: BufferGroupBinding::<_, BetaBindGroups>::get::<1, 0>(),
            cell_size: self.cell_size,
            heightmap_width: self.heightmap_dimensions.x,
            cell_count,
            max_point_count: self.max_point_count,
            sorted_triangle_indices: BufferGroupBinding::<_, BetaBindGroups>::get::<2, 0>(),
            triangle_count: BufferGroupBinding::<_, BetaBindGroups>::get::<2, 1>(),
            max_triangle_count,
        }
        .compile();

        let [grad_fn] = Height2Grad {
            heightmap: BufferGroupBinding::<_, BetaBindGroups>::get::<1, 0>().cast_hidden(),
            gradient_map: BufferGroupBinding::<_, BetaBindGroups>::get::<3, 0>(),
            cell_size: self.cell_size,
            heightmap_width: self.heightmap_dimensions.x,
            cell_count,
        }
        .compile();

        let mut pipeline = ComputePipeline::new([&height_fn, &grad_fn]);
        pipeline.workgroups[0] = 
            Vector3::new(
                self.heightmap_dimensions.x.get() / 8,
                self.heightmap_dimensions.y.get() / 8,
                1,
            );
        pipeline.workgroups[1] = pipeline.workgroups[0];
        ThalassicPipeline {
            pipeline,
            bind_grps: Some(bind_grps),
            triangle_buffer: Vec::new(),
            points_buffer: Vec::new(),
        }
    }
}

pub struct ThalassicPipeline {
    pipeline: ComputePipeline<BetaBindGroups, 2>,
    bind_grps: Option<(
        GpuBufferSet<HeightMapBindGrp>,
        GpuBufferSet<PclBindGrp>,
        GpuBufferSet<GradMapBindGrp>,
    )>,
    triangle_buffer: Vec<Vector4<u32>>,
    points_buffer: Vec<AlignedVec4<f32>>,
}

impl ThalassicPipeline {
    pub fn provide_points(
        &mut self,
        mut points_storage: PointCloudStorage,
        out_heightmap: &mut [f32],
        out_gradient: &mut [f32],
    ) -> PointCloudStorage {
        let image_width = points_storage.image_size.x.get();
        let image_height = points_storage.image_size.y.get();
        self.points_buffer.resize(image_width as usize * image_height as usize, AlignedVec4::default());
        points_storage.points_grp.buffers.0.read(&mut self.points_buffer);

        self.triangle_buffer.clear();
        self.triangle_buffer.extend(
            (0..(image_height - 1))
                .flat_map(|y| (0..(image_width - 1)).map(move |x| (x, y)))
                .flat_map(|(x, y)| {
                    let current = x + y * image_width;
                    let next = current + 1;
                    let below = current + image_width;
                    let below_next = below + 1;
                    [
                        (current, below, next),
                        (next, below, below_next),
                    ]
                })
                .filter_map(|(x, y, z)| {
                    let v1 = self.points_buffer[x as usize];
                    let v2 = self.points_buffer[y as usize];
                    let v3 = self.points_buffer[z as usize];

                    if v1.w == 0.0 || v2.w == 0.0 || v3.w == 0.0 {
                        None
                    } else {
                        Some(
                            Vector4::new(x, y, z, f32::to_bits((v1.y + v2.y + v3.y) / 3.0)),
                        )
                    }
                })
        );
        if self.triangle_buffer.is_empty() {
            return points_storage;
        }
        glidesort::sort_by(&mut self.triangle_buffer, |a, b| {
            f32::from_bits(a.w).total_cmp(&f32::from_bits(b.w)).reverse()
        });

        let (height_grp, pcl_grp, grad_grp) = self.bind_grps.take().unwrap();
        let mut bind_grps: BetaBindGroups =
            (points_storage.points_grp, height_grp, pcl_grp, grad_grp);

        self.pipeline
            .new_pass(|mut lock| {
                bind_grps
                    .2
                    // The height element is skipped as the underlying type is vec3, which has padding at the end
                    .write::<0, _>(bytemuck::cast_slice(&self.triangle_buffer), &mut lock);
                bind_grps
                    .2
                    .write::<1, _>(&(self.triangle_buffer.len() as u32), &mut lock);
                &mut bind_grps
            })
            .finish();

        let (points_grp, height_grp, pcl_grp, grad_grp) = bind_grps;
        height_grp.buffers.0.read(out_heightmap);
        grad_grp.buffers.0.read(out_gradient);

        self.bind_grps = Some((height_grp, pcl_grp, grad_grp));
        points_storage.points_grp = points_grp;
        points_storage
    }
}

// pub struct Expander {}
// impl Expander {

//     //TODO: populate constants
//     const RADIUS: f32 = 3.0;
//     const GRID_WIDTH: u32 = 10;
//     const GRID_HEIGHT: u32 = 10;
    
//     pub fn expand(obstacles: &[u32]) -> GpuBufferSet<ExpanderOutputBindGrp> {        
//         let grid_size = (Self::GRID_WIDTH * Self::GRID_HEIGHT) as usize;

//         let [expand_fn] = ExpandObstacles {
//             obstacles: BufferGroupBinding::<_, ExpanderBindGroups>::get::<0, 0>(),
//             closest: BufferGroupBinding::<_, ExpanderBindGroups>::get::<0, 1>(),
//             expanded: BufferGroupBinding::<_, ExpanderBindGroups>::get::<1, 0>(),
//             radius: Self::RADIUS,
//             grid_width: NonZeroU32::new(Self::GRID_WIDTH).unwrap(),
//             grid_height: NonZeroU32::new(Self::GRID_HEIGHT).unwrap(),
//         }
//         .compile();

//         let mut pipeline = ComputePipeline::new([&expand_fn]);
//         pipeline.workgroups = [Vector3::new(
//             Self::GRID_WIDTH,
//             Self::GRID_HEIGHT,
//             1,
//         )];

//         let mut bind_grps = (
//             GpuBufferSet::from((
//                 StorageBuffer::new_dyn(grid_size).unwrap(), 
//                 StorageBuffer::new_dyn(grid_size * 2).unwrap(),
//             )), 
//             GpuBufferSet::from((
//                 StorageBuffer::new_dyn(grid_size).unwrap(), 
//             ))
//         );

//         // write obstacle data to buffer before the first pass...
//         pipeline
//             .new_pass(|mut lock| {
//                 bind_grps.0.write::<0, _>( obstacles, &mut lock);
//                 &mut bind_grps
//             })
//             .finish();

//         // ...no need to touch buffers again in the remaining (radius-1) passes
//         for _ in 0..(Self::RADIUS as usize) {
//             pipeline
//                 .new_pass(|mut _lock| &mut bind_grps)
//                 .finish();
//         }

//         bind_grps.1
//     }
// }