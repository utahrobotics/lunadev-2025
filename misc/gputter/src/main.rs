use std::num::NonZeroU32;

use gputter::buffers::{Index, StaticIndexable};
use gputter_macros::build_shader;
build_shader!(
    Test,
    r#"
#[buffer] var<storage, read_write> heightmap: u32;
 
const NUMBER: f32 = {{number}};
const COUNT: NonZeroU32 = {{index}};

@compute
@workgroup_size(1, 1, COUNT)
fn main(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
) {}"#
);

fn main() {
    let tuple = (false, 0u32, -2i32);
    let a = StaticIndexable::<Index<0>>::get(&tuple);
    let b = StaticIndexable::<Index<1>>::get(&tuple);
    let c = StaticIndexable::<Index<2>>::get(&tuple);
    // let test = Test {
    //     heightmap: todo!(),
    //     points: todo!(),
    //     original_heightmap: todo!(),
    //     number: 0.2,
    //     index: NonZeroU32::new(3).unwrap(),
    // };
    // test.compile();
}
