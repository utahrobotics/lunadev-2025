use std::num::NonZeroU64;

use wgpu::{util::StagingBelt, CommandEncoder};

use crate::{get_device, types::GpuType, GpuDevice};

pub mod storage;
pub mod uniform;

pub trait GpuBuffer {
    type Data: GpuType + ?Sized;

    fn get_buffer(&self) -> &wgpu::Buffer;
    fn pre_submission(&self, encoder: &mut CommandEncoder);
    fn post_submission(&self);
    fn create_layout(binding: u32) -> wgpu::BindGroupLayoutEntry;
    fn get_writable_size(&self) -> u64;
}

pub struct GpuWriteLock<'a> {
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
    pub(crate) device: &'static wgpu::Device,
}

pub trait WritableGpuBuffer: GpuBuffer {
    fn write(
        &mut self,
        data: &Self::Data,
        GpuWriteLock { encoder, device }: &mut GpuWriteLock,
        staging_belt: &mut StagingBelt,
    ) {
        let bytes = data.to_bytes();
        staging_belt
            .write_buffer(
                encoder,
                self.get_buffer(),
                0,
                NonZeroU64::new(bytes.len() as u64).unwrap(),
                device,
            )
            .copy_from_slice(bytes);
    }
}

pub struct GpuReadLock<'a> {
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
    pub(crate) device: &'static wgpu::Device,
}

pub trait GpuBufferTuple {
    fn create_layouts() -> Box<[wgpu::BindGroupLayoutEntry]>;
    fn get_max_writable_size(&self) -> u64;
    fn pre_submission(&self, encoder: &mut CommandEncoder);
    fn post_submission(&self);
}

macro_rules! tuple_impl {
    ($count: literal, $($index: tt $ty:ident),+) => {
        impl<$($ty: GpuBuffer),*> GpuBufferTuple for ($($ty,)*) {
            fn create_layouts() -> Box<[wgpu::BindGroupLayoutEntry]> {
                Box::new([
                    $(
                        $ty::create_layout($index as u32),
                    )*
                ])
            }

            fn get_max_writable_size(&self) -> u64 {
                let mut max = 0;
                $(
                    max = max.max(self.$index.get_writable_size());
                )*
                max
            }

            fn pre_submission(&self, encoder: &mut CommandEncoder) {
                $(
                    self.$index.pre_submission(encoder);
                )*
            }

            fn post_submission(&self) {
                $(
                    self.$index.post_submission();
                )*
            }
        }
    }
}

tuple_impl!(1, 0 A);
tuple_impl!(2, 0 A, 1 B);
tuple_impl!(3, 0 A, 1 B, 2 C);
tuple_impl!(4, 0 A, 1 B, 2 C, 3 D);

pub struct GpuBufferSet<S: GpuBufferTuple> {
    pub buffers: S,
    bind_group: wgpu::BindGroup,
    staging_belt: StagingBelt,
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
                    staging_belt: StagingBelt::new(buffers.get_max_writable_size()),
                    buffers,
                }
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
        Self: WriteableGpuBufferInSet<I, Data = T>,
    {
        self.write_to(data, lock);
    }
    pub(crate) fn set_into_compute_pass<'a>(
        &'a self,
        index: u32,
        pass: &mut wgpu::ComputePass<'a>,
    ) {
        pass.set_bind_group(index, &self.bind_group, &[]);
    }
    pub(crate) fn pre_submission(&mut self, encoder: &mut CommandEncoder) {
        self.staging_belt.finish();
        self.buffers.pre_submission(encoder);
    }
}
