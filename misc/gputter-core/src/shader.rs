use std::{marker::PhantomData, sync::Arc};

use wgpu::{ShaderModule, SubmissionIndex};

use crate::{
    buffers::{storage::StorageBuffer, uniform::UniformBuffer, GpuBufferSet, GpuBufferTuple},
    tuple::StaticIndexable, types::GpuType,
};

/// A list (tuple) of [`GpuBufferTuple`].
pub trait IndexGpuBufferTupleList<const GRP_IDX: usize, const BIND_IDX: usize> {
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

    pub fn get<const GRP_IDX: usize, const BIND_IDX: usize>() -> Self
    where
        S: IndexGpuBufferTupleList<GRP_IDX, BIND_IDX, Binding = Self>,
    {
        S::get()
    }
}

impl<T: GpuType, S> BufferGroupBinding<UniformBuffer<T>, S> {
    pub const fn unchecked_cast<U: GpuType>(self) -> BufferGroupBinding<UniformBuffer<U>, S> {
        BufferGroupBinding {
            group_index: self.group_index,
            binding_index: self.binding_index,
            phantom: PhantomData,
        }
    }
}

impl<T: GpuType, HM, SM, S> BufferGroupBinding<StorageBuffer<T, HM, SM>, S> {
    pub const fn unchecked_cast<U: GpuType>(self) -> BufferGroupBinding<StorageBuffer<U, HM, SM>, S> {
        BufferGroupBinding {
            group_index: self.group_index,
            binding_index: self.binding_index,
            phantom: PhantomData,
        }
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

impl<S, const I1: usize, const I2: usize> IndexGpuBufferTupleList<I1, I2> for S
where
    S: StaticIndexable<I1>,
    <S as StaticIndexable<I1>>::Output: StaticIndexable<I2>,
{
    type Binding =
        BufferGroupBinding<<<S as StaticIndexable<I1>>::Output as StaticIndexable<I2>>::Output, S>;

    fn get() -> Self::Binding {
        BufferGroupBinding {
            group_index: I1 as u32,
            binding_index: I2 as u32,
            phantom: PhantomData,
        }
    }
}
