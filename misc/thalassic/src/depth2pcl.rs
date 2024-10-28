use gputter::build_shader;

build_shader!(
    pub(crate) Depth2Pcl,
    r#"
#[buffer(HostWriteOnly)] var<storage, read> depths: array<u32, PIXEL_COUNT>;
#[buffer(HostReadOnly)] var<storage, read_write> points: array<vec4f, PIXEL_COUNT>;
#[buffer(HostWriteOnly)] var<uniform> transform: mat4x4f;

const IMAGE_WIDTH: NonZeroU32 = {{image_width}};
const FOCAL_LENGTH_PX: f32 = {{focal_length_px}};
const PRINCIPAL_POINT_PX: vec2f = {{principal_point_px}};
const DEPTH_SCALE: f32 = {{depth_scale}};
const PIXEL_COUNT: NonZeroU32 = {{pixel_count}};

@compute
@workgroup_size(IMAGE_WIDTH, PIXEL_COUNT / IMAGE_WIDTH, 1)
fn main(
    @builtin(local_invocation_id) local_invocation_id : vec3u,
) {
    let i = local_invocation_id.x + local_invocation_id.y * IMAGE_WIDTH;

    if depths[i] == 0 {
        points[i].w = 0.0;
        return;
    }

    let depth = f32(depths[i]) * DEPTH_SCALE;
    let x = (f32(local_invocation_id.x) - PRINCIPAL_POINT_PX.x) / FOCAL_LENGTH_PX;
    let y = (f32(local_invocation_id.y) - PRINCIPAL_POINT_PX.y) / FOCAL_LENGTH_PX;

    let point = normalize(vec3(x, y, -1)) * depth;
    var point_transformed = transform * vec4<f32>(point, 1.0);
    point_transformed.w = 1.0;
    points[i] = point_transformed;
}
"#
);
