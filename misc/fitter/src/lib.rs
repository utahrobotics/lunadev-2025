use bytemuck::cast_ref;
use compute_shader::{
    buffers::{
        BufferType, DynamicSize, HostReadWrite, HostWriteOnly, ShaderReadOnly,
        ShaderReadWrite, TypedOpaqueBuffer,
    },
    wgpu, Compute,
};
use crossbeam::queue::SegQueue;
use nalgebra::{Rotation3, Vector2, Vector3, Vector4};
use rand::{thread_rng, Rng};

// Points, Translation
type TranslateShader = Compute<(
    BufferType<[Vector4<f32>], HostWriteOnly, ShaderReadWrite>,
    BufferType<[f32; 3], HostWriteOnly, ShaderReadOnly>,
)>;
// Points, Rotation (Mat3 with padding), Origin
type RotateShader = Compute<(
    BufferType<[Vector4<f32>], HostWriteOnly, ShaderReadWrite>,
    BufferType<[[f32; 4]; 3], HostWriteOnly, ShaderReadOnly>,
    BufferType<[f32; 3], HostWriteOnly, ShaderReadOnly>,
)>;
// Points (Option<[f32; 3]>), Distance (Atomicf32), Translations (Vec3), Rotations (Mat3 with padding), PointsOrigin
type FitterShader = Compute<(
    BufferType<[Vector4<f32>], HostWriteOnly, ShaderReadOnly>,
    BufferType<[u32], HostReadWrite, ShaderReadOnly>,
    BufferType<[[f32; 4]], HostWriteOnly, ShaderReadOnly>,
    BufferType<[[[f32; 4]; 3]], HostWriteOnly, ShaderReadOnly>,
    BufferType<[f32; 3], HostWriteOnly, ShaderReadOnly>,
)>;

pub struct Plane {
    pub rotation_matrix: Rotation3<f32>,
    pub origin: Vector3<f32>,
    pub size: Vector2<f32>,
}

impl Plane {
    fn to_string(&self) -> String {
        let inv = self.rotation_matrix.inverse();
        let matrix = inv.matrix();
        format!(
            "Plane{{rotation_matrix:mat3x3({},{},{},{},{},{},{},{},{}),origin:vec3({},{},{}),half_size:vec2({},{})}}",
            matrix[0], matrix[1], matrix[2],
            matrix[3], matrix[4], matrix[5],
            matrix[6], matrix[7], matrix[8],
            self.origin.x, self.origin.y, self.origin.z,
            self.size.x / 2.0, self.size.y / 2.0
        )
    }
}

pub struct BufferFitter {
    iterations: usize,
    sample_count: usize,
    max_translation: f32,
    max_rotation: f32,

    translate_shader: TranslateShader,
    rotate_shader: RotateShader,
    fitter_shader: FitterShader,

    point_buffers: SegQueue<TypedOpaqueBuffer<[Vector4<f32>]>>,
    sample_buffers: SegQueue<(Box<[[f32; 4]]>, Box<[[[f32; 4]; 3]]>)>,
    distances_reset: Box<[u32]>,
    distances_buffers: SegQueue<Box<[u32]>>,
}

impl BufferFitter {
    pub async fn fit_sparse(&self, points: &mut [Option<Vector3<f32>>]) -> anyhow::Result<()> {
        let mut point_buffer = match self.point_buffers.pop() {
            Some(x) => x,
            None => TypedOpaqueBuffer::new(DynamicSize::<Vector4<f32>>::new(points.len())).await?,
        };
        point_buffer.get_slice_mut(|slice| {
            points.iter().zip(slice).for_each(|(src, dst)| {
                if let Some(src) = src {
                    *dst = Vector4::new(src.x, src.y, src.z, 1.0);
                } else {
                    *dst = Vector4::default();
                }
            });
        }).await;

        let result = self.fit_buffer(&mut point_buffer).await;
        point_buffer.get_slice(|slice| {
            points.iter_mut().zip(slice).for_each(|(dst, src)| {
                if src.w == 1.0 {
                    *dst = Some(Vector3::new(src.x, src.y, src.z));
                } else {
                    *dst = None;
                }
            });
        }).await;

        self.point_buffers.push(point_buffer);
        result
    }

    pub async fn fit_dense(&self, points: &mut [Vector3<f32>]) -> anyhow::Result<()> {
        let mut point_buffer = match self.point_buffers.pop() {
            Some(x) => x,
            None => TypedOpaqueBuffer::new(DynamicSize::<Vector4<f32>>::new(points.len())).await?,
        };
        point_buffer.get_slice_mut(|slice| {
            points
                .iter()
                .map(|src| Vector4::new(src.x, src.y, src.z, 1.0))
                .chain(std::iter::repeat(Vector4::default()))
                .zip(slice)
                .for_each(|(src, dst)| {
                    *dst = src;
                });
        }).await;

        let result = self.fit_buffer(&mut point_buffer).await;
        point_buffer.get_slice(|slice| {
            points.iter_mut().zip(slice).for_each(|(dst, src)| {
                debug_assert_eq!(src.w, 1.0);
                *dst = Vector3::new(src.x, src.y, src.z);
            });
        }).await;

        self.point_buffers.push(point_buffer);
        result
    }

    pub async fn fit_buffer(
        &self,
        point_buffer: &mut TypedOpaqueBuffer<[Vector4<f32>]>,
    ) -> anyhow::Result<()> {
        let mut origin = Vector3::default();
        point_buffer.get_slice(|points| {
            let mut count = 0usize;

            for p in &*points {
                if p.w != 0.0 {
                    origin += Vector3::new(p.x, p.y, p.z);
                    count += 1;
                }
            }
            origin.unscale_mut(count as f32);
        }).await;

        let (mut translation_buffer, mut rotation_buffer) =
            self.sample_buffers.pop().unwrap_or_else(|| {
                (
                    vec![[0.0, 0.0, 0.0, 0.0]; self.sample_count + 1].into_boxed_slice(),
                    vec![
                        [
                            [1.0, 0.0, 0.0, 0.0],
                            [0.0, 1.0, 0.0, 0.0],
                            [0.0, 0.0, 1.0, 0.0]
                        ];
                        self.sample_count + 1
                    ]
                    .into_boxed_slice(),
                )
            });

        let mut rng = thread_rng();

        for _ in 0..self.iterations {
            for i in 1..=self.sample_count {
                translation_buffer[i] = [
                    rng.gen_range(-self.max_translation..=self.max_translation),
                    rng.gen_range(-self.max_translation..=self.max_translation),
                    rng.gen_range(-self.max_translation..=self.max_translation),
                    0.0,
                ];
                let mut rand_axis: Vector3<f32> = Vector3::new(
                    rng.gen_range(-1.0..=1.0),
                    rng.gen_range(-1.0..=1.0),
                    rng.gen_range(-1.0..=1.0),
                );
                rand_axis.normalize_mut();

                let rot_mat = Rotation3::new(
                    rand_axis * rng.gen_range(-self.max_rotation..=self.max_rotation),
                )
                .into_inner();
                rotation_buffer[i] = [
                    [rot_mat[0], rot_mat[1], rot_mat[2], 0.0],
                    [rot_mat[3], rot_mat[4], rot_mat[5], 0.0],
                    [rot_mat[6], rot_mat[7], rot_mat[8], 0.0],
                ];
            }
            let mut distances_buffer = self
                .distances_buffers
                .pop()
                .unwrap_or_else(|| vec![0; self.sample_count * 2 + 2].into_boxed_slice());
            self.fitter_shader
                .new_pass(
                    &**point_buffer,
                    &*self.distances_reset,
                    &*translation_buffer,
                    &*rotation_buffer,
                    cast_ref::<_, [f32; 3]>(&origin),
                )
                .call((), &mut *distances_buffer, (), (), ())
                .await;
            let (min_i, _) = distances_buffer
                .iter()
                .copied()
                .enumerate()
                .min_by_key(|&(_, dist)| dist)
                .unwrap();
            self.distances_buffers.push(distances_buffer);

            if min_i % 2 == 0 {
                let min_translation = translation_buffer[min_i / 2];
                self.translate_shader
                    .new_pass(
                        &**point_buffer,
                        &[min_translation[0], min_translation[1], min_translation[2]],
                    )
                    .call(&mut **point_buffer, ())
                    .await;
            } else {
                let min_rotation = rotation_buffer[min_i / 2];
                self.rotate_shader
                    .new_pass(
                        &**point_buffer,
                        &min_rotation,
                        cast_ref::<_, [f32; 3]>(&origin),
                    )
                    .call(&mut **point_buffer, (), ())
                    .await;
            }
        }

        self.sample_buffers
            .push((translation_buffer, rotation_buffer));

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BufferFitterBuilder {
    pub point_count: usize,
    pub iterations: usize,
    pub max_translation: f32,
    pub max_rotation: f32,
    pub sample_count: usize,
    pub distance_resolution: f32,
}

impl BufferFitterBuilder {
    pub async fn build(self, planes: &[Plane]) -> anyhow::Result<BufferFitter> {
        let variation_count = self.iterations * self.sample_count + 1;
        let distance_resolution = 1.0 / self.distance_resolution;
        let mut planes_str = String::new();

        for plane in planes {
            planes_str.push_str(&plane.to_string());
            planes_str.push_str(",");
        }
        planes_str.pop();

        let shader = format!(
            r#"
@group(0) @binding(0) var<storage, read_write> points: array<vec3<f32>, {}>;
@group(0) @binding(1) var<uniform, read> translation: vec3<f32>;

@compute
@workgroup_size({}, 1, 1)
fn main(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
) {{
    points[workgroup_id.x] += translation;
}}
"#,
            self.point_count, self.point_count,
        );

        let translate_shader = TranslateShader::new(
            wgpu::ShaderModuleDescriptor {
                label: Some("TranslateShader"),
                source: wgpu::ShaderSource::Wgsl(shader.into()),
            },
            BufferType::new_dyn(self.point_count),
            BufferType::new(),
        )
        .await?;

        let shader = format!(
            r#"
@group(0) @binding(0) var<storage, read_write> points: array<vec3<f32>, {}>;
@group(0) @binding(1) var<uniform, read> rotation: mat3x3<f32>;
@group(0) @binding(2) var<uniform, read> origin: vec3<f32>;

@compute
@workgroup_size({}, 1, 1)
fn main(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
) {{
    points[workgroup_id.x] = rotation * (points[workgroup_id.x] - origin) + origin;
}}
"#,
            self.point_count, self.point_count,
        );

        let rotate_shader = RotateShader::new(
            wgpu::ShaderModuleDescriptor {
                label: Some("RotateShader"),
                source: wgpu::ShaderSource::Wgsl(shader.into()),
            },
            BufferType::new_dyn(self.point_count),
            BufferType::new(),
            BufferType::new(),
        )
        .await?;

        let shader = format!(
            r#"
@group(0) @binding(0) var<storage, read> points: array<vec4<f32>, {}>;
@group(0) @binding(1) var<storage, read_write> distances: array<atomic<u32>, {}>;
@group(0) @binding(2) var<storage, read> translations: array<vec3<f32>, {}>;
@group(0) @binding(3) var<storage, read> rotations: array<mat3x3<f32>, {}>;
@group(0) @binding(4) var<uniform, read> origin: vec3<f32>;

struct Plane {{
    inv_rotation_matrix: mat3x3<f32>;
    origin: vec3<f32>;
    half_size: vec2<f32>;
}};

const PLANES = array<Plane, {}>({planes_str});
const DISTANCE_RESOLUTION = {distance_resolution};

@compute
@workgroup_size({}, 2, {})
fn main(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
) {{
    var point = points[workgroup_id.x];

    if point.w == 0.0 {{
        return;
    }}

    if workgroup_id.y == 0 {{
        point = point + translations[workgroup_id.z];
    }} else {{
        point = rotations[workgroup_id.z] * (point - origin) + origin;
    }}

    var min_distance = {};
    for (var i = 0; i < {}, i++) {{
        let plane = PLANES[i];
        let transformed_point = plane.inv_rotation_matrix * (point - plane.origin);
        var dist: f32;

        if abs(transformed_point.x) <= plane.half_size.x {{
            if abs(transformed_point.y) <= plane.half_size.y {{
                dist = abs(transformed_point.z);
            }} else {{
                dist = distance(
                    transformed_point,
                    vec3(
                        transformed_point.x,
                        sign(transformed_point.y) * plane.half_size.y,
                        transformed_point.z
                    )
                );
            }}

        }} else if abs(transformed_point.y) <= plane.half_size.y {{
            dist = distance(
                transformed_point,
                vec3(
                    sign(transformed_point.x) * plane.half_size.x,
                    transformed_point.y,
                    transformed_point.z
                )
            );

        }} else {{
            dist = distance(
                transformed_point,
                vec3(
                    sign(transformed_point.x) * plane.half_size.x,
                    sign(transformed_point.y) * plane.half_size.y,
                    transformed_point.z
                )
            );
        }}

        if dist < min_distance {{
            min_distance = dist;
        }}
    }}

    atomicAdd(&distances[workgroup_id.z * 2 + workgroup_id.y], u32(round(min_distance * DISTANCE_RESOLUTION)));
}}
"#,
            self.point_count,
            self.sample_count * 2 + 2,
            self.sample_count + 1,
            self.sample_count + 1,
            planes.len(),
            self.point_count,
            f32::MAX,
            self.sample_count + 1,
            planes.len()
        );

        log::debug!("Compiling shader:\n\n{shader}");

        let fitter_shader = FitterShader::new(
            wgpu::ShaderModuleDescriptor {
                label: Some("FitterShader"),
                source: wgpu::ShaderSource::Wgsl(shader.into()),
            },
            BufferType::new_dyn(self.point_count),
            BufferType::new_dyn(variation_count),
            BufferType::new_dyn(variation_count),
            BufferType::new_dyn(variation_count),
            BufferType::new(),
        )
        .await?;

        Ok(BufferFitter {
            translate_shader,
            rotate_shader,
            fitter_shader,
            iterations: self.iterations,
            sample_count: self.sample_count,
            max_translation: self.max_translation,
            max_rotation: self.max_rotation,
            point_buffers: SegQueue::default(),
            sample_buffers: SegQueue::default(),
            distances_reset: vec![0; self.sample_count * 2 + 2].into_boxed_slice(),
            distances_buffers: SegQueue::default(),
        })
    }
}
