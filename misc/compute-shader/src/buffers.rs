use std::{
    future::Future,
    marker::PhantomData,
    mem::{align_of, size_of},
    num::NonZeroU64,
    ops::{Deref, DerefMut},
    sync::RwLock,
};

use bytemuck::{bytes_of, cast_slice, cast_slice_mut, from_bytes, from_bytes_mut};
use crossbeam::queue::SegQueue;
use fxhash::FxHashMap;
use tokio::sync::oneshot;
use wgpu::{util::StagingBelt, CommandEncoder, Maintain, MapMode};

use crate::{get_gpu_device, GpuDevice};

pub trait HostReadableWritable {
    const CAN_READ: bool;
    const CAN_WRITE: bool;
}

#[derive(Debug, Clone, Copy)]
pub struct HostReadOnly;

impl HostReadableWritable for HostReadOnly {
    const CAN_READ: bool = true;
    const CAN_WRITE: bool = false;
}
#[derive(Debug, Clone, Copy)]
pub struct HostWriteOnly;

impl HostReadableWritable for HostWriteOnly {
    const CAN_READ: bool = false;
    const CAN_WRITE: bool = true;
}

#[derive(Debug, Clone, Copy)]
pub struct HostReadWrite;

impl HostReadableWritable for HostReadWrite {
    const CAN_READ: bool = true;
    const CAN_WRITE: bool = true;
}

pub trait ShaderWritable {
    const CAN_WRITE: bool;
}

#[derive(Debug, Clone, Copy)]
pub struct ShaderReadOnly;

impl ShaderWritable for ShaderReadOnly {
    const CAN_WRITE: bool = false;
}

#[derive(Debug, Clone, Copy)]
pub struct ShaderReadWrite;

impl ShaderWritable for ShaderReadWrite {
    const CAN_WRITE: bool = true;
}

pub trait UniformOrStorage {
    const IS_UNIFORM: bool;
}

#[derive(Debug, Clone, Copy)]
pub struct UniformOnly;

impl UniformOrStorage for UniformOnly {
    const IS_UNIFORM: bool = true;
}

#[derive(Debug, Clone, Copy)]
pub struct StorageOnly;

impl UniformOrStorage for StorageOnly {
    const IS_UNIFORM: bool = false;
}

pub struct BufferType<T: BufferSized + ?Sized, H, S, O = StorageOnly> {
    size: T::Size,
    _phantom: PhantomData<(H, S, O, T)>,
}

impl<T: BufferSized, H, S, O> Clone for BufferType<T, H, S, O> {
    fn clone(&self) -> Self {
        Self {
            size: self.size,
            _phantom: PhantomData,
        }
    }
}

impl<T: BufferSized, H, S, O> Copy for BufferType<T, H, S, O> {}

impl<T: 'static, H, S, O> BufferType<T, H, S, O> {
    pub fn new() -> Self {
        Self {
            size: StaticSize::default(),
            _phantom: PhantomData,
        }
    }
}

impl<T: 'static, H, S, O> BufferType<[T], H, S, O> {
    pub fn new_dyn(len: usize) -> Self {
        Self {
            size: DynamicSize::new(len),
            _phantom: PhantomData,
        }
    }
}

pub trait CreateBuffer: ValidBufferType {
    const HOST_CAN_READ: bool;
    const HOST_CAN_WRITE: bool;
    const SHADER_CAN_WRITE: bool;

    fn size(&self) -> u64;
    fn into_buffer(&self, index: usize, device: &wgpu::Device) -> wgpu::Buffer;
    fn into_layout(&self, binding: u32) -> wgpu::BindGroupLayoutEntry;
}

impl<T: BufferSized + ?Sized, H: HostReadableWritable, S: ShaderWritable, O: UniformOrStorage>
    CreateBuffer for BufferType<T, H, S, O>
where
    Self: ValidBufferType,
{
    const HOST_CAN_READ: bool = H::CAN_READ;
    const HOST_CAN_WRITE: bool = H::CAN_WRITE;
    const SHADER_CAN_WRITE: bool = S::CAN_WRITE;

    fn size(&self) -> u64 {
        self.size.size()
    }

    fn into_buffer(&self, index: usize, device: &wgpu::Device) -> wgpu::Buffer {
        let additional_usage = if Self::HOST_CAN_READ && Self::HOST_CAN_WRITE {
            wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST
        } else if Self::HOST_CAN_WRITE {
            wgpu::BufferUsages::COPY_DST
        } else {
            wgpu::BufferUsages::COPY_SRC
        };

        if O::IS_UNIFORM {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("Arg Buffer {index}")),
                size: self.size.size(),
                usage: wgpu::BufferUsages::UNIFORM | additional_usage,
                mapped_at_creation: false,
            })
        } else {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("Arg Buffer {index}")),
                size: self.size.size(),
                usage: wgpu::BufferUsages::STORAGE | additional_usage,
                mapped_at_creation: false,
            })
        }
    }

    fn into_layout(&self, binding: u32) -> wgpu::BindGroupLayoutEntry {
        if Self::SHADER_CAN_WRITE {
            wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }
        } else if O::IS_UNIFORM {
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
        } else {
            wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }
        }
    }
}

pub trait ValidBufferType {
    type WriteType: ?Sized + BufferSized + 'static;
    type ReadType: ?Sized + BufferSized + 'static;
}

impl<T: BufferSized + ?Sized + 'static, O> ValidBufferType
    for BufferType<T, HostReadOnly, ShaderReadOnly, O>
{
    type WriteType = ();
    type ReadType = T;
}

impl<T: BufferSized + ?Sized + 'static, O> ValidBufferType
    for BufferType<T, HostWriteOnly, ShaderReadOnly, O>
{
    type WriteType = T;
    type ReadType = ();
}

impl<T: BufferSized + ?Sized + 'static, O> ValidBufferType
    for BufferType<T, HostReadWrite, ShaderReadOnly, O>
{
    type WriteType = T;
    type ReadType = T;
}

impl<T: BufferSized + ?Sized + 'static> ValidBufferType
    for BufferType<T, HostReadOnly, ShaderReadWrite, StorageOnly>
{
    type WriteType = ();
    type ReadType = T;
}

impl<T: BufferSized + ?Sized + 'static> ValidBufferType
    for BufferType<T, HostWriteOnly, ShaderReadWrite, StorageOnly>
{
    type WriteType = T;
    type ReadType = ();
}

impl<T: BufferSized + ?Sized + 'static> ValidBufferType
    for BufferType<T, HostReadWrite, ShaderReadWrite, StorageOnly>
{
    type WriteType = T;
    type ReadType = T;
}

pub trait BufferSize: Copy + Default + Send + 'static {
    fn size(&self) -> u64;
}

pub struct StaticSize<T>(PhantomData<T>);

unsafe impl<T> Send for StaticSize<T> {}
unsafe impl<T> Sync for StaticSize<T> {}

impl<T> Default for StaticSize<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T> Copy for StaticSize<T> {}
impl<T> Clone for StaticSize<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> BufferSize for StaticSize<T> {
    fn size(&self) -> u64 {
        size_of::<T>() as u64
    }
}

pub struct DynamicSize<T: ?Sized>(pub usize, PhantomData<T>);

unsafe impl<T> Send for DynamicSize<T> {}
unsafe impl<T> Sync for DynamicSize<T> {}

impl<T: 'static> BufferSize for DynamicSize<T> {
    fn size(&self) -> u64 {
        let stride = size_of::<T>().next_multiple_of(align_of::<T>()) as u64;
        self.0 as u64 * stride
    }
}

impl<T: ?Sized> DynamicSize<T> {
    pub fn new(len: usize) -> Self {
        Self(len, PhantomData)
    }
}
impl<T: ?Sized> Default for DynamicSize<T> {
    fn default() -> Self {
        Self(0, PhantomData)
    }
}
impl<T: ?Sized> Copy for DynamicSize<T> {}
impl<T: ?Sized> Clone for DynamicSize<T> {
    fn clone(&self) -> Self {
        *self
    }
}

pub trait BufferSized {
    type Size: BufferSize;
}

impl<T: 'static> BufferSized for T {
    type Size = StaticSize<T>;
}

impl<T: 'static> BufferSized for [T] {
    type Size = DynamicSize<T>;
}

pub trait BufferSource<T: BufferSized + ?Sized> {
    fn into_buffer(
        self,
        command_encoder: &mut CommandEncoder,
        buffer: &wgpu::Buffer,
        stager: &mut StagingBelt,
        device: &wgpu::Device,
    );
}

impl<T: ?Sized + BufferSized + 'static> BufferSource<T> for () {
    fn into_buffer(
        self,
        _command_encoder: &mut CommandEncoder,
        _buffer: &wgpu::Buffer,
        _stager: &mut StagingBelt,
        _device: &wgpu::Device,
    ) {
    }
}

impl<T: BufferSized + bytemuck::Pod> BufferSource<T> for &T {
    fn into_buffer(
        self,
        command_encoder: &mut CommandEncoder,
        buffer: &wgpu::Buffer,
        stager: &mut StagingBelt,
        device: &wgpu::Device,
    ) {
        stager
            .write_buffer(
                command_encoder,
                buffer,
                0,
                NonZeroU64::new(size_of::<T>() as u64).unwrap(),
                device,
            )
            .copy_from_slice(bytes_of(self));
    }
}

impl<T: BufferSized + bytemuck::Pod> BufferSource<[T]> for &[T] {
    fn into_buffer(
        self,
        command_encoder: &mut CommandEncoder,
        buffer: &wgpu::Buffer,
        stager: &mut StagingBelt,
        device: &wgpu::Device,
    ) {
        if self.is_empty() {
            return;
        }
        let bytes = cast_slice(self);
        stager
            .write_buffer(
                command_encoder,
                buffer,
                0,
                NonZeroU64::new(bytes.len() as u64).unwrap(),
                device,
            )
            .copy_from_slice(bytes);
    }
}

impl<T: BufferSized + bytemuck::Pod> BufferSource<T> for Option<&T> {
    fn into_buffer(
        self,
        command_encoder: &mut CommandEncoder,
        buffer: &wgpu::Buffer,
        stager: &mut StagingBelt,
        device: &wgpu::Device,
    ) {
        if let Some(item) = self {
            item.into_buffer(command_encoder, buffer, stager, device);
        }
    }
}

pub trait BufferDestination<T: BufferSized + ?Sized> {
    type State;
    fn enqueue(
        &self,
        command_encoder: &mut CommandEncoder,
        src_buffer: &wgpu::Buffer,
        buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
        device: &wgpu::Device,
    ) -> Self::State;
    fn from_buffer(
        &mut self,
        state: Self::State,
        device: &wgpu::Device,
        buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
    ) -> impl Future<Output = ()>;
}

impl<T: BufferSized + bytemuck::Pod> BufferDestination<T> for &mut T {
    type State = wgpu::Buffer;

    fn enqueue(
        &self,
        command_encoder: &mut CommandEncoder,
        src_buffer: &wgpu::Buffer,
        buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
        device: &wgpu::Device,
    ) -> Self::State {
        let buffer = {
            let reader = buffers.read().unwrap();
            reader
                .get(&(size_of::<T>() as u64))
                .and_then(|queue| queue.pop())
                .unwrap_or_else(|| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: None,
                        size: size_of::<T>() as u64,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    })
                })
        };

        command_encoder.copy_buffer_to_buffer(&src_buffer, 0, &buffer, 0, size_of::<T>() as u64);

        buffer
    }

    async fn from_buffer(
        &mut self,
        buffer: Self::State,
        device: &wgpu::Device,
        buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
    ) {
        {
            let slice = buffer.slice(..);
            let (sender, receiver) = oneshot::channel::<()>();
            slice.map_async(MapMode::Read, move |result| {
                if result.is_err() {
                    return;
                }
                let _ = sender.send(());
            });
            device.poll(Maintain::Poll);
            receiver.await.expect("Failed to map buffer");
            let slice = slice.get_mapped_range();
            let buffer_ref: &T = from_bytes(&slice);
            **self = *buffer_ref;
        }

        buffer.unmap();

        {
            let reader = buffers.read().unwrap();
            if let Some(queue) = reader.get(&(size_of::<T>() as u64)) {
                queue.push(buffer);
                return;
            }
        }

        let queue = SegQueue::new();
        queue.push(buffer);
        buffers
            .write()
            .unwrap()
            .insert(size_of::<T>() as u64, queue);
    }
}

impl<T: BufferSized + bytemuck::Pod + Send> BufferDestination<[T]> for &mut [T] {
    type State = Option<wgpu::Buffer>;

    fn enqueue(
        &self,
        command_encoder: &mut CommandEncoder,
        src_buffer: &wgpu::Buffer,
        buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
        device: &wgpu::Device,
    ) -> Self::State {
        if self.is_empty() {
            return None;
        }
        let size = (self.len() * size_of::<T>()) as u64;
        let buffer = {
            let reader = buffers.read().unwrap();
            reader
                .get(&size)
                .and_then(|queue| queue.pop())
                .unwrap_or_else(|| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: None,
                        size,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    })
                })
        };

        command_encoder.copy_buffer_to_buffer(&src_buffer, 0, &buffer, 0, size);

        Some(buffer)
    }

    async fn from_buffer(
        &mut self,
        buffer: Self::State,
        device: &wgpu::Device,
        buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
    ) {
        let Some(buffer) = buffer else {
            return;
        };
        {
            let size = (self.len() * size_of::<T>()) as u64;
            let slice = buffer.slice(0..size);
            let (sender, receiver) = oneshot::channel::<()>();
            slice.map_async(MapMode::Read, move |result| {
                if result.is_err() {
                    return;
                }
                let _ = sender.send(());
            });
            device.poll(Maintain::Poll);
            receiver.await.expect("Failed to map buffer");
            let slice = slice.get_mapped_range();

            self.copy_from_slice(cast_slice(&slice));
        }

        buffer.unmap();

        {
            let reader = buffers.read().unwrap();
            if let Some(queue) = reader.get(&buffer.size()) {
                queue.push(buffer);
                return;
            }
        }

        let queue = SegQueue::new();
        let size = buffer.size();
        queue.push(buffer);
        buffers.write().unwrap().insert(size, queue);
    }
}

impl<'a, T: 'static> BufferDestination<T> for Option<&'a mut T>
where
    &'a mut T: BufferDestination<T>,
{
    type State = Option<<&'a mut T as BufferDestination<T>>::State>;

    fn enqueue(
        &self,
        command_encoder: &mut CommandEncoder,
        src_buffer: &wgpu::Buffer,
        buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
        device: &wgpu::Device,
    ) -> Self::State {
        if let Some(item) = self {
            Some(item.enqueue(command_encoder, src_buffer, buffers, device))
        } else {
            None
        }
    }

    async fn from_buffer(
        &mut self,
        state: Self::State,
        device: &wgpu::Device,
        buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
    ) {
        if let Some(item) = self {
            item.from_buffer(state.unwrap(), device, buffers).await;
        }
    }
}

impl<T: ?Sized + BufferSized + 'static> BufferDestination<T> for () {
    type State = ();

    fn enqueue(
        &self,
        _command_encoder: &mut CommandEncoder,
        _src_buffer: &wgpu::Buffer,
        _buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
        _device: &wgpu::Device,
    ) -> Self::State {
    }

    async fn from_buffer(
        &mut self,
        _state: Self::State,
        _device: &wgpu::Device,
        _buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
    ) {
    }
}

pub struct OpaqueBuffer {
    pub size: u64,
    pub start_offset: u64,
    buffer: wgpu::Buffer,
}

impl<T: ?Sized + BufferSized + 'static> BufferSource<T> for &OpaqueBuffer {
    fn into_buffer(
        self,
        command_encoder: &mut CommandEncoder,
        buffer: &wgpu::Buffer,
        _stager: &mut StagingBelt,
        _device: &wgpu::Device,
    ) {
        if self.size == 0 {
            return;
        }
        command_encoder.copy_buffer_to_buffer(
            &self.buffer,
            self.start_offset,
            &buffer,
            0,
            self.size,
        );
    }
}

impl<T: ?Sized + BufferSized + 'static> BufferDestination<T> for &mut OpaqueBuffer {
    type State = ();

    fn enqueue(
        &self,
        command_encoder: &mut CommandEncoder,
        src_buffer: &wgpu::Buffer,
        _buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
        _device: &wgpu::Device,
    ) -> Self::State {
        if self.size == 0 {
            return;
        }
        command_encoder.copy_buffer_to_buffer(
            &src_buffer,
            0,
            &self.buffer,
            self.start_offset,
            self.size,
        );
    }

    async fn from_buffer(
        &mut self,
        (): Self::State,
        _device: &wgpu::Device,
        _buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
    ) {
    }
}

impl OpaqueBuffer {
    pub async fn new(size: impl BufferSize) -> anyhow::Result<Self> {
        let size = size.size();
        let GpuDevice { device, .. } = get_gpu_device().await?;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        });
        Ok(Self {
            buffer,
            size,
            start_offset: 0,
        })
    }

    pub async fn new_from_value<T: bytemuck::Pod>(value: &T) -> anyhow::Result<Self> {
        let size = size_of::<T>() as u64;
        let GpuDevice { device, .. } = get_gpu_device().await?;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            mapped_at_creation: true,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        });
        buffer
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(bytes_of(value));
        buffer.unmap();
        Ok(Self {
            buffer,
            size,
            start_offset: 0,
        })
    }

    pub async fn new_from_slice<T: bytemuck::Pod>(slice: &[T]) -> anyhow::Result<Self> {
        let GpuDevice { device, .. } = get_gpu_device().await?;
        let bytes: &[u8] = cast_slice(slice);
        let size = bytes.len() as u64;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            mapped_at_creation: true,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        });
        buffer
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(bytes);
        buffer.unmap();
        Ok(Self {
            buffer,
            size,
            start_offset: 0,
        })
    }

    pub async fn get_bytes(&mut self, f: impl FnOnce(&[u8])) {
        let slice = self.buffer.slice(0..self.size);
        let (sender, receiver) = oneshot::channel::<()>();
        slice.map_async(MapMode::Read, move |_| {
            let _sender = sender;
        });
        let _ = receiver.await;
        f(&slice.get_mapped_range());
        self.buffer.unmap();
    }

    pub async fn get_bytes_mut(&mut self, f: impl FnOnce(&mut [u8])) {
        let slice = self.buffer.slice(0..self.size);
        let (sender, receiver) = oneshot::channel::<()>();
        slice.map_async(MapMode::Write, move |_| {
            let _sender = sender;
        });
        let _ = receiver.await;
        f(&mut slice.get_mapped_range_mut());
        self.buffer.unmap();
    }
}

#[repr(transparent)]
pub struct TypedOpaqueBuffer<T: ?Sized> {
    buffer: OpaqueBuffer,
    _phantom: PhantomData<T>,
}

impl<T: ?Sized> TypedOpaqueBuffer<T> {
    pub fn from_opaque_buffer(buf: OpaqueBuffer) -> Self {
        Self {
            buffer: buf,
            _phantom: PhantomData,
        }
    }

    pub fn from_opaque_buffer_ref(buf: &OpaqueBuffer) -> &Self {
        unsafe { std::mem::transmute(buf) }
    }

    pub fn from_opaque_buffer_mut(buf: &mut OpaqueBuffer) -> &mut Self {
        unsafe { std::mem::transmute(buf) }
    }

    pub async fn new(size: impl BufferSize) -> anyhow::Result<Self> {
        Ok(Self {
            buffer: OpaqueBuffer::new(size).await?,
            _phantom: PhantomData,
        })
    }

    pub fn into_inner(self) -> OpaqueBuffer {
        self.buffer
    }
}

impl<T: bytemuck::Pod> TypedOpaqueBuffer<T> {
    pub async fn new_from_value(value: &T) -> anyhow::Result<Self> {
        Ok(Self {
            buffer: OpaqueBuffer::new_from_value(value).await?,
            _phantom: PhantomData,
        })
    }

    pub async fn get_value(&mut self, f: impl FnOnce(&T)) {
        self.buffer.get_bytes(|bytes| f(from_bytes(bytes))).await;
    }

    pub async fn get_value_mut(&mut self, f: impl FnOnce(&mut T)) {
        self.buffer
            .get_bytes_mut(|bytes| f(from_bytes_mut(bytes)))
            .await;
    }
}

impl<T: bytemuck::Pod> TypedOpaqueBuffer<[T]> {
    pub async fn new_from_slice(slice: &[T]) -> anyhow::Result<Self> {
        Ok(Self {
            buffer: OpaqueBuffer::new_from_slice(slice).await?,
            _phantom: PhantomData,
        })
    }

    pub async fn get_slice(&mut self, f: impl FnOnce(&[T])) {
        self.buffer.get_bytes(|bytes| f(cast_slice(bytes))).await;
    }

    pub async fn get_slice_mut(&mut self, f: impl FnOnce(&mut [T])) {
        self.buffer
            .get_bytes_mut(|bytes| f(cast_slice_mut(bytes)))
            .await;
    }
}

impl<T: ?Sized> Deref for TypedOpaqueBuffer<T> {
    type Target = OpaqueBuffer;

    fn deref(&self) -> &Self::Target {
        &self.buffer
    }
}

impl<T: ?Sized> DerefMut for TypedOpaqueBuffer<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buffer
    }
}
