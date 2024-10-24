use std::num::NonZeroU32;

use gputter::{
    buffers::{
        storage::{
            HostHidden, HostReadOnly, HostReadWrite, ShaderReadOnly, ShaderReadWrite, StorageBuffer,
        },
        uniform::UniformBuffer,
        GpuBufferSet, StaticIndexable,
    },
    shader::BufferGroupBinding,
};
use gputter_macros::build_shader;
build_shader!(
    Test,
    r#"
#[buffer(HostHidden)] var<storage, read_write> heightmap: array<u32, COUNT2>;
#[buffer(HostWriteOnly)] var<uniform> heightmap2: u32;
 
const NUMBER: f32 = {{number}};
const COUNT: NonZeroU32 = {{index}};
const COUNT2: u32 = 4;

@compute
@workgroup_size(1, 1, COUNT)
fn main(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
) {}"#
);

type BindGroupA = (
    UniformBuffer<u32>,
    StorageBuffer<f32, HostReadWrite, ShaderReadOnly>,
);

type BindGroupB = (
    StorageBuffer<f32, HostReadOnly, ShaderReadWrite>,
    StorageBuffer<[u32; 4], HostHidden, ShaderReadWrite>,
);

type BindGroupSet = (GpuBufferSet<BindGroupA>, GpuBufferSet<BindGroupB>);

fn main() {
    let test = Test {
        heightmap: BufferGroupBinding::<_, BindGroupSet>::get::<1, 1>(),
        heightmap2: BufferGroupBinding::<_, BindGroupSet>::get::<0, 0>(),
        number: 2.2,
        index: NonZeroU32::new(1).unwrap(),
    };
    let test = test.compile();
}
