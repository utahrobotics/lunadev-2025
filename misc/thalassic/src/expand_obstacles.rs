use gputter::build_shader;

build_shader!(
    pub(crate) ExpandObstacles,
r#"

// `obstacles` array
//
// before this shader runs, obstacles[i] should be: 
//      `0` if empty cell 
//      `1` if obstacle cell
//
// after this shader, obstacles[i] will be: 
//      `0` if empty cell
//      `1` if obstacle cell
//      `2` if distance to the nearest obstacle <= `radius` 
#[buffer] var<storage, read_write> obstacles: array<u32>;

// radius of the robot in meters
#[buffer] var<uniform> radius: f32;

const CELL_SIZE: f32 = {{cell_size}};

const GRID_WIDTH: NonZeroU32 = {{grid_width}};
const GRID_HEIGHT: NonZeroU32 = {{grid_height}};

@compute
@workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) cell: vec3u) {

    let radius_in_cells = radius / CELL_SIZE;

    let radius_ceil = u32(ceil(radius_in_cells));
    let pos = cell.xy;

    if (obstacles[xy_to_index(pos)] == 1) {

        // convert to i32 temporarily to avoid underflowing to maximum u32 values
        let start_x = u32( max( i32(0), i32(pos.x - radius_ceil ) ) );
        let end_x = min(GRID_WIDTH-1, pos.x + radius_ceil);
        
        let start_y = u32( max( i32(0), i32(pos.y - radius_ceil ) ) );
        let end_y = min( GRID_HEIGHT-1 , pos.y + radius_ceil);
    
        for (var x = start_x; x <= end_x; x++) {
            for (var y = start_y; y <= end_y; y++) {

                let nearby_pos = vec2(x, y);
                let nearby_i = xy_to_index(nearby_pos);
    
                // check if this cell is still unmarked before calculating distance
                if (obstacles[nearby_i] == 0 && distance(vec2(f32(x), f32(y)), vec2f(pos)) <= radius_in_cells) {
                    obstacles[nearby_i] = u32(2);
                }
            } 
        }

    }

}


fn xy_to_index(pos: vec2u) -> u32 {
    return pos.y * GRID_WIDTH + pos.x;
}
fn index_to_xy(index: u32) -> vec2u {
    return vec2u(index % GRID_WIDTH, index / GRID_WIDTH);
} 

"#
);
