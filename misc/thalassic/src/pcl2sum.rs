use gputter::build_shader;

build_shader!(
    pub(crate) Pcl2Sum,
    r#"
    #[buffer] var<storage, read_write> sum: array<vec2f, CELL_COUNT>;
    #[buffer] var<storage, read_write> points: array<vec4f, MAX_POINT_COUNT>;
    
    const HEIGHTMAP_WIDTH: NonZeroU32 = {{heightmap_width}};
    const CELL_SIZE: f32 = {{cell_size}};
    const CELL_COUNT: NonZeroU32 = {{cell_count}};
    const MAX_POINT_COUNT: NonZeroU32 = {{max_point_count}}; /!/ sub with 32
    
    @compute
    @workgroup_size(8, 8, 1)
    fn height(
        @builtin(global_invocation_id) global_invocation_id : vec3u,
    ) {
        let heightmap_point = vec2(f32(global_invocation_id.x), f32(global_invocation_id.y)) * CELL_SIZE;
        let heightmap_index = global_invocation_id.x + global_invocation_id.y * HEIGHTMAP_WIDTH;

        for (var i = 0u; i < MAX_POINT_COUNT; i++) {
            let point = points[i];
            if (point.w == 0.0) {
                continue;
            }
            if (abs(heightmap_point.x - point.x) > CELL_SIZE) {
                continue;
            }
            if (abs(heightmap_point.y - point.z) > CELL_SIZE) {
                continue;
            }
            sum[heightmap_index].x += 1.0;
            sum[heightmap_index].y += point.y;
        }
    }
"#
);
