use std::num::NonZeroU32;

use gputter::{
    buffers::{
        storage::{HostHidden, HostReadOnly, HostReadWrite, ShaderReadWrite, StorageBuffer},
        uniform::UniformBuffer,
        GpuBufferSet,
    },
    compute::ComputePipeline,
    init_gputter_blocking,
    shader::BufferGroupBinding,
    types::AlignedVec2,
};
use gputter_macros::build_shader;
build_shader!(
    Test,
    r#"
#[buffer(HostHidden)] var<storage, read_write> heightmap: array<vec2<f32>, COUNT2>;
const COUNT: NonZeroU32 = {{index}};
#[buffer(HostReadWrite)] var<storage, read_write> counter: u32;
 
const NUMBER: f32 = {{number}};
const COUNT2: u32 = 4;

@compute
@workgroup_size(1, 1, COUNT)
fn main(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
) {
    var local = counter;
    while true {
        local += 1u;
        var failed = false;
        for (var i = 2u; i < local; i++) {
            if local % i == 0u {
                failed = true;
                break;
            }
        }
        if !failed {
            break;
        }
    }
    counter = local;
}"#
);

type BindGroupA = (
    UniformBuffer<u32>,
    StorageBuffer<u32, HostReadWrite, ShaderReadWrite>,
);

type BindGroupB = (
    StorageBuffer<f32, HostReadOnly, ShaderReadWrite>,
    StorageBuffer<[AlignedVec2<f32>; 4], HostHidden, ShaderReadWrite>,
);

type BindGroupSet = (GpuBufferSet<BindGroupA>, GpuBufferSet<BindGroupB>);

fn main() {
    init_gputter_blocking().unwrap();
    let test = Test {
        heightmap: BufferGroupBinding::<_, BindGroupSet>::get::<1, 1>(),
        counter: BufferGroupBinding::<_, BindGroupSet>::get::<0, 1>(),
        number: 2.2,
        index: NonZeroU32::new(1).unwrap(),
    };
    let [main_fn] = test.compile();
    let pipeline = ComputePipeline::new([&main_fn]);
    let mut bind_grps = (
        GpuBufferSet::from((UniformBuffer::new(), StorageBuffer::new())),
        GpuBufferSet::from((StorageBuffer::new(), StorageBuffer::new())),
    );
    for i in 0..10 {
        pipeline
            .new_pass(|mut lock| {
                if i == 0 {
                    bind_grps.0.write::<1, _>(&1146643, &mut lock);
                }
                &mut bind_grps
            })
            .finish();
        let mut counter = 0u32;
        bind_grps.0.buffers.1.read(&mut counter);
        println!("Counter: {}", counter);
    }
}
