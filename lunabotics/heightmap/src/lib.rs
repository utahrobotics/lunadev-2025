use std::future::Future;

use compute_shader::{
    buffers::{
        BufferSource, BufferType, GpuBuffer, HostReadWrite, HostWriteOnly, ReadGuard,
        ShaderReadOnly, ShaderReadWrite,
    },
    wgpu, Compute,
};
use nalgebra::{Vector2, Vector4};

type HeightMapBuffers = (
    // In the shader code, the type is atomic<u32>, but in the shader f32s are bitcasted to u32s so that they can be atomically updated
    BufferType<[f32], HostReadWrite, ShaderReadWrite>,
    BufferType<[Vector4<f32>], HostWriteOnly, ShaderReadOnly>,
    BufferType<[f32], HostWriteOnly, ShaderReadOnly>,
    // BufferType<[f32; 10], HostReadWrite, ShaderReadWrite>,
);
type HeightMapShader = Compute<HeightMapBuffers>;

pub struct HeightMapper {
    heightmap_size: Vector2<u32>,
    heightmap: GpuBuffer<[f32]>,
    heightmap_shader: HeightMapShader,
    tri_count: u32,
}

impl HeightMapper {
    pub async fn new(
        heightmap_size: Vector2<u32>,
        cell_size: f32,
        projection_size: Vector2<u32>,
    ) -> anyhow::Result<Self> {
        let cell_count = heightmap_size.x as usize * heightmap_size.y as usize;
        let point_count = projection_size.x * projection_size.y;
        let shader = format!(
            r#"
@group(0) @binding(0) var<storage, read_write> heightmap: array<atomic<u32>, {cell_count}>;
@group(0) @binding(1) var<storage, read> points: array<vec4<f32>, {point_count}>;
@group(0) @binding(2) var<storage, read> original_heightmap: array<f32, {cell_count}>;
// @group(0) @binding(3) var<storage, read_write> debug_out: array<f32, 10>;

const PROJECTION_WIDTH: u32 = {};
const HEIGHTMAP_WIDTH: u32 = {};
const CELL_SIZE: f32 = {cell_size};

fn barycentric(pv1: vec3<f32>, pv2: vec3<f32>, pv3: vec3<f32>, pp: vec2<f32>) -> vec3<f32> {{
    let v0 = vec3<f32>(pv1.x, pv1.z, 0.0);
    let v1 = vec3<f32>(pv2.x, pv2.z, 0.0);
    let v2 = vec3<f32>(pv3.x, pv3.z, 0.0);
    let p = vec3<f32>(pp.x, pp.y, 0.0);

    let d00 = dot(v0 - v2, v0 - v2);
    let d01 = dot(v0 - v2, v1 - v2);
    let d11 = dot(v1 - v2, v1 - v2);
    let d20 = dot(p - v2, v0 - v2);
    let d21 = dot(p - v2, v1 - v2);

    let invDenom = 1.0 / (d00 * d11 - d01 * d01);
    let v = (d11 * d20 - d01 * d21) * invDenom;
    let w = (d00 * d21 - d01 * d20) * invDenom;
    let u = 1.0 - v - w;

    return vec3<f32>(u, v, w);
}}

@compute
@workgroup_size(1, 1, 1)
fn main(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
) {{
    let heightmap_x = f32(workgroup_id.x) * CELL_SIZE;
    let heightmap_y = f32(workgroup_id.y) * CELL_SIZE;
    let heightmap_index = workgroup_id.y * HEIGHTMAP_WIDTH + workgroup_id.x;
    let tri_index = workgroup_id.z;
    let half_layer_index = tri_index / (PROJECTION_WIDTH - 1);
    let layer_index = half_layer_index / 2;
    let projection_x = tri_index % (PROJECTION_WIDTH - 1);
    let v1_index = layer_index * PROJECTION_WIDTH + projection_x;
    let v1 = points[v1_index];
    var v2: vec4<f32>;
    var v3: vec4<f32>;

    if half_layer_index % 2 == 0 {{
        v2 = points[v1_index + 1];
        v3 = points[v1_index + 1 + PROJECTION_WIDTH];
    }} else {{
        v2 = points[v1_index + PROJECTION_WIDTH];
        v3 = points[v1_index + 1 + PROJECTION_WIDTH];
    }}
    
    // if workgroup_id.x == 0 && workgroup_id.y == 0 && tri_index == 0 {{
    //     debug_out[0] = v1.x;
    //     debug_out[1] = v1.y;
    //     debug_out[2] = v1.z;
    //     debug_out[3] = v2.x;
    //     debug_out[4] = v2.y;
    //     debug_out[5] = v2.z;
    //     debug_out[6] = v3.x;
    //     debug_out[7] = v3.y;
    //     debug_out[8] = v3.z;
    //     // debug_out[0] = f32(v1_index);
    //     // debug_out[1] = f32(half_layer_index);
    //     // debug_out[2] = f32(layer_index);
    //     // debug_out[3] = f32(projection_x);
    // }}

    if v1.w == 0.0 || v2.w == 0.0 || v3.w == 0.0 {{
        return;
    }}

    let bc = barycentric(v1.xyz, v2.xyz, v3.xyz, vec2(heightmap_x, heightmap_y));

    if (bc.x < 0.0 || bc.y < 0.0 || bc.z < 0.0) {{
        // atomicStore(&heightmap[heightmap_index], bitcast<u32>(f32(3.0)));
        return;
    }}

    let new_height = bc.x * v1.y + bc.y * v2.y + bc.z * v3.y;
    let original_height = original_heightmap[heightmap_index];
    let old_height = bitcast<f32>(atomicLoad(&heightmap[heightmap_index]));

    if old_height != original_height {{
        let old_diff = abs(original_height - old_height);
        let new_diff = abs(original_height - new_height);

        if new_diff >= old_diff {{
            return;
        }}
    }}

    atomicStore(&heightmap[heightmap_index], bitcast<u32>(new_height));
}}
"#,
            projection_size.x, heightmap_size.x
        );

        log::trace!("HeightMapShader:\n\n{}", shader);

        let heightmap_shader = HeightMapShader::new(
            wgpu::ShaderModuleDescriptor {
                label: Some("HeightMapShader"),
                source: wgpu::ShaderSource::Wgsl(shader.into()),
            },
            BufferType::new_dyn(cell_count),
            BufferType::new_dyn(point_count as usize),
            BufferType::new_dyn(cell_count),
            // BufferType::new()
        )
        .await?;

        Ok(Self {
            heightmap_shader,
            heightmap_size,
            heightmap: GpuBuffer::<[f32]>::zeroed(cell_count).await?,
            tri_count: 2 * (projection_size.x - 1) * (projection_size.y - 1),
        })
    }

    pub fn call<'a>(
        &'a mut self,
        points: impl BufferSource<[Vector4<f32>]>,
        // debug_output: impl BufferDestination<[f32; 10]> + 'a,
    ) -> impl Future<Output = ()> + 'a {
        self.heightmap_shader
            .new_pass(&self.heightmap, points, &self.heightmap)
            .workgroups_count(self.heightmap_size.x, self.heightmap_size.y, self.tri_count)
            .call(&mut self.heightmap, (), ())
    }

    pub async fn read_heightmap(&mut self) -> ReadGuard<[f32]> {
        self.heightmap.read().await
    }
}
