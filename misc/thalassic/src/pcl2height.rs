use gputter::build_shader;

build_shader!(
    pub(crate) Pcl2Height,
    r#"
    #[buffer(HostReadOnly)] var<storage, read_write> heightmap: array<atomic<u32>, CELL_COUNT>;
    #[buffer(HostReadOnly)] var<storage, read_write> points: array<vec4f, POINT_COUNT>;
    #[buffer(HostWriteOnly)] var<storage, read> original_heightmap: array<f32, CELL_COUNT>;
    
    const PROJECTION_WIDTH: NonZeroU32 = {{projection_width}}; /!/ sub with 12
    const HEIGHTMAP_WIDTH: NonZeroU32 = {{heightmap_width}};
    const CELL_SIZE: f32 = {{cell_size}};
    const CELL_COUNT: NonZeroU32 = {{cell_count}};
    const POINT_COUNT: NonZeroU32 = {{point_count}}; /!/ sub with 32
    
    fn barycentric(pv1: vec3f, pv2: vec3f, pv3: vec3f, pp: vec2f) -> vec3f {
        let v0 = vec3f(pv1.x, pv1.z, 0.0);
        let v1 = vec3f(pv2.x, pv2.z, 0.0);
        let v2 = vec3f(pv3.x, pv3.z, 0.0);
        let p = vec3f(pp.x, pp.y, 0.0);
    
        let d00 = dot(v0 - v2, v0 - v2);
        let d01 = dot(v0 - v2, v1 - v2);
        let d11 = dot(v1 - v2, v1 - v2);
        let d20 = dot(p - v2, v0 - v2);
        let d21 = dot(p - v2, v1 - v2);
    
        let invDenom = 1.0 / (d00 * d11 - d01 * d01);
        let v = (d11 * d20 - d01 * d21) * invDenom;
        let w = (d00 * d21 - d01 * d20) * invDenom;
        let u = 1.0 - v - w;
    
        return vec3f(u, v, w);
    }
    
    @compute
    @workgroup_size(1, 1, 1)
    fn main(
        @builtin(workgroup_id) workgroup_id : vec3u,
    ) {
        let heightmap_x = f32(workgroup_id.x) * CELL_SIZE;
        let heightmap_y = f32(workgroup_id.y) * CELL_SIZE;
        let heightmap_index = workgroup_id.y * HEIGHTMAP_WIDTH + workgroup_id.x;
        let tri_index = workgroup_id.z;
        let half_layer_index = tri_index / (PROJECTION_WIDTH - 1);
        let layer_index = half_layer_index / 2;
        let projection_x = tri_index % (PROJECTION_WIDTH - 1);
        let v1_index = layer_index * PROJECTION_WIDTH + projection_x;
        let v1 = points[v1_index];
        var v2: vec4f;
        var v3: vec4f;
    
        if half_layer_index % 2 == 0 {
            v2 = points[v1_index + 1];
            v3 = points[v1_index + 1 + PROJECTION_WIDTH];
        } else {
            v2 = points[v1_index + PROJECTION_WIDTH];
            v3 = points[v1_index + 1 + PROJECTION_WIDTH];
        }
        
        if v1.w == 0.0 || v2.w == 0.0 || v3.w == 0.0 {
            return;
        }
    
        let bc = barycentric(v1.xyz, v2.xyz, v3.xyz, vec2(heightmap_x, heightmap_y));
    
        if (bc.x < 0.0 || bc.y < 0.0 || bc.z < 0.0) {
            return;
        }
    
        let new_height = bc.x * v1.y + bc.y * v2.y + bc.z * v3.y;
        let original_height = original_heightmap[heightmap_index];
        let old_height = bitcast<f32>(atomicLoad(&heightmap[heightmap_index]));
    
        if old_height != original_height {
            let old_diff = abs(original_height - old_height);
            let new_diff = abs(original_height - new_height);
    
            if new_diff >= old_diff {
                return;
            }
        }
    
        atomicStore(&heightmap[heightmap_index], bitcast<u32>(new_height));
    }
"#
);
