use std::marker::PhantomData;

use wgpu::ShaderModule;

use crate::buffers::{GpuBufferSet, GpuBufferTuple, StaticIndexable};

/// A list (tuple) of [`GpuBufferTuple`].
pub trait GpuBufferTupleList<const GRP_IDX: u32, const BIND_IDX: u32> {
    type Binding;

    fn get() -> Self::Binding;
}

macro_rules! tuple_idx_impl {
    ($index1: tt $index2: tt $($ty:ident),+) => {
        impl<$($ty: GpuBufferTuple),*> GpuBufferTupleList<$index1, $index2> for ($(GpuBufferSet<$ty>,)*)
        where
            <Self as StaticIndexable<$index1>>::Output: StaticIndexable<$index2>
        {
            type Binding = BufferGroupBinding<<<Self as StaticIndexable<$index1>>::Output as StaticIndexable<$index2>>::Output, Self>;
            // type Output = ;
            fn get() -> Self::Binding {
                BufferGroupBinding {
                    group_index: $index1,
                    binding_index: $index2,
                    phantom: PhantomData
                }
            }
        }
    }
}

tuple_idx_impl!(0 0 A);
tuple_idx_impl!(0 1 A);
tuple_idx_impl!(0 2 A);
tuple_idx_impl!(0 3 A);

tuple_idx_impl!(0 0 A, B);
tuple_idx_impl!(0 1 A, B);
tuple_idx_impl!(0 2 A, B);
tuple_idx_impl!(0 3 A, B);
tuple_idx_impl!(1 0 A, B);
tuple_idx_impl!(1 1 A, B);
tuple_idx_impl!(1 2 A, B);
tuple_idx_impl!(1 3 A, B);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferGroupBinding<B, S> {
    group_index: u32,
    binding_index: u32,
    phantom: PhantomData<fn() -> (B, S)>,
}

impl<B, S> BufferGroupBinding<B, S> {
    pub const fn new_unchecked(group_index: u32, binding_index: u32) -> Self {
        Self {
            group_index,
            binding_index,
            phantom: PhantomData,
        }
    }

    pub fn get<const GRP_IDX: u32, const BIND_IDX: u32>() -> Self
    where
        S: GpuBufferTupleList<GRP_IDX, BIND_IDX, Binding = Self>,
    {
        S::get()
    }
}

impl<B, S> std::fmt::Display for BufferGroupBinding<B, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "@group({}) @binding({}) ",
            self.group_index, self.binding_index
        )
    }
}

pub struct CompiledShader<S> {
    shader: ShaderModule,
    phantom: PhantomData<fn() -> S>,
}

impl<S> From<ShaderModule> for CompiledShader<S> {
    fn from(shader: ShaderModule) -> Self {
        Self {
            shader,
            phantom: PhantomData,
        }
    }
}
