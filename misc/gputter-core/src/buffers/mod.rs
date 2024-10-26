use std::num::NonZeroU64;

use wgpu::{util::StagingBelt, CommandEncoder, Device};

use crate::{get_device, size::BufferSize, types::GpuType, GpuDevice};

pub mod storage;
pub mod uniform;

pub trait GpuBuffer {
    type Data: ?Sized;
    /// The type of buffer used to read from this buffer
    type ReadBuffer;
    type Size: BufferSize;

    fn get_buffer(&self) -> &wgpu::Buffer;
    fn make_read_buffer(size: Self::Size, device: &wgpu::Device) -> Self::ReadBuffer;
    fn create_layout(binding: u32) -> wgpu::BindGroupLayoutEntry;
    fn get_size(&self) -> Self::Size;
}

pub trait WritableGpuBuffer: GpuBuffer {
    fn write_bytes(
        &self,
        data: &[u8],
        encoder: &mut CommandEncoder,
        staging_belt: &mut StagingBelt,
        device: &wgpu::Device,
    ) {
        let len = data.len() as u64;
        let Some(len) = NonZeroU64::new(len) else {
            return;
        };
        staging_belt
            .write_buffer(encoder, self.get_buffer(), 0, len, device)
            .copy_from_slice(data);
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

    // fn write_bytes(
    //     &self,
    //     data: Self::BytesSet<'_>,
    //     encoder: &mut CommandEncoder,
    //     staging_belt: &mut StagingBelt,
    //     device: &wgpu::Device,
    // );
    // fn copy_to_read_buffer(&self, encoder: &mut CommandEncoder, read_buffers: &Self::ReadBufferSet);
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

            // fn write_bytes(&self, data: Self::BytesSet<'_>, encoder: &mut CommandEncoder, staging_belt: &mut StagingBelt, device: &wgpu::Device) {
            //     $(
            //         self.$index.write_bytes(data[$index], encoder, staging_belt, device);
            //     )*
            // }

            // fn copy_to_read_buffer(&self, encoder: &mut CommandEncoder, read_buffers: &Self::ReadBufferSet) {
            //     $(
            //         self.$index.copy_to_read_buffer(encoder, &read_buffers.$index);
            //     )*
            // }

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

pub struct GpuWriteLock<'a> {
    pub(crate) device: &'static Device,
    pub(crate) encoder: &'a mut CommandEncoder,
}

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
    pub fn lock_write<'a>(&'a mut self, lock: GpuWriteLock<'a>) -> LockedGpuWriter<'a, S> {
        LockedGpuWriter { inner: self, lock: Some(lock) }
    }
    // fn copy_to_read_buffer(&self, buffers: &S, encoder: &mut CommandEncoder) {
    //     buffers.copy_to_read_buffer(encoder, &self.read_buffers);
    // }
}

pub struct LockedGpuWriter<'a, S: GpuBufferTuple> {
    inner: &'a mut GpuReaderWriter<S>,
    lock: Option<GpuWriteLock<'a>>,
}

impl<'a, S: GpuBufferTuple> LockedGpuWriter<'a, S> {
    pub fn unlock(mut self) -> GpuWriteLock<'a> {
        self.lock.take().unwrap()
    }
    pub fn write_into<T, B>(&mut self, data: &T, buffer: &B)
    where
        T: GpuType,
        B: WritableGpuBuffer<Data = T>,
    {
        let lock = self.lock.as_mut().unwrap();
        buffer.write_bytes(data.to_bytes(), lock.encoder, &mut self.inner.staging_belt, lock.device);
    }
}

impl<'a, S: GpuBufferTuple> Drop for LockedGpuWriter<'a, S> {
    fn drop(&mut self) {
        self.inner.staging_belt.finish();
    }
}

pub struct GpuBufferSet<S: GpuBufferTuple> {
    pub buffers: S,
    bind_group: wgpu::BindGroup,
}

pub trait ValidGpuBufferSet {
    fn set_into_compute_pass<'a>(&'a self, index: u32, pass: &mut wgpu::ComputePass<'a>);
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
                    buffers,
                    bind_group,
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
