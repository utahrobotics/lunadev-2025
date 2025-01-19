use std::{f32::consts::PI, num::NonZeroU32};

use bytemuck::{Pod, Zeroable};
use depth2pcl::Depth2Pcl;
use gputter::{
    buffers::{
        storage::{HostReadOnly, HostWriteOnly, ShaderReadOnly, ShaderReadWrite, StorageBuffer},
        uniform::UniformBuffer,
        GpuBufferSet,
    },
    compute::ComputePipeline,
    shader::BufferGroupBinding,
    types::{AlignedMatrix4, AlignedVec3, AlignedVec4},
};
use grad2obstacle::Grad2Obstacle;
use height2grad::Height2Grad;
use nalgebra::{Vector2, Vector3, Vector4};
use pcl2height::Pcl2HeightV2;

// mod clustering;
mod depth2pcl;
mod grad2obstacle;
mod height2grad;
mod pcl2height;
// pub use clustering::Clusterer;

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

/// The set of bind groups used by the DepthProjector
type AlphaBindGroups = (GpuBufferSet<DepthBindGrp>, GpuBufferSet<PointsBindGrp>);

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

/// 1. Whether or not each cell is an obstacle, denoted with `1` or `0`.
///
/// This bind group is the output of the obstaclemapper and input to the obstacle expander.
type ObstacleMapBindGrp = (StorageBuffer<[u32], HostReadOnly, ShaderReadWrite>,);

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
    GpuBufferSet<PointsBindGrp>,
    GpuBufferSet<HeightMapBindGrp>,
    GpuBufferSet<PclBindGrp>,
    GpuBufferSet<GradMapBindGrp>,
    GpuBufferSet<ObstacleMapBindGrp>,
    GpuBufferSet<ObstacleMapperInputBindGrp>,
    GpuBufferSet<ExpanderBindGrp>,
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
                StorageBuffer::new_dyn(pixel_count.div_ceil(2) as usize).unwrap(),
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
        debug_assert_eq!(
            depths.len(),
            self.image_size.x.get() as usize * self.image_size.y.get() as usize
        );

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
    pub max_triangle_count: NonZeroU32,
}

impl ThalassicBuilder {
    pub fn build(self) -> ThalassicPipeline {
        let cell_count = self.heightmap_dimensions.x.get() * self.heightmap_dimensions.y.get();
        let cell_count = NonZeroU32::new(cell_count).unwrap();

        let [height_fn] = Pcl2HeightV2 {
            points: BufferGroupBinding::<_, BetaBindGroups>::get::<0, 0>().unchecked_cast(),
            heightmap: BufferGroupBinding::<_, BetaBindGroups>::get::<1, 0>(),
            cell_size: self.cell_size,
            heightmap_width: self.heightmap_dimensions.x,
            cell_count,
            max_point_count: self.max_point_count,
            sorted_triangle_indices: BufferGroupBinding::<_, BetaBindGroups>::get::<2, 0>(),
            triangle_count: BufferGroupBinding::<_, BetaBindGroups>::get::<2, 1>(),
            max_triangle_count: self.max_triangle_count,
        }
        .compile();

        let [grad_fn] = Height2Grad {
            heightmap: BufferGroupBinding::<_, BetaBindGroups>::get::<1, 0>(),
            gradient_map: BufferGroupBinding::<_, BetaBindGroups>::get::<3, 0>(),
            cell_size: self.cell_size,
            heightmap_width: self.heightmap_dimensions.x,
            cell_count,
        }
        .compile();

        let [obstacle_fn] = Grad2Obstacle {
            obstacle_map: BufferGroupBinding::<_, BetaBindGroups>::get::<4, 0>(),
            gradient_map: BufferGroupBinding::<_, BetaBindGroups>::get::<3, 0>(),
            max_gradient: BufferGroupBinding::<_, BetaBindGroups>::get::<5, 0>(),
            height_map: BufferGroupBinding::<_, BetaBindGroups>::get::<1, 0>(),
            heightmap_width: self.heightmap_dimensions.x,
            cell_count,
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

        let mut pipeline = ComputePipeline::new([&height_fn, &grad_fn, &obstacle_fn, &expand_fn]);
        pipeline.workgroups = [Vector3::new(
            self.heightmap_dimensions.x.get() / 8,
            self.heightmap_dimensions.y.get() / 8,
            1,
        ); 4];

        let bind_grps = (
            GpuBufferSet::from((StorageBuffer::new_dyn(cell_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((
                StorageBuffer::new_dyn(self.max_triangle_count.get() as usize).unwrap(),
                UniformBuffer::new(),
            )),
            GpuBufferSet::from((StorageBuffer::new_dyn(cell_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((StorageBuffer::new_dyn(cell_count.get() as usize).unwrap(),)),
            GpuBufferSet::from((UniformBuffer::new(),)),
            GpuBufferSet::from((UniformBuffer::new(),)),
        );

        ThalassicPipeline {
            pipeline,
            bind_grps: Some(bind_grps),
            triangle_buffer: Vec::new(),
            points_buffer: Vec::new(),
            new_radius: Some(0.25),
            new_max_gradient: Some(45.0f32.to_radians()),
        }
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Occupancy(u32);

impl Occupancy {
    pub const FREE: Self = Self(0);

    pub fn occupied(self) -> bool {
        self.0 != 0
    }
}

pub struct ThalassicPipeline {
    pipeline: ComputePipeline<BetaBindGroups, 4>,
    bind_grps: Option<(
        GpuBufferSet<HeightMapBindGrp>,
        GpuBufferSet<PclBindGrp>,
        GpuBufferSet<GradMapBindGrp>,
        GpuBufferSet<ObstacleMapBindGrp>,
        GpuBufferSet<ObstacleMapperInputBindGrp>,
        GpuBufferSet<ExpanderBindGrp>,
    )>,
    triangle_buffer: Vec<Vector4<u32>>,
    points_buffer: Vec<AlignedVec4<f32>>,
    new_radius: Option<f32>,
    new_max_gradient: Option<f32>,
}

impl ThalassicPipeline {
    pub fn provide_points(
        &mut self,
        mut points_storage: PointCloudStorage,
        out_heightmap: &mut [f32],
        out_gradient: &mut [f32],
        out_expanded_obstacles: &mut [Occupancy],
    ) -> PointCloudStorage {
        let image_width = points_storage.image_size.x.get();
        let image_height = points_storage.image_size.y.get();
        self.points_buffer.resize(
            image_width as usize * image_height as usize,
            AlignedVec4::default(),
        );
        points_storage
            .points_grp
            .buffers
            .0
            .read(&mut self.points_buffer);

        self.triangle_buffer.clear();
        self.triangle_buffer.extend(
            (0..(image_height - 1))
                .flat_map(|y| (0..(image_width - 1)).map(move |x| (x, y)))
                .flat_map(|(x, y)| {
                    let current = x + y * image_width;
                    let next = current + 1;
                    let below = current + image_width;
                    let below_next = below + 1;
                    [(current, below, next), (next, below, below_next)]
                })
                .filter_map(|(x, y, z)| {
                    let v1 = self.points_buffer[x as usize];
                    let v2 = self.points_buffer[y as usize];
                    let v3 = self.points_buffer[z as usize];

                    if v1.w == 0.0 || v2.w == 0.0 || v3.w == 0.0 {
                        None
                    } else {
                        let p1 = Vector3::new(v1.x, v1.y, v1.z);
                        let p2 = Vector3::new(v2.x, v2.y, v2.z);
                        let p3 = Vector3::new(v3.x, v3.y, v3.z);

                        let l1 = (p1 - p2).magnitude();
                        let l2 = (p2 - p3).magnitude();
                        let l3 = (p3 - p1).magnitude();

                        // Heron's formula
                        let s = (l1 + l2 + l3) / 2.0;
                        let triangle_area = (s * (s - l1) * (s - l2) * (s - l3)).sqrt();

                        let circumradius = l1 * l2 * l3 / (4.0 * triangle_area);
                        let circle_area = PI * circumradius * circumradius;

                        if triangle_area / circle_area < 0.2 {
                            None
                        } else {
                            Some(Vector4::new(
                                x,
                                y,
                                z,
                                f32::to_bits((v1.y + v2.y + v3.y) / 3.0),
                            ))
                        }
                    }
                }),
        );
        if self.triangle_buffer.is_empty() {
            return points_storage;
        }
        glidesort::sort_in_vec_by(&mut self.triangle_buffer, |a, b| {
            f32::from_bits(a.w)
                .total_cmp(&f32::from_bits(b.w))
                .reverse()
        });

        let (
            height_grp,
            pcl_grp,
            grad_grp,
            obstacle_map,
            obstacle_mapper_input_grp,
            expander_input_grp,
        ) = self.bind_grps.take().unwrap();
        let mut bind_grps: BetaBindGroups = (
            points_storage.points_grp,
            height_grp,
            pcl_grp,
            grad_grp,
            obstacle_map,
            obstacle_mapper_input_grp,
            expander_input_grp,
        );

        self.pipeline
            .new_pass(|mut lock| {
                bind_grps
                    .2
                    // The height element is skipped as the underlying type is vec3, which has padding at the end
                    .write::<0, _>(bytemuck::cast_slice(&self.triangle_buffer), &mut lock);
                bind_grps
                    .2
                    .write::<1, _>(&(self.triangle_buffer.len() as u32), &mut lock);
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
            pcl_grp,
            grad_grp,
            obstacle_map,
            obstacle_mapper_input_grp,
            expander_input_grp,
        ) = bind_grps;
        height_grp.buffers.0.read(out_heightmap);
        grad_grp.buffers.0.read(out_gradient);
        obstacle_map
            .buffers
            .0
            .read(bytemuck::cast_slice_mut(out_expanded_obstacles));

        self.bind_grps = Some((
            height_grp,
            pcl_grp,
            grad_grp,
            obstacle_map,
            obstacle_mapper_input_grp,
            expander_input_grp,
        ));
        points_storage.points_grp = points_grp;
        points_storage
    }

    pub fn set_radius(&mut self, radius: f32) {
        self.new_radius = Some(radius);
    }
}
