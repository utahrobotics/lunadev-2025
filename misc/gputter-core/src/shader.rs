use std::{marker::PhantomData, sync::Arc};

use wgpu::ShaderModule;

use crate::buffers::{GpuBufferSet, GpuBufferTuple, StaticIndexable, ValidGpuBufferSet};

/// A list (tuple) of [`GpuBufferTuple`].
pub trait IndexGpuBufferTupleList<const GRP_IDX: u32, const BIND_IDX: u32> {
    type Binding;

    fn get() -> Self::Binding;
}

pub trait GpuBufferTupleList {
    fn create_layout_entries() -> Box<[Box<[wgpu::BindGroupLayoutEntry]>]>;
    fn set_into_compute_pass<'a>(&'a self, pass: &mut wgpu::ComputePass<'a>);
}


macro_rules! tuple_impl {
    ($count: literal, $($index: tt $ty:ident),+) => {
        impl<$($ty: GpuBufferTuple),*> GpuBufferTupleList for ($(GpuBufferSet<$ty>,)*)
        where
        $(GpuBufferSet<$ty>: ValidGpuBufferSet,)*
        {
            fn create_layout_entries() -> Box<[Box<[wgpu::BindGroupLayoutEntry]>]> {
                Box::new(
                    [
                        $(
                            $ty::create_layouts(),
                        )*
                    ]
                )
            }
            fn set_into_compute_pass<'a>(&'a self, pass: &mut wgpu::ComputePass<'a>) {
                $(
                    self.$index.set_into_compute_pass($index, pass);
                )*
            }
        }
    }
}

tuple_impl!(1, 0 A);
tuple_impl!(2, 0 A, 1 B);
tuple_impl!(3, 0 A, 1 B, 2 C);
tuple_impl!(4, 0 A, 1 B, 2 C, 3 D);

macro_rules! tuple_idx_impl {
    ($index1: tt $index2: tt $($ty:ident),+) => {
        impl<$($ty: GpuBufferTuple),*> IndexGpuBufferTupleList<$index1, $index2> for ($(GpuBufferSet<$ty>,)*)
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
        S: IndexGpuBufferTupleList<GRP_IDX, BIND_IDX, Binding = Self>,
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

// pub struct CompiledShader<S> {
//     shader: ShaderModule,
//     phantom: PhantomData<fn() -> S>,
// }

// impl<S> From<ShaderModule> for CompiledShader<S> {
//     fn from(shader: ShaderModule) -> Self {
//         Self {
//             shader,
//             phantom: PhantomData,
//         }
//     }
// }

pub struct ComputeFn<S> {
    pub(crate) shader: Arc<ShaderModule>,
    pub(crate) name: &'static str,
    phantom: PhantomData<fn() -> S>,
}

impl<S> Clone for ComputeFn<S> {
    fn clone(&self) -> Self {
        Self {
            shader: self.shader.clone(),
            name: self.name,
            phantom: PhantomData,
        }
    }
}

impl<S> std::fmt::Debug for ComputeFn<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComputeFn")
            .field("shader", &self.shader)
            .field("name", &self.name)
            .finish()
    }
}

impl<S> ComputeFn<S> {
    pub fn new_unchecked(shader: Arc<ShaderModule>, name: &'static str) -> Self {
        Self {
            shader,
            name,
            phantom: PhantomData,
        }
    }

    pub fn assert_name(&self, name: &'static str) {
        assert_eq!(name, self.name);
    }
}