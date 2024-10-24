use wgpu::{util::StagingBelt, CommandEncoder};

use crate::{get_device, size::BufferSize, GpuDevice};

pub mod storage;
pub mod uniform;

pub trait GpuBuffer {
    type Data: ?Sized;
    /// The type of buffer used to read from this buffer
    type ReadBuffer;
    type Size: BufferSize;

    fn get_entire_binding(&self) -> wgpu::BufferBinding;
    fn write_bytes(
        &self,
        data: &[u8],
        encoder: &mut CommandEncoder,
        staging_belt: &mut StagingBelt,
        device: &wgpu::Device,
    );
    fn copy_to_read_buffer(&self, encoder: &mut CommandEncoder, read_buffer: &Self::ReadBuffer);
    fn make_read_buffer(size: Self::Size, device: &wgpu::Device) -> Self::ReadBuffer;
    fn into_layout(&self, binding: u32) -> wgpu::BindGroupLayoutEntry;
}

pub trait GpuBufferTuple {
    type BytesSet<'a>;
    type ReadBufferSet;
    type SizeSet: Copy;

    fn write_bytes(
        &self,
        data: Self::BytesSet<'_>,
        encoder: &mut CommandEncoder,
        staging_belt: &mut StagingBelt,
        device: &wgpu::Device,
    );
    fn copy_to_read_buffer(&self, encoder: &mut CommandEncoder, read_buffers: &Self::ReadBufferSet);
    fn make_read_buffers(sizes: Self::SizeSet, device: &wgpu::Device) -> Self::ReadBufferSet;
    fn max_size(sizes: Self::SizeSet) -> u64;
}

pub trait StaticIndexable<const I: usize> {
    type Output;
    fn get(&self) -> &Self::Output;
}

macro_rules! tuple_impl {
    ($count: literal, $($index: tt $ty:ident),+) => {
        impl<$($ty: GpuBuffer),*> GpuBufferTuple for ($($ty,)*) {
            type BytesSet<'a> = [&'a [u8]; $count];
            type ReadBufferSet = ($($ty::ReadBuffer,)*);
            type SizeSet = ($($ty::Size,)*);

            fn write_bytes(&self, data: Self::BytesSet<'_>, encoder: &mut CommandEncoder, staging_belt: &mut StagingBelt, device: &wgpu::Device) {
                $(
                    self.$index.write_bytes(data[$index], encoder, staging_belt, device);
                )*
            }

            fn copy_to_read_buffer(&self, encoder: &mut CommandEncoder, read_buffers: &Self::ReadBufferSet) {
                $(
                    self.$index.copy_to_read_buffer(encoder, &read_buffers.$index);
                )*
            }

            fn make_read_buffers(sizes: Self::SizeSet, device: &wgpu::Device) -> Self::ReadBufferSet {
                (
                    $(
                        $ty::make_read_buffer(sizes.$index, device),
                    )*
                )
            }

            fn max_size(sizes: Self::SizeSet) -> u64 {
                let mut max = 0;
                $(
                    max = max.max(sizes.$index.size());
                )*
                max
            }
        }
    }
}

tuple_impl!(1, 0 A);
tuple_impl!(2, 0 A, 1 B);
tuple_impl!(3, 0 A, 1 B, 2 C);
tuple_impl!(4, 0 A, 1 B, 2 C, 3 D);

macro_rules! tuple_idx_impl {
    ($index: tt $selected: ident $($ty:ident),+) => {
        impl<$($ty),*> StaticIndexable<$index> for ($($ty,)*) {
            type Output = $selected;
            fn get(&self) -> &Self::Output {
                &self.$index
            }
        }
        impl<$($ty: GpuBuffer),*> StaticIndexable<$index> for GpuBufferSet<($($ty,)*)> {
            type Output = $selected;
            fn get(&self) -> &Self::Output {
                &self.buffers.$index
            }
        }
    }
}

tuple_idx_impl!(0 A A);

tuple_idx_impl!(0 A A, B);
tuple_idx_impl!(1 B A, B);

tuple_idx_impl!(0 A A, B, C);
tuple_idx_impl!(1 B A, B, C);
tuple_idx_impl!(2 C A, B, C);

tuple_idx_impl!(0 A A, B, C, D);
tuple_idx_impl!(1 B A, B, C, D);
tuple_idx_impl!(2 C A, B, C, D);
tuple_idx_impl!(3 D A, B, C, D);

pub struct GpuReaderWriter<S: GpuBufferTuple> {
    staging_belt: StagingBelt,
    read_buffers: S::ReadBufferSet,
}

impl<S: GpuBufferTuple> GpuReaderWriter<S> {
    pub fn new(sizes: S::SizeSet) -> Self {
        let GpuDevice { device, .. } = get_device();
        let read_buffers = S::make_read_buffers(sizes, device);
        Self {
            staging_belt: StagingBelt::new(S::max_size(sizes)),
            read_buffers,
        }
    }
    fn write_bytes(
        &mut self,
        buffers: &S,
        data: S::BytesSet<'_>,
        encoder: &mut CommandEncoder,
        device: &wgpu::Device,
    ) {
        buffers.write_bytes(data, encoder, &mut self.staging_belt, device);
    }
    fn copy_to_read_buffer(&self, buffers: &S, encoder: &mut CommandEncoder) {
        buffers.copy_to_read_buffer(encoder, &self.read_buffers);
    }
}

pub struct GpuBufferSet<S: GpuBufferTuple> {
    pub(crate) buffers: S,
    bind_group: wgpu::BindGroup,
}

macro_rules! set_impl {
    ($count: literal, $($index: tt $ty:ident),+) => {
        impl<$($ty: GpuBuffer),*> From<($($ty,)*)> for GpuBufferSet<($($ty,)*)> {
            fn from(buffers: ($($ty,)*)) -> Self {
                let GpuDevice { device, .. } = get_device();
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    layout: &device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        entries: &[
                            $(
                                buffers.$index.into_layout($index),
                            )*
                        ],
                        label: None,
                    }),
                    entries: &[
                        $(
                            wgpu::BindGroupEntry {
                                binding: $index as u32,
                                resource: wgpu::BindingResource::Buffer(buffers.$index.get_entire_binding()),
                            },
                        )*
                    ],
                    label: None,
                });
                Self {
                    buffers,
                    bind_group,
                }
            }
        }
    }
}

set_impl!(1, 0 A);
set_impl!(2, 0 A, 1 B);
set_impl!(3, 0 A, 1 B, 2 C);
set_impl!(4, 0 A, 1 B, 2 C, 3 D);
