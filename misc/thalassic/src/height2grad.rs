use gputter::build_shader;

// Generates a list of gradient magnitudes from a given heightmap
build_shader!(
    pub(crate) Height2Grad,
    r#"
    const HEIGHTMAP_WIDTH: NonZeroU32 = {{heightmap_width}};
    const CELL_SIZE: f32 = {{cell_size}};
    const CELL_COUNT: NonZeroU32 = {{cell_count}};
    const PI: f32 = 3.141592653589793;

    // Shader is read_write as it is written to in another shader
    #[buffer(HostHidden)] var<storage, read_write> heightmap: array<f32, CELL_COUNT>;
    #[buffer(HostReadOnly)] var<storage, read_write> gradient_map: array<f32, CELL_COUNT>;

    @compute
    @workgroup_size(1, 1, 1)
    fn grad(
        @builtin(workgroup_id) workgroup_id : vec3u,
    ) {
        var minHeight = heightmap[workgroup_id.y * HEIGHTMAP_WIDTH + workgroup_id.x];
        var minCoords = vec2f(0, 0);
        var maxHeight = heightmap[workgroup_id.y * HEIGHTMAP_WIDTH + workgroup_id.x];
        var maxCoords = vec2f(0, 0);

        for (var y = 0u; y < 3u; y++) {
            for (var x = 0u; x < 3u; x++) {
                let height = heightmap[(workgroup_id.y + y) * HEIGHTMAP_WIDTH + workgroup_id.x + x];

                if (height < minHeight) {
                    minHeight = height;
                    minCoords = vec2(f32(x), f32(y));
                } else if (height > maxHeight) {
                    maxHeight = height;
                    maxCoords = vec2(f32(x), f32(y));
                }
            }
        }

        let dx = length(maxCoords - minCoords) * CELL_SIZE;
        if (dx == 0.0) {
            gradient_map[(workgroup_id.y + 1) * HEIGHTMAP_WIDTH + workgroup_id.x + 1] = 0.0;
            return;
        }
        let dy = maxHeight - minHeight;
        gradient_map[(workgroup_id.y + 1) * HEIGHTMAP_WIDTH + workgroup_id.x + 1] = atan(dy / dx);
    }
    "#
);
