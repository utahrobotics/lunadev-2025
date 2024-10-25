use crate::{get_device, GpuDevice};

use std::num::NonZeroU64;

use wgpu::util::StagingBelt;

use wgpu::CommandEncoder;

use super::GpuBuffer;

use crate::types::GpuType;

use std::marker::PhantomData;

/// Uniform Buffers can only be read from shaders, and written to by the host.
pub struct UniformBuffer<T: ?Sized> {
    pub(crate) buffer: wgpu::Buffer,
    pub(crate) phantom: PhantomData<T>,
}

impl<T: GpuType + ?Sized> GpuBuffer for UniformBuffer<T> {
    type Data = T;
    type ReadBuffer = ();
    type Size = T::Size;

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
            .write_buffer(encoder, &self.buffer, 0, len, device)
            .copy_from_slice(data);
    }
    fn copy_to_read_buffer(&self, _encoder: &mut CommandEncoder, _read_buffer: &Self::ReadBuffer) {}
    fn make_read_buffer(_size: Self::Size, _device: &wgpu::Device) -> Self::ReadBuffer {
        ()
    }

    fn create_layout(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }
    }
    fn get_entire_binding(&self) -> wgpu::BufferBinding {
        self.buffer.as_entire_buffer_binding()
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
        write!(
            f,
            "Type is too large to be used in a uniform buffer (max 65536 bytes)"
        )
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
