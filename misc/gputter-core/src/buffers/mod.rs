use std::num::NonZeroU64;

use wgpu::{util::StagingBelt, CommandEncoder};

use crate::{ get_device, size::BufferSize, types::GpuType, GpuDevice};

pub mod storage;
pub mod uniform;

pub trait GpuBuffer {
    type Data: GpuType + ?Sized;
    /// The type of buffer used to read from this buffer
    type ReadBuffer;
    type Size: BufferSize;

    fn get_buffer(&self) -> &wgpu::Buffer;
    fn make_read_buffer(size: Self::Size, device: &wgpu::Device) -> Self::ReadBuffer;
    fn create_layout(binding: u32) -> wgpu::BindGroupLayoutEntry;
    fn get_size(&self) -> Self::Size;
}

pub struct GpuWriteLock<'a> {
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
    pub(crate) device: &'static wgpu::Device,
}

pub trait WritableGpuBuffer: GpuBuffer
{
    fn write(
        &mut self,
        data: &Self::Data,
        GpuWriteLock { encoder, device }: &mut GpuWriteLock,
        staging_belt: &mut StagingBelt,
    ) {
        let bytes = data.to_bytes();
        staging_belt
            .write_buffer(encoder, self.get_buffer(), 0, NonZeroU64::new(bytes.len() as u64).unwrap(), device)
            .copy_from_slice(bytes);
    }
}

pub trait ReadableGpuBuffer: GpuBuffer {
    fn copy_to_read_buffer(&self, encoder: &mut CommandEncoder, read_buffer: &wgpu::Buffer) {
        encoder.copy_buffer_to_buffer(self.get_buffer(), 0, read_buffer, 0, self.get_size().size());
    }
}

pub trait GpuBufferTuple {
    type BytesSet<'a>;
    type ReadBufferSet;
    type SizeSet: Copy;

    fn make_read_buffers(sizes: Self::SizeSet, device: &wgpu::Device) -> Self::ReadBufferSet;
    fn max_size(sizes: Self::SizeSet) -> u64;
    fn create_layouts() -> Box<[wgpu::BindGroupLayoutEntry]>;
    fn get_size(&self) -> Self::SizeSet;
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

            fn make_read_buffers(sizes: Self::SizeSet, device: &wgpu::Device) -> Self::ReadBufferSet {
                (
                    $(
                        $ty::make_read_buffer(sizes.$index, device),
                    )*
                )
            }

            fn get_size(&self) -> Self::SizeSet {
                (
                    $(
                        self.$index.get_size(),
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

            fn create_layouts() -> Box<[wgpu::BindGroupLayoutEntry]> {
                Box::new([
                    $(
                        $ty::create_layout($index as u32),
                    )*
                ])
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
        // impl<$($ty: GpuBuffer),*> StaticIndexable<$index> for GpuBufferSet<($($ty,)*)> {
        //     type Output = $selected;
        //     fn get(&self) -> &Self::Output {
        //         &self.buffers.$index
        //     }
        // }
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


pub struct GpuBufferSet<S: GpuBufferTuple> {
    pub buffers: S,
    bind_group: wgpu::BindGroup,
    staging_belt: StagingBelt,
    read_buffers: S::ReadBufferSet
}

pub trait ValidGpuBufferSet {
    fn set_into_compute_pass<'a>(&'a self, index: u32, pass: &mut wgpu::ComputePass<'a>);
}

pub trait WriteableGpuBufferInSet<const I: usize> {
    type Data: ?Sized;
    fn write_to(&mut self, data: &Self::Data, lock: &mut GpuWriteLock);
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
                                $ty::create_layout($index),
                            )*
                        ],
                        label: None,
                    }),
                    entries: &[
                        $(
                            wgpu::BindGroupEntry {
                                binding: $index as u32,
                                resource: wgpu::BindingResource::Buffer(buffers.$index.get_buffer().as_entire_buffer_binding()),
                            },
                        )*
                    ],
                    label: None,
                });
                Self {
                    bind_group,
                    read_buffers: <($($ty,)*)>::make_read_buffers(buffers.get_size(), &device),
                    staging_belt: StagingBelt::new(<($($ty,)*)>::max_size(buffers.get_size())),
                    buffers,
                }
            }
        }

        impl<$($ty: GpuBuffer),*> ValidGpuBufferSet for GpuBufferSet<($($ty,)*)> {
            fn set_into_compute_pass<'a>(&'a self, index: u32, pass: &mut wgpu::ComputePass<'a>) {
                pass.set_bind_group(index, &self.bind_group, &[]);
            }
        }
    }
}

set_impl!(1, 0 A);
set_impl!(2, 0 A, 1 B);
set_impl!(3, 0 A, 1 B, 2 C);
set_impl!(4, 0 A, 1 B, 2 C, 3 D);

macro_rules! write_impl {
    ($index: tt $selected: ident, $($ty:ident)+) => {
        impl<$($ty: GpuBuffer),*> WriteableGpuBufferInSet<$index> for GpuBufferSet<($($ty,)*)>
        where
            $selected:WritableGpuBuffer
        {
            type Data = $selected::Data;
            
            fn write_to(&mut self, data: &Self::Data, lock: &mut GpuWriteLock) {
                self.buffers.$index.write(data, lock, &mut self.staging_belt);
            }
        }
    }
}

write_impl!(0 A, A);
write_impl!(0 A, A B);
write_impl!(1 B, A B);

impl<S: GpuBufferTuple> GpuBufferSet<S> {
    pub fn write<const I: usize, T>(&mut self, data: &T, lock: &mut GpuWriteLock)
    where
        Self: WriteableGpuBufferInSet<I, Data = T>
    {
        self.write_to(data, lock);
    }
}