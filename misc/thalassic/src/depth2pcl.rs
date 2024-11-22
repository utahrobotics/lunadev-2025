use gputter::build_shader;

build_shader!(
    pub(crate) Depth2Pcl,
    r#"
#[buffer(HostWriteOnly)] var<storage, read> depths: array<u32, HALF_PIXEL_COUNT>;
#[buffer(HostReadOnly)] var<storage, read_write> points: array<vec4f, PIXEL_COUNT>;
#[buffer(HostWriteOnly)] var<uniform> transform: mat4x4f;
#[buffer(HostWriteOnly)] var<uniform> depth_scale: f32;

const IMAGE_WIDTH: NonZeroU32 = {{image_width}};
const FOCAL_LENGTH_PX: f32 = {{focal_length_px}};
const PRINCIPAL_POINT_PX: vec2f = {{principal_point_px}};
const PIXEL_COUNT: NonZeroU32 = {{pixel_count}};
const HALF_PIXEL_COUNT: NonZeroU32 = {{half_pixel_count}};

@compute
@workgroup_size(1, 1, 1)
fn main(
    @builtin(workgroup_id) workgroup_id : vec3u,
) {
    let i = workgroup_id.x + workgroup_id.y * IMAGE_WIDTH;
    let double_depth = depths[i / 2];
    var depthu: u32;
    if i % 2 == 1 {
        depthu = double_depth >> 16;
    } else {
        depthu = double_depth & 0xFFFF;
    }

    if depthu == 0 {
        points[i].w = 0.0;
        return;
    }

    let depth = f32(depthu) * depth_scale;
    let x = (f32(workgroup_id.x) - PRINCIPAL_POINT_PX.x) / FOCAL_LENGTH_PX;
    let y = (f32(workgroup_id.y) - PRINCIPAL_POINT_PX.y) / FOCAL_LENGTH_PX;

    let point = normalize(vec3(x, y, -1)) * depth;
    var point_transformed = transform * vec4<f32>(point, 1.0);
    point_transformed.w = 1.0;
    points[i] = point_transformed;
}
"#
);
