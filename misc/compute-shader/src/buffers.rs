use std::{
    cell::OnceCell,
    future::Future,
    marker::PhantomData,
    mem::{align_of, size_of},
    num::NonZeroU64,
    ops::{Deref, DerefMut},
    sync::{Exclusive, RwLock},
};

use bytemuck::{bytes_of, cast_slice, cast_slice_mut, checked::from_bytes_mut, from_bytes};
use crossbeam::queue::SegQueue;
use futures::FutureExt;
use fxhash::FxHashMap;
use tokio::sync::oneshot;
use wgpu::{util::StagingBelt, BufferView, BufferViewMut, CommandEncoder, Maintain, MapMode};

use crate::{get_gpu_device, GpuDevice};

trait HostReadableWritable {
    const CAN_READ: bool;
    const CAN_WRITE: bool;
}

/// Marker type indicating that a buffer can only be read from by the host.
#[derive(Debug, Clone, Copy)]
pub struct HostReadOnly;

impl HostReadableWritable for HostReadOnly {
    const CAN_READ: bool = true;
    const CAN_WRITE: bool = false;
}

/// Marker type indicating that a buffer can only be written to by the host.
#[derive(Debug, Clone, Copy)]
pub struct HostWriteOnly;

impl HostReadableWritable for HostWriteOnly {
    const CAN_READ: bool = false;
    const CAN_WRITE: bool = true;
}

/// Marker type indicating that a buffer can be read from and written to by the host.
#[derive(Debug, Clone, Copy)]
pub struct HostReadWrite;

impl HostReadableWritable for HostReadWrite {
    const CAN_READ: bool = true;
    const CAN_WRITE: bool = true;
}

trait ShaderWritable {
    const CAN_WRITE: bool;
}

/// Marker type indicating that a buffer can only be read from by shaders.
#[derive(Debug, Clone, Copy)]
pub struct ShaderReadOnly;

impl ShaderWritable for ShaderReadOnly {
    const CAN_WRITE: bool = false;
}

/// Marker type indicating that a buffer can be read from and written to by shaders.
#[derive(Debug, Clone, Copy)]
pub struct ShaderReadWrite;

impl ShaderWritable for ShaderReadWrite {
    const CAN_WRITE: bool = true;
}

trait UniformOrStorage {
    const IS_UNIFORM: bool;
}

/// Marker type indicating that a buffer is a uniform buffer.
#[derive(Debug, Clone, Copy)]
pub struct UniformOnly;

impl UniformOrStorage for UniformOnly {
    const IS_UNIFORM: bool = true;
}

/// Marker type indicating that a buffer is a storage buffer.
///
/// This is the default since storage buffers are bigger and can be written to.
#[derive(Debug, Clone, Copy)]
pub struct StorageOnly;

impl UniformOrStorage for StorageOnly {
    const IS_UNIFORM: bool = false;
}

/// Statically encodes the type of buffer.
///
/// The first generic type is the type of data stored in the buffer.
/// The second generic type is the type of access the host has to the buffer.
/// The third generic type is the type of access shaders have to the buffer.
/// The fourth generic type is the type of buffer (uniform or storage), and is Storage by default
pub struct BufferType<T: BufferSized + ?Sized, H, S, O = StorageOnly> {
    size: T::Size,
    _phantom: PhantomData<fn() -> (H, S, O, T)>,
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
    /// Creates a new buffer capable of holding `T`.
    pub fn new() -> Self {
        Self {
            size: StaticSize::default(),
            _phantom: PhantomData,
        }
    }
}

impl<T: 'static, H, S, O> BufferType<[T], H, S, O> {
    /// Creates a new buffer capable of holding a slice of `T` with the given `len`.
    pub fn new_dyn(len: usize) -> Self {
        Self {
            size: DynamicSize::new(len),
            _phantom: PhantomData,
        }
    }
}

/// A trait for `BufferType` indicating that it is capable of creating a buffer.
///
/// If a `BufferType` implements `ValidBufferType`, it can be used to create a buffer.
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

/// Marker trait for valid buffer types considering all of the statically known information.
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

/// A trait for types that can accurately represent the size of a buffer.
pub trait BufferSize: Copy + Default + Send + 'static {
    fn size(&self) -> u64;
}

/// A buffer size that is statically known as `T` is statically sized.
pub struct StaticSize<T>(PhantomData<fn() -> T>);

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

/// A buffer size that is a multiple of `T`, where `T` is statically sized.
///
/// Used for slices.
pub struct DynamicSize<T: ?Sized>(pub usize, PhantomData<fn() -> T>);

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

/// Marker trait for types that can fit into a buffer.
///
/// Valid types are types that have a `'static` lifetime, and are either
/// statically sized, or are slices of types that are statically sized.
pub trait BufferSized {
    /// The type of size that this type can be represented with.
    type Size: BufferSize;
}

impl<T: 'static> BufferSized for T {
    type Size = StaticSize<T>;
}

impl<T: 'static> BufferSized for [T] {
    type Size = DynamicSize<T>;
}

/// A type that implements `BufferSource<T>` can be written to a buffer that stores `T`.
///
/// All buffer sized types are buffer sources for themselves.  
/// `()` is a buffer source for any type, but it is a noop. No data actually gets written to the buffer.  
/// `Option<&T>` is a buffer source for `T`, but only if the option is `Some`. Otherwise, it is equivalent to `()`.  
/// Refer to `OpaqueBuffer` for the last category.
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

impl<T: bytemuck::Pod> BufferSource<[T]> for &[T] {
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

/// A type that implements `BufferDestination<T>` can be read from a buffer that stores `T`.
///
/// All buffer sized types are buffer destinations for themselves.
/// `()` is a buffer destination for any type, but it is a noop. No data actually gets read from the buffer.
/// `Option<&mut T>` is a buffer destination for `T`, but only if the option is `Some`. Otherwise, it is equivalent to `()`.
/// Refer to `OpaqueBuffer` for the last category.
pub trait BufferDestination<T: ?Sized> {
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

impl<T: bytemuck::Pod> BufferDestination<T> for &mut T {
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

        command_encoder.copy_buffer_to_buffer(src_buffer, 0, &buffer, 0, size_of::<T>() as u64);

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

impl<T: bytemuck::Pod> BufferDestination<[T]> for &mut [T] {
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

        command_encoder.copy_buffer_to_buffer(src_buffer, 0, &buffer, 0, size);

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

impl<'a, T> BufferDestination<T> for Option<&'a mut T>
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

impl<T: ?Sized> BufferDestination<T> for () {
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

/// A GPU Buffer is a buffer that cannot be read or written to outside of a shader.
///
/// This is useful for when you want to pass data between shaders but don't need to read or write to it on the host.
/// If you do need to access the data, you can copy to and from this buffer with another buffer that has your desired access.
///
/// An `OpaqueBuffer` is a valid buffer source and destination for any buffer sized type. Care must be taken to ensure that the
/// `OpaqueBuffer` is of an appropriate size for the actual type stored in the buffer.
///
/// While this is limiting, this is the fastest way to pass data between shaders as it skips synchronizing data with the host entirely.
/// Due to the underlying implemention of `wgpu`, for a buffer to be able to copy to and from other buffers its data cannot be accessible
/// by the host in any capacity.
pub struct GpuBuffer<T: BufferSized + ?Sized> {
    size: T::Size,
    buffer: wgpu::Buffer,
    read_buffer: Exclusive<OnceCell<wgpu::Buffer>>,
    write_buffer: Exclusive<OnceCell<wgpu::Buffer>>,
    _phantom: PhantomData<fn() -> T>,
}

impl<T: ?Sized + BufferSized + 'static> BufferSource<T> for &GpuBuffer<T> {
    fn into_buffer(
        self,
        command_encoder: &mut CommandEncoder,
        buffer: &wgpu::Buffer,
        _stager: &mut StagingBelt,
        _device: &wgpu::Device,
    ) {
        if self.size.size() == 0 {
            return;
        }
        command_encoder.copy_buffer_to_buffer(&self.buffer, 0, &buffer, 0, self.size.size());
    }
}

impl<T: ?Sized + BufferSized + 'static> BufferDestination<T> for &mut GpuBuffer<T> {
    type State = ();

    fn enqueue(
        &self,
        command_encoder: &mut CommandEncoder,
        src_buffer: &wgpu::Buffer,
        _buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
        _device: &wgpu::Device,
    ) -> Self::State {
        if self.size.size() == 0 {
            return;
        }
        command_encoder.copy_buffer_to_buffer(src_buffer, 0, &self.buffer, 0, self.size.size());
    }

    async fn from_buffer(
        &mut self,
        (): Self::State,
        _device: &wgpu::Device,
        _buffers: &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
    ) {
    }
}

impl<T: BufferSized + ?Sized> GpuBuffer<T> {
    /// Creates a zeroed buffer with the given size.
    async fn zeroed_inner(size: T::Size) -> anyhow::Result<Self> {
        let GpuDevice { device, .. } = get_gpu_device().await?;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size.size(),
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        });
        Ok(Self {
            buffer,
            size,
            read_buffer: Default::default(),
            write_buffer: Default::default(),
            _phantom: PhantomData,
        })
    }

    async fn read_bytes(&mut self) -> BytesReadGuard {
        let GpuDevice { device, queue } = get_gpu_device().now_or_never().unwrap().unwrap();

        let read_buffer = self.read_buffer.get_mut().get_mut_or_init(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: self.size.size(),
                mapped_at_creation: false,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            })
        });

        let mut command_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        command_encoder.copy_buffer_to_buffer(&self.buffer, 0, read_buffer, 0, self.size.size());

        let slice = self.buffer.slice(0..self.size.size());
        let (sender, receiver) = oneshot::channel::<()>();
        slice.map_async(MapMode::Read, move |_| {
            let _sender = sender;
        });
        queue.submit(std::iter::once(command_encoder.finish()));
        let _ = receiver.await;
        let view = slice.get_mapped_range();
        BytesReadGuard {
            view: Some(view),
            buffer: &self.buffer,
        }
    }

    async fn write_bytes(&mut self) -> BytesWriteGuard {
        let GpuDevice { device, queue } = get_gpu_device().now_or_never().unwrap().unwrap();

        let mut already_mapped = false;

        let write_buffer = self.write_buffer.get_mut().get_mut_or_init(|| {
            already_mapped = true;
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: self.size.size(),
                mapped_at_creation: true,
                usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            })
        });

        let slice = write_buffer.slice(0..self.size.size());
        if !already_mapped {
            let (sender, receiver) = oneshot::channel::<()>();
            slice.map_async(MapMode::Read, move |_| {
                let _sender = sender;
            });
            queue.submit(std::iter::empty());
            let _ = receiver.await;
        }
        let view = slice.get_mapped_range_mut();
        BytesWriteGuard {
            view: Some(view),
            size: self.size.size(),
            referenced: false,
            write_buffer,
            buffer: &self.buffer,
        }
    }
}

impl<T: bytemuck::Pod> GpuBuffer<T> {
    /// Creates a buffer that will store the given value.
    pub async fn new(value: &T) -> anyhow::Result<Self> {
        let size = StaticSize::<T>::default();
        let GpuDevice { device, .. } = get_gpu_device().await?;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size.size(),
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
            read_buffer: Default::default(),
            write_buffer: Default::default(),
            _phantom: PhantomData,
        })
    }

    pub async fn zeroed() -> anyhow::Result<Self> {
        Self::zeroed_inner(StaticSize::default()).await
    }

    pub async fn read(&mut self) -> ReadGuard<'_, T> {
        let guard = self.read_bytes().await;
        ReadGuard {
            guard,
            _phantom: PhantomData,
        }
    }

    pub async fn write(&mut self) -> WriteGuard<'_, T> {
        let guard = self.write_bytes().await;
        WriteGuard {
            guard,
            _phantom: PhantomData,
        }
    }
}

impl<T: bytemuck::Pod> GpuBuffer<[T]> {
    /// Creates a buffer that will store the given slice.
    pub async fn new(slice: &[T]) -> anyhow::Result<Self> {
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
            size: DynamicSize::new(slice.len()),
            read_buffer: Default::default(),
            write_buffer: Default::default(),
            _phantom: PhantomData,
        })
    }

    pub async fn zeroed(len: usize) -> anyhow::Result<Self> {
        Self::zeroed_inner(DynamicSize::new(len)).await
    }

    pub async fn read(&mut self) -> ReadGuard<'_, [T]> {
        let guard = self.read_bytes().await;
        ReadGuard {
            guard,
            _phantom: PhantomData,
        }
    }

    pub async fn write(&mut self) -> WriteGuard<'_, [T]> {
        let guard = self.write_bytes().await;
        WriteGuard {
            guard,
            _phantom: PhantomData,
        }
    }
}

struct BytesReadGuard<'a> {
    buffer: &'a wgpu::Buffer,
    view: Option<BufferView<'a>>,
}

impl<'a> Drop for BytesReadGuard<'a> {
    fn drop(&mut self) {
        self.view = None;
        self.buffer.unmap();
    }
}

impl<'a> Deref for BytesReadGuard<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &*self.view.as_ref().unwrap()
    }
}

struct BytesWriteGuard<'a> {
    buffer: &'a wgpu::Buffer,
    write_buffer: &'a wgpu::Buffer,
    size: u64,
    referenced: bool,
    view: Option<BufferViewMut<'a>>,
}

impl<'a> BytesWriteGuard<'a> {
    /// Writes the data in this buffer into the original buffer.
    ///
    /// This happens automatically when the guard is dropped. However, it is not guaranteed that the write completes as soon as `drop` returns.
    ///
    /// # Note
    /// Awaiting the future is optional. The data will be written to the buffer regardless at some point in the future.
    /// Await the future if you want to know when the data has been written.
    pub async fn flush(&mut self) {
        let GpuDevice { device, queue } = get_gpu_device().now_or_never().unwrap().unwrap();

        let mut command_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        command_encoder.copy_buffer_to_buffer(self.write_buffer, 0, self.buffer, 0, self.size);

        let idx = queue.submit(std::iter::once(command_encoder.finish()));
        self.referenced = false;

        let _ = tokio::task::spawn_blocking(|| {
            device.poll(wgpu::MaintainBase::WaitForSubmissionIndex(idx));
        })
        .await;
    }
}

impl<'a> Drop for BytesWriteGuard<'a> {
    fn drop(&mut self) {
        if self.referenced {
            let _ = self.flush();
        }
        self.view = None;
        self.buffer.unmap();
    }
}

impl<'a> Deref for BytesWriteGuard<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &*self.view.as_ref().unwrap()
    }
}

impl<'a> DerefMut for BytesWriteGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.referenced = true;
        &mut *self.view.as_mut().unwrap()
    }
}

pub struct ReadGuard<'a, T: ?Sized> {
    guard: BytesReadGuard<'a>,
    _phantom: PhantomData<T>,
}

impl<'a, T: bytemuck::Pod> Deref for ReadGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        from_bytes(&self.guard)
    }
}

impl<'a, T: bytemuck::Pod> Deref for ReadGuard<'a, [T]> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        cast_slice(&self.guard)
    }
}

pub struct WriteGuard<'a, T: ?Sized> {
    guard: BytesWriteGuard<'a>,
    _phantom: PhantomData<T>,
}

impl<'a, T: bytemuck::Pod> Deref for WriteGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        from_bytes(&self.guard)
    }
}

impl<'a, T: bytemuck::Pod> Deref for WriteGuard<'a, [T]> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        cast_slice(&self.guard)
    }
}

impl<'a, T: bytemuck::Pod> DerefMut for WriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        from_bytes_mut(&mut self.guard)
    }
}

impl<'a, T: bytemuck::Pod> DerefMut for WriteGuard<'a, [T]> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        cast_slice_mut(&mut self.guard)
    }
}
