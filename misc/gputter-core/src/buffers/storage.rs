use super::{GpuBuffer, ReadableGpuBuffer, WritableGpuBuffer};

use crate::size::{BufferSize, DynamicSize};

use crate::{get_device, GpuDevice};

use crate::size::StaticSize;

use std::marker::PhantomData;

use crate::types::GpuType;

pub trait HostStorageBufferMode {
    fn get_usage() -> wgpu::BufferUsages;
}

#[derive(Clone, Copy, Debug)]
pub struct HostHidden;

impl HostStorageBufferMode for HostHidden {
    fn get_usage() -> wgpu::BufferUsages {
        wgpu::BufferUsages::STORAGE
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HostReadOnly;

impl HostStorageBufferMode for HostReadOnly {
    fn get_usage() -> wgpu::BufferUsages {
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HostWriteOnly;

impl HostStorageBufferMode for HostWriteOnly {
    fn get_usage() -> wgpu::BufferUsages {
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HostReadWrite;

impl HostStorageBufferMode for HostReadWrite {
    fn get_usage() -> wgpu::BufferUsages {
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST
    }
}

pub trait ShaderStorageBufferMode {
    fn readonly() -> bool;
}

#[derive(Clone, Copy, Debug)]
pub struct ShaderReadOnly;

impl ShaderStorageBufferMode for ShaderReadOnly {
    fn readonly() -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ShaderReadWrite;

impl ShaderStorageBufferMode for ShaderReadWrite {
    fn readonly() -> bool {
        false
    }
}

pub struct StorageBuffer<T: GpuType + ?Sized, HM, SM> {
    pub(crate) buffer: wgpu::Buffer,
    pub(crate) size: T::Size,
    pub(crate) phantom: PhantomData<(fn() -> (HM, SM), T)>,
}

impl<T, HM, SM> StorageBuffer<T, HM, SM>
where
    T: GpuType<Size = StaticSize<T>>,
    HM: HostStorageBufferMode,
{
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
            usage: HM::get_usage(),
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
        write!(
            f,
            "Type is too large to be used in a storage buffer (max 134217728 bytes)"
        )
    }
}

impl std::error::Error for TooLargeForStorage {}

impl<T, HM, SM> StorageBuffer<[T], HM, SM>
where
    [T]: GpuType<Size = DynamicSize<T>>,
    HM: HostStorageBufferMode,
{
    pub fn new_dyn(len: usize) -> Result<Self, TooLargeForStorage> {
        let size = len as u64 * size_of::<T>() as u64;
        if size < 134217728 {
            let GpuDevice { device, .. } = get_device();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: HM::get_usage(),
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

impl<T, SM> GpuBuffer for StorageBuffer<T, HostHidden, SM>
where
    T: GpuType + ?Sized,
    SM: ShaderStorageBufferMode,
{
    type Data = T;
    type ReadBuffer = ();
    type Size = T::Size;

    fn make_read_buffer(_size: Self::Size, _device: &wgpu::Device) -> Self::ReadBuffer {
        ()
    }

    fn create_layout(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: SM::readonly(),
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }
    }
    fn get_buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }
    fn get_size(&self) -> Self::Size {
        self.size
    }
}

impl<T, SM> GpuBuffer for StorageBuffer<T, HostWriteOnly, SM>
where
    T: GpuType + ?Sized,
    SM: ShaderStorageBufferMode,
{
    type Data = T;
    type ReadBuffer = ();
    type Size = T::Size;

    fn make_read_buffer(_size: Self::Size, _device: &wgpu::Device) -> Self::ReadBuffer {
        ()
    }

    fn create_layout(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: SM::readonly(),
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }
    }
    fn get_buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }
    fn get_size(&self) -> Self::Size {
        self.size
    }
}

impl<T, SM> WritableGpuBuffer for StorageBuffer<T, HostWriteOnly, SM>
where
    T: GpuType + ?Sized,
    SM: ShaderStorageBufferMode, {}

impl<T, SM> GpuBuffer for StorageBuffer<T, HostReadOnly, SM>
where
    T: GpuType + ?Sized,
    SM: ShaderStorageBufferMode,
{
    type Data = T;
    type ReadBuffer = wgpu::Buffer;
    type Size = T::Size;


    fn make_read_buffer(size: Self::Size, device: &wgpu::Device) -> Self::ReadBuffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn create_layout(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: SM::readonly(),
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }
    }
    fn get_buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }
    fn get_size(&self) -> Self::Size {
        self.size
    }
}

impl<T, SM> ReadableGpuBuffer for StorageBuffer<T, HostReadOnly, SM>
where
    T: GpuType + ?Sized,
    SM: ShaderStorageBufferMode, {}

impl<T, SM> GpuBuffer for StorageBuffer<T, HostReadWrite, SM>
where
    T: GpuType + ?Sized,
    SM: ShaderStorageBufferMode,
{
    type Data = T;
    type ReadBuffer = wgpu::Buffer;
    type Size = T::Size;

    fn make_read_buffer(size: Self::Size, device: &wgpu::Device) -> Self::ReadBuffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn create_layout(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: SM::readonly(),
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }
    }
    fn get_buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }
    fn get_size(&self) -> Self::Size {
        self.size
    }
}

impl<T, SM> WritableGpuBuffer for StorageBuffer<T, HostReadWrite, SM>
where
    T: GpuType + ?Sized,
    SM: ShaderStorageBufferMode, {}
impl<T, SM> ReadableGpuBuffer for StorageBuffer<T, HostReadWrite, SM>
where
    T: GpuType + ?Sized,
    SM: ShaderStorageBufferMode, {}