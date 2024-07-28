@group(0) @binding(0) var<storage, read_write> returned: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> rays: array<vec3<f32>>;
@group(0) @binding(2) var<storage, read> depths: array<u32>;
@group(0) @binding(3) var<storage, read> min_depth: f32;
@group(0) @binding(4) var<storage, read> depth_scale: f32;

@compute
@workgroup_size(2, 1, 1)
fn main(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
    @builtin(local_invocation_id) local_invocation_id : vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>
) {
    let double_depth = depths[workgroup_id.x + workgroup_id.y * num_workgroups.x];
    var depth = 0.0;
    if local_invocation_id.x == 0 {
        depth = f32(double_depth & 0xFFFF);
    } else {
        depth = f32(double_depth >> 16);
    }
    let index = workgroup_id.x * 2 + workgroup_id.y * num_workgroups.x * 2 + local_invocation_id.x;
    depth *= depth_scale;
    if depth < min_depth {
        returned[index].w = 0.0;
        return;
    }
    let point = rays[index] * depth;
    returned[index].x = point.x;
    returned[index].y = point.y;
    returned[index].z = point.z;
    returned[index].w = 1.0;
}