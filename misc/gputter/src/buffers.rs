use std::{marker::PhantomData, num::NonZeroU64};

use wgpu::{util::StagingBelt, CommandEncoder};

use crate::{get_device, size::{BufferSize, DynamicSize, StaticSize}, types::GpuType, GpuDevice};

pub trait GpuBuffer {
    type Data: ?Sized;
    /// The type of buffer used to read from this buffer
    type ReadBuffer;
    type Size: BufferSize;

    fn write_bytes(&self, data: &[u8], encoder: &mut CommandEncoder, staging_belt: &mut StagingBelt, device: &wgpu::Device);
    fn copy_to_read_buffer(&self, encoder: &mut CommandEncoder, read_buffer: &Self::ReadBuffer);
    fn make_read_buffer(size: Self::Size, device: &wgpu::Device) -> Self::ReadBuffer;
}

/// Uniform Buffers can only be read from shaders, and written to by the host.
pub struct UniformBuffer<T: ?Sized> {
    buffer: wgpu::Buffer,
    phantom: PhantomData<T>,
}

impl<T: GpuType + ?Sized> GpuBuffer for UniformBuffer<T> {
    type Data = T;
    type ReadBuffer = ();
    type Size = T::Size;

    fn write_bytes(&self, data: &[u8], encoder: &mut CommandEncoder, staging_belt: &mut StagingBelt, device: &wgpu::Device) {
        let len = data.len() as u64;
        let Some(len) = NonZeroU64::new(len) else { return; };
        staging_belt.write_buffer(
            encoder,
            &self.buffer,
            0,
            len,
            device
        ).copy_from_slice(data);
    }
    fn copy_to_read_buffer(&self, _encoder: &mut CommandEncoder, _read_buffer: &Self::ReadBuffer) {}
    fn make_read_buffer(_size: Self::Size, _device: &wgpu::Device) -> Self::ReadBuffer {
        ()
    }
}

impl<T> UniformBuffer<T> {
    pub fn new() -> Self {
        const {
            // If this assertion fails, the size of T
            // is too large to be used in a uniform buffer.
            assert!(size_of::<T>() < 65536);
        }
        let GpuDevice { device, .. } = get_device();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size_of::<T>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            phantom: PhantomData,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLargeForUniform;

impl std::fmt::Display for TooLargeForUniform {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Type is too large to be used in a uniform buffer (max 65536 bytes)")
    }
}

impl std::error::Error for TooLargeForUniform {}

impl<T> UniformBuffer<[T]> {
    pub fn new(len: usize) -> Result<Self, TooLargeForUniform> {
        let size = len as u64 * size_of::<T>() as u64;
        if size < 65536 {
            let GpuDevice { device, .. } = get_device();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            Ok(Self {
                buffer,
                phantom: PhantomData,
            })
        } else {
            Err(TooLargeForUniform)
        }
    }
}

pub trait StorageBufferMode {
    fn get_usage() -> wgpu::BufferUsages;
}

#[derive(Clone, Copy, Debug)]
pub struct Hidden;

impl StorageBufferMode for Hidden {
    fn get_usage() -> wgpu::BufferUsages {
        wgpu::BufferUsages::STORAGE
    }
    
}

#[derive(Clone, Copy, Debug)]
pub struct ReadOnly;

impl StorageBufferMode for ReadOnly {
    fn get_usage() -> wgpu::BufferUsages {
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WriteOnly;

impl StorageBufferMode for WriteOnly {
    fn get_usage() -> wgpu::BufferUsages {
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ReadWrite;

impl StorageBufferMode for ReadWrite {
    fn get_usage() -> wgpu::BufferUsages {
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST
    }
}

pub struct StorageBuffer<T: GpuType + ?Sized, M> {
    buffer: wgpu::Buffer,
    size: T::Size,
    phantom: PhantomData<(fn() -> M, T)>,
}

impl<T: GpuType<Size = StaticSize<T>>, M: StorageBufferMode> StorageBuffer<T, M> {
    pub fn new() -> Self {
        const {
            // If this assertion fails, the size of T
            // is too large to be used in a storage buffer.
            assert!(std::mem::size_of::<T>() < 134217728);
        }
        let GpuDevice { device, .. } = get_device();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size_of::<T>() as u64,
            usage: M::get_usage(),
            mapped_at_creation: false,
        });
        Self {
            buffer,
            size: StaticSize::default(),
            phantom: PhantomData,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLargeForStorage;

impl std::fmt::Display for TooLargeForStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Type is too large to be used in a storage buffer (max 134217728 bytes)")
    }
}

impl std::error::Error for TooLargeForStorage {}

impl<T, M: StorageBufferMode> StorageBuffer<[T], M> where [T]: GpuType<Size = DynamicSize<T>> {
    pub fn new(len: usize) -> Result<Self, TooLargeForStorage> {
        let size = len as u64 * size_of::<T>() as u64;
        if size < 134217728 {
            let GpuDevice { device, .. } = get_device();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: M::get_usage(),
                mapped_at_creation: false,
            });
            Ok(Self {
                buffer,
                size: DynamicSize::new(len),
                phantom: PhantomData,
            })
        } else {
            Err(TooLargeForStorage)
        }
    }
}

impl<T: GpuType + ?Sized> GpuBuffer for StorageBuffer<T, Hidden> {
    type Data = T;
    type ReadBuffer = ();
    type Size = T::Size;

    fn write_bytes(&self, _data: &[u8], _encoder: &mut CommandEncoder, _staging_belt: &mut StagingBelt, _device: &wgpu::Device) { }
    fn copy_to_read_buffer(&self, _encoder: &mut CommandEncoder, _read_buffer: &Self::ReadBuffer) { }

    fn make_read_buffer(_size: Self::Size, _device: &wgpu::Device) -> Self::ReadBuffer {
        ()
    }
}

impl<T: GpuType + ?Sized> GpuBuffer for StorageBuffer<T, WriteOnly> {
    type Data = T;
    type ReadBuffer = ();
    type Size = T::Size;

    fn write_bytes(&self, data: &[u8], encoder: &mut CommandEncoder, staging_belt: &mut StagingBelt, device: &wgpu::Device) {
        let len = data.len() as u64;
        let Some(len) = NonZeroU64::new(len) else { return; };
        staging_belt.write_buffer(
            encoder,
            &self.buffer,
            0,
            len,
            device
        ).copy_from_slice(data);
    }
    fn copy_to_read_buffer(&self, _encoder: &mut CommandEncoder, _read_buffer: &Self::ReadBuffer) { }

    fn make_read_buffer(_size: Self::Size, _device: &wgpu::Device) -> Self::ReadBuffer {
        ()
    }
}

impl<T: GpuType + ?Sized> GpuBuffer for StorageBuffer<T, ReadOnly> {
    type Data = T;
    type ReadBuffer = wgpu::Buffer;
    type Size = T::Size;

    fn write_bytes(&self, _data: &[u8], _encoder: &mut CommandEncoder, _staging_belt: &mut StagingBelt, _device: &wgpu::Device) { }
    fn copy_to_read_buffer(&self, encoder: &mut CommandEncoder, read_buffer: &Self::ReadBuffer) {
        encoder.copy_buffer_to_buffer(&self.buffer, 0, read_buffer, 0, self.size.size());
    }

    fn make_read_buffer(size: Self::Size, device: &wgpu::Device) -> Self::ReadBuffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
}

impl<T: GpuType + ?Sized> GpuBuffer for StorageBuffer<T, ReadWrite> {
    type Data = T;
    type ReadBuffer = wgpu::Buffer;
    type Size = T::Size;

    fn write_bytes(&self, data: &[u8], encoder: &mut CommandEncoder, staging_belt: &mut StagingBelt, device: &wgpu::Device) {
        let len = data.len() as u64;
        let Some(len) = NonZeroU64::new(len) else { return; };
        staging_belt.write_buffer(
            encoder,
            &self.buffer,
            0,
            len,
            device
        ).copy_from_slice(data);
    }
    fn copy_to_read_buffer(&self, encoder: &mut CommandEncoder, read_buffer: &Self::ReadBuffer) {
        encoder.copy_buffer_to_buffer(&self.buffer, 0, read_buffer, 0, self.size.size());
    }

    fn make_read_buffer(size: Self::Size, device: &wgpu::Device) -> Self::ReadBuffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
}

pub trait GpuBufferTuple {
    type BytesSet<'a>;
    type ReadBufferSet;
    type SizeSet: Copy;

    fn write_bytes(&self, data: Self::BytesSet<'_>, encoder: &mut CommandEncoder, staging_belt: &mut StagingBelt, device: &wgpu::Device);
    fn copy_to_read_buffer(&self, encoder: &mut CommandEncoder, read_buffers: &Self::ReadBufferSet);
    fn make_read_buffers(sizes: Self::SizeSet, device: &wgpu::Device) -> Self::ReadBufferSet;
    fn max_size(sizes: Self::SizeSet) -> u64;
}

macro_rules! set_impl {
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

set_impl!(1, 0 A);
set_impl!(2, 0 A, 1 B);
set_impl!(3, 0 A, 1 B, 2 C);
set_impl!(4, 0 A, 1 B, 2 C, 3 D);


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
    fn write_bytes(&mut self, buffers: &S, data: S::BytesSet<'_>, encoder: &mut CommandEncoder, device: &wgpu::Device) {
        buffers.write_bytes(data, encoder, &mut self.staging_belt, device);
    }
    fn copy_to_read_buffer(&self, buffers: &S, encoder: &mut CommandEncoder) {
        buffers.copy_to_read_buffer(encoder, &self.read_buffers);
    }
}
