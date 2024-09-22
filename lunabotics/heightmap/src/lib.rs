
use compute_shader::{buffers::{BufferSource, BufferType, GpuBuffer, HostReadWrite, HostWriteOnly, ReadGuard, ShaderReadOnly, ShaderReadWrite}, wgpu, Compute};
use nalgebra::{Vector2, Vector4};

type HeightMapShader = Compute<(
    // In the shader code, the type is atomic<u32>, but in the shader f32s are bitcasted to u32s so that they can be atomically updated
    BufferType<[f32], HostReadWrite, ShaderReadWrite>,
    BufferType<[Vector4<f32>], HostWriteOnly, ShaderReadOnly>,
    BufferType<[f32], HostWriteOnly, ShaderReadOnly>,
)>;

pub struct HeightMapper {
    heightmap_size: Vector2<u32>,
    heightmap: GpuBuffer<[f32]>,
    heightmap_shader: HeightMapShader,
}

impl HeightMapper {
    pub async fn new(
        heightmap_size: Vector2<u32>,
        cell_size: f32,
        projection_size: Vector2<usize>
    ) -> anyhow::Result<Self> {
        let cell_count = heightmap_size.x * heightmap_size.y;
        let point_count = projection_size.x * projection_size.y;
        let shader = format!(
            r#"
@group(0) @binding(0) var<storage, read_write> heightmap: array<atomic<u32>, {cell_count}>;
@group(0) @binding(1) var<storage, read> points: array<vec4<f32>, {point_count}>;
@group(0) @binding(2) var<storage, read> original_heightmap: array<f32, {cell_count}>;

const PROJECTION_WIDTH: u32 = {};
const HEIGHTMAP_WIDTH: u32 = {};
const CELL_SIZE: f32 = {cell_size};

fn barycentric(v1: vec3<f32>, v2: vec3<f32>, v3: vec3<f32>, p: vec2<f32>) -> vec3<f32> {{
    let u = cross(
        vec3<f32>(v3.x - v1.x, v2.x - v1.x, v1.x - p.x), 
        vec3<f32>(v3.z - v1.z, v2.z - v1.z, v1.z - p.y)
    );
    
    if (abs(u.z) < 1.0) {{
        return vec3<f32>(-1.0, 1.0, 1.0);
    }}
    
    return vec3<f32>(1.0 - (u.x+u.y)/u.z, u.y/u.z, u.x/u.z); 
}}

@compute
@workgroup_size(1, 1, 1)
fn main(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
) {{
    let heightmap_x = f32(workgroup_id.x) * CELL_SIZE;
    let heightmap_y = f32(workgroup_id.y) * CELL_SIZE;
    let heightmap_index = heightmap_y * HEIGHTMAP_WIDTH + heightmap_x;
    let tri_index = workgroup_id.z;
    let half_layer_index = tri_index / (PROJECTION_WIDTH - 1);
    let layer_index = half_layer_index / 2;
    let projection_x = tri_index % (PROJECTION_WIDTH - 1);
    let v1_index = layer_index * PROJECTION_WIDTH + projection_x;
    let v1: vec4<f32>;
    let v2: vec4<f32>;
    let v3: vec4<f32>;
    
    if half_layer_index % 2 == 0 {{
        v1 = points[v1_index];
        v2 = points[v1_index + 1];
        v3 = points[v1_index + 1 + PROJECTION_WIDTH];
    }} else {{
        v1 = points[v1_index];
        v2 = points[v1_index + PROJECTION_WIDTH];
        v3 = points[v1_index + 1 + PROJECTION_WIDTH];
    }}
    
    if v1.w == 0.0 || v2.w == 0.0 || v3.w == 0.0 {{
        return;
    }}
    
    let bc = barycentric(v1.xyz, v2.xyz, v3.xyz, vec2(heightmap_x, heightmap_y));
    
    if (bc.x < 0.0 || bc.y < 0.0 || bc.z < 0.0) {{
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
            BufferType::new_dyn(cell_count as usize),
            BufferType::new_dyn(point_count),
            BufferType::new_dyn(cell_count as usize),
        )
        .await?;

        Ok(Self {
            heightmap_shader,
            heightmap_size,
            heightmap: GpuBuffer::<[f32]>::zeroed(cell_count as usize).await?,
        })
    }
    
    pub async fn process(&mut self, points: impl BufferSource<[Vector4<f32>]>) {
        self.heightmap_shader
            .new_pass(&self.heightmap, points, &self.heightmap)
            .workgroups_count(self.heightmap_size.x, self.heightmap_size.y, 1)
            .call(&mut self.heightmap, (), ())
            .await
    }
    
    pub async fn read_heightmap(&mut self) -> ReadGuard<[f32]> {
        self.heightmap.read().await
    }
}