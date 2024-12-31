use gputter::build_shader;

build_shader!(
    pub(crate) ExpandObstacles,
r#"

    #[buffer] var<storage, read_write> obstacles: array<u32>;

    // (x, y) means "the closest obstacle to this position is at (x-1, y-1) 
    // (0, 0) means "don't know where the closest obstacle is"
    #[buffer] var<storage, read_write> closest: array<u32>;
    
    #[buffer] var<storage, read_write> expanded: array<u32>;
    #[buffer] var<uniform> radius: f32;
    
    const GRID_WIDTH: NonZeroU32 = {{grid_width}};
    const GRID_HEIGHT: NonZeroU32 = {{grid_height}};

    @compute
    @workgroup_size(8, 8, 1)
    fn main(@builtin(global_invocation_id) cell: vec3u) {
    
        let i = pos_to_index(cell.xy);
        let pos = index_to_xy(i);
        
        if (obstacles[i] == 1) {
            set_closest(pos, pos + 1); 
        } else {
            var min_dist: f32 = radius;
    
            var dirs = array<vec2i, 4>(
                vec2i( 0,  1),
                vec2i( 0, -1),
                vec2i( 1,  0),
                vec2i(-1,  0),
            );
    
            for (var i = 0; i < 4; i++) {
                let adj = vec2i(pos) + dirs[i];
    
                // if adjacent cell is out of bounds, ignore it
                if (adj.x < 0 || adj.x >= i32(GRID_WIDTH) || adj.y < 0 || adj.y >= i32(GRID_HEIGHT)) { 
                    continue; 
                }
                
                // if the adjacent cell has not calculated its own closest obstacle, ignore it
                let closest_at_adj = get_closest(vec2u(adj));
                if (closest_at_adj.x == 0) {
                    continue;
                }
                
                let dist_to_closest_at_adj = distance(vec2f(pos), vec2f(closest_at_adj - 1));
    
                // if this adjacent cell has the closest obstacle to this position than any other adjacent cell, 
                // make that the closest obstacle to this position
                if (dist_to_closest_at_adj <= min_dist) {
                    min_dist = dist_to_closest_at_adj;
                    set_closest(pos, closest_at_adj);
                }
            }
    
        }
    
    }
    
    fn set_closest(pos: vec2u, value: vec2u) {
        let i = pos_to_index(pos);
        closest[i] = value.x;
        closest[i + GRID_WIDTH * GRID_HEIGHT] = value.y;
        
        expanded[i] = u32(1);
    }
    fn get_closest(pos: vec2u) -> vec2u {
        let i = pos_to_index(pos);
        return vec2u(
            closest[i],
            closest[i + GRID_WIDTH * GRID_HEIGHT],
        );
    }
    
    fn pos_to_index(pos: vec2u) -> u32 {
        return pos.y * GRID_WIDTH + pos.x;
    }
    fn index_to_xy(index: u32) -> vec2u {
        return vec2u(index % GRID_WIDTH, index / GRID_WIDTH);
    } 
    // fn dist(a: vec2u, b: vec2u) -> f32 {
    //     return sqrt(f32(a.x - b.x) * f32(a.x - b.x) + f32(a.y - b.y) * f32(a.y - b.y) );
    // }
"#
);
