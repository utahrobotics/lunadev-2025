use gputter::build_shader;

// Generates a list of gradient magnitudes from a given heightmap
build_shader!(
    pub(crate) Grad2Obstacle,
    r#"
    const HEIGHTMAP_WIDTH: NonZeroU32 = {{heightmap_width}};
    const CELL_COUNT: NonZeroU32 = {{cell_count}};

    // Shader is read_write as it is written to in another shader
    #[buffer] var<storage, read_write> obstacle_map: array<u32, CELL_COUNT>;
    #[buffer] var<storage, read_write> gradient_map: array<f32, CELL_COUNT>;
    #[buffer] var<uniform> max_gradient: f32;

    @compute
    @workgroup_size(8, 8, 1)
    fn grad(
        @builtin(global_invocation_id) global_invocation_id : vec3u,
    ) {
        let index = global_invocation_id.y * HEIGHTMAP_WIDTH + global_invocation_id.x;
        if (gradient_map[index] > max_gradient) {
            obstacle_map[index] = 1u;
        } else {
            obstacle_map[index] = 0u;
        }
    }
    "#
);
