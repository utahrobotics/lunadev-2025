use compute_shader::{
    buffers::{
        BufferType, HostReadOnly, HostWriteOnly, ShaderReadOnly, ShaderReadWrite, UniformOnly,
    },
    wgpu, Compute,
};
use crossbeam::queue::SegQueue;
use nalgebra::{Isometry3, Matrix4, Vector2, Vector4};

type ProjectShader = Compute<(
    BufferType<[u32], HostWriteOnly, ShaderReadOnly>,
    BufferType<[Vector4<f32>], HostReadOnly, ShaderReadWrite>,
    BufferType<Matrix4<f32>, HostWriteOnly, ShaderReadOnly, UniformOnly>,
)>;

pub struct CameraProjection {
    project_shader: ProjectShader,
    image_size: Vector2<u32>,
    point_buffers: SegQueue<Box<[Vector4<f32>]>>,
}

impl CameraProjection {
    pub async fn project<T>(
        &self,
        depths: &[u32],
        camera_isometry: Isometry3<f32>,
        f: impl FnOnce(&[Vector4<f32>]) -> T,
    ) -> T {
        let mut point_buffer = self.point_buffers.pop().unwrap_or_else(|| {
            vec![Vector4::default(); self.image_size.x as usize * self.image_size.y as usize]
                .into_boxed_slice()
        });
        let mat4 = camera_isometry.to_matrix();

        self.project_shader
            .new_pass(depths, (), &mat4)
            .workgroup_size(self.image_size.x, self.image_size.y, 1)
            .call((), &mut *point_buffer, ())
            .await;
        let result = f(&point_buffer);
        self.point_buffers.push(point_buffer);
        result
    }

    pub async fn new(
        focal_length_px: f32,
        image_size: Vector2<u32>,
        depth_scale: f32,
    ) -> anyhow::Result<Self> {
        let principal_point_px = image_size / 2;
        let pixel_count = image_size.x as usize * image_size.y as usize;
        let shader = format!(
            r#"
@group(0) @binding(0) var<storage, read> depths: array<u32, {pixel_count}>;
@group(0) @binding(1) var<storage, read_write> points: array<vec4<f32>, {pixel_count}>;
@group(0) @binding(2) var<uniform> transform: mat4x4<f32>;

const FOCAL_LENGTH_PX: f32 = {focal_length_px};
const PRINCIPAL_POINT_PX = vec2<f32>({}, {});
const IMAGE_WIDTH: u32 = {};
const DEPTH_SCALE: f32 = {};

@compute
@workgroup_size(1, 1, 1)
fn main(
     @builtin(workgroup_id) workgroup_id : vec3<u32>,
) {{
    let i = workgroup_id.x + workgroup_id.y * IMAGE_WIDTH;

    if depths[i] == 0 {{
        points[i].w = 0.0;
        return;
    }}

    let depth = f32(depths[i]) * DEPTH_SCALE;
    let y = (f32(workgroup_id.y) - f32(PRINCIPAL_POINT_PX.y)) / FOCAL_LENGTH_PX;
    let x = (f32(workgroup_id.x) - f32(PRINCIPAL_POINT_PX.x)) / FOCAL_LENGTH_PX;

    let point = vec4<f32>(x * depth, y * depth, -depth, 1.0);
    points[i] = vec4<f32>(point.xyz, 1.0);
}}
"#,
            principal_point_px.x, principal_point_px.y, image_size.x, depth_scale,
        );

        let project_shader = ProjectShader::new(
            wgpu::ShaderModuleDescriptor {
                label: Some("ProjectShader"),
                source: wgpu::ShaderSource::Wgsl(shader.into()),
            },
            BufferType::new_dyn(pixel_count),
            BufferType::new_dyn(pixel_count),
            BufferType::new(),
        )
        .await?;

        Ok(Self {
            project_shader,
            point_buffers: SegQueue::new(),
            image_size,
        })
    }
}
