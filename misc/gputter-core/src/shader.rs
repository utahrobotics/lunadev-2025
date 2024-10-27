use std::{marker::PhantomData, sync::Arc};

use wgpu::{ShaderModule, SubmissionIndex};

use crate::{
    buffers::{GpuBufferSet, GpuBufferTuple},
    tuple::StaticIndexable,
};

/// A list (tuple) of [`GpuBufferTuple`].
pub trait IndexGpuBufferTupleList<const GRP_IDX: u32, const BIND_IDX: u32> {
    type Binding;

    fn get() -> Self::Binding;
}

pub trait GpuBufferTupleList {
    fn create_layout_entries() -> Box<[Box<[wgpu::BindGroupLayoutEntry]>]>;
    fn pre_submission(&mut self, encoder: &mut wgpu::CommandEncoder);
    fn post_submission(&mut self, device: &wgpu::Device, idx: SubmissionIndex);
    fn set_into_compute_pass<'a>(&'a self, pass: &mut wgpu::ComputePass<'a>);
}

macro_rules! tuple_impl {
    ($count: literal, $($index: tt $ty:ident),+) => {
        impl<$($ty: GpuBufferTuple),*> GpuBufferTupleList for ($(GpuBufferSet<$ty>,)*)
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
            fn pre_submission(&mut self, encoder: &mut wgpu::CommandEncoder) {
                $(
                    self.$index.pre_submission(encoder);
                )*
            }

            fn post_submission(&mut self, device: &wgpu::Device, idx: SubmissionIndex) {
                let _slices = ($(
                    self.$index.post_submission(),
                )*);
                device.poll(wgpu::Maintain::WaitForSubmissionIndex(idx));
            }
        }
    }
}

macro_rules! tuple_idx_impl {
    ($index1: tt $selected: ident $index2: tt $($ty:ident),+) => {
        impl<$($ty: GpuBufferTuple),*> IndexGpuBufferTupleList<$index1, $index2> for ($(GpuBufferSet<$ty>,)*)
        where
            $selected: StaticIndexable<$index2>
        {
            type Binding = BufferGroupBinding<$selected::Output, Self>;
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

tuple_impl!(1, 0 A);
tuple_impl!(2, 0 A, 1 B);
tuple_impl!(3, 0 A, 1 B, 2 C);
tuple_impl!(4, 0 A, 1 B, 2 C, 3 D);
tuple_impl!(5, 0 A, 1 B, 2 C, 3 D, 4 E);
tuple_impl!(6, 0 A, 1 B, 2 C, 3 D, 4 E, 5 F);

tuple_idx_impl!(0 A 0 A);
tuple_idx_impl!(0 A 1 A);
tuple_idx_impl!(0 A 2 A);
tuple_idx_impl!(0 A 3 A);
tuple_idx_impl!(0 A 4 A);
tuple_idx_impl!(0 A 5 A);

tuple_idx_impl!(0 A 0 A, B);
tuple_idx_impl!(0 A 1 A, B);
tuple_idx_impl!(0 A 2 A, B);
tuple_idx_impl!(0 A 3 A, B);
tuple_idx_impl!(0 A 4 A, B);
tuple_idx_impl!(0 A 5 A, B);
tuple_idx_impl!(1 B 0 A, B);
tuple_idx_impl!(1 B 1 A, B);
tuple_idx_impl!(1 B 2 A, B);
tuple_idx_impl!(1 B 3 A, B);
tuple_idx_impl!(1 B 4 A, B);
tuple_idx_impl!(1 B 5 A, B);

tuple_idx_impl!(0 A 0 A, B, C);
tuple_idx_impl!(0 A 1 A, B, C);
tuple_idx_impl!(0 A 2 A, B, C);
tuple_idx_impl!(0 A 3 A, B, C);
tuple_idx_impl!(0 A 4 A, B, C);
tuple_idx_impl!(0 A 5 A, B, C);
tuple_idx_impl!(1 B 0 A, B, C);
tuple_idx_impl!(1 B 1 A, B, C);
tuple_idx_impl!(1 B 2 A, B, C);
tuple_idx_impl!(1 B 3 A, B, C);
tuple_idx_impl!(1 B 4 A, B, C);
tuple_idx_impl!(1 B 5 A, B, C);
tuple_idx_impl!(2 C 0 A, B, C);
tuple_idx_impl!(2 C 1 A, B, C);
tuple_idx_impl!(2 C 2 A, B, C);
tuple_idx_impl!(2 C 3 A, B, C);
tuple_idx_impl!(2 C 4 A, B, C);
tuple_idx_impl!(2 C 5 A, B, C);
