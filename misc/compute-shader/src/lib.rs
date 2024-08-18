use std::sync::RwLock;

use buffers::{BufferDestination, BufferSource, CreateBuffer, ValidBufferType};
use crossbeam::queue::SegQueue;
use futures::FutureExt;
use fxhash::FxHashMap;
use tokio::sync::OnceCell;
use wgpu::{util::StagingBelt, BindGroupLayoutEntry, CommandEncoder};

pub mod buffers;
pub use wgpu;

#[cfg(test)]
mod tests;

struct GpuDevice {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

static GPU_DEVICE: OnceCell<GpuDevice> = OnceCell::const_new();

async fn get_gpu_device() -> anyhow::Result<&'static GpuDevice> {
    GPU_DEVICE
        .get_or_try_init(|| async {
            // The instance is a handle to our GPU
            // Backends::all => Vulkan + Metal + DX12 + Browser WebGPU
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                ..Default::default()
            });

            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::default(),
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .ok_or_else(|| anyhow::anyhow!("Failed to request adapter"))?;

            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        required_features: wgpu::Features::empty(),
                        // WebGL doesn't support all of wgpu's features, so if
                        // we're building for the web, we'll have to disable some.
                        required_limits: if cfg!(target_arch = "wasm32") {
                            wgpu::Limits::downlevel_webgl2_defaults()
                        } else {
                            wgpu::Limits::default()
                        },
                        label: None,
                    },
                    None, // Trace path
                )
                .await?;
            Ok(GpuDevice { device, queue })
        })
        .await
}

pub struct Compute<A> {
    arg_buffers: Box<[wgpu::Buffer]>,

    staging_belt_size: u64,
    staging_belts: SegQueue<StagingBelt>,
    compute_pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,

    buffers: RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,

    phantom: std::marker::PhantomData<A>,
}

impl<A> Compute<A> {
    fn new_inner(
        shader_module_decsriptor: wgpu::ShaderModuleDescriptor<'_>,
        device: &wgpu::Device,
        arg_buffers: Box<[wgpu::Buffer]>,
        entries: Box<[BindGroupLayoutEntry]>,
        staging_belt_size: u64,
    ) -> Self {
        let module = device.create_shader_module(shader_module_decsriptor);

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &entries,
            label: Some("bind_group_layout"),
        });

        let entries: Box<[_]> = arg_buffers
            .iter()
            .enumerate()
            .map(|(i, buf)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: wgpu::BindingResource::Buffer(buf.as_entire_buffer_binding()),
            })
            .collect();

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &entries,
            label: Some("bind_group"),
        });

        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Compute Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute Pipeline"),
            layout: Some(&compute_pipeline_layout),
            module: &module,
            entry_point: "main",
        });

        Self {
            staging_belts: SegQueue::new(),
            staging_belt_size,
            arg_buffers,
            compute_pipeline,
            bind_group,
            buffers: RwLock::new(FxHashMap::default()),
            phantom: std::marker::PhantomData,
        }
    }

    fn new_pass_inner(
        &self,
        into_buffer: impl FnOnce(&mut CommandEncoder, &[wgpu::Buffer], &mut StagingBelt, &wgpu::Device),
    ) -> ComputePass<A> {
        let GpuDevice { device, .. } = get_gpu_device().now_or_never().unwrap().unwrap();

        let mut command_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });
        let mut stager = self
            .staging_belts
            .pop()
            .unwrap_or_else(|| StagingBelt::new(self.staging_belt_size));
        into_buffer(&mut command_encoder, &self.arg_buffers, &mut stager, device);
        ComputePass {
            command_encoder,
            compute: self,
            stager,
            workgroup_size: (1, 1, 1),
        }
    }

    pub async fn write_args_inner(
        &self,
        into_buffer: impl FnOnce(&mut CommandEncoder, &[wgpu::Buffer], &mut StagingBelt, &wgpu::Device),
    ) {
        let GpuDevice { device, queue } = get_gpu_device().now_or_never().unwrap().unwrap();
        let ComputePass {
            mut stager,
            command_encoder,
            ..
        } = self.new_pass_inner(into_buffer);
        stager.finish();
        let idx = queue.submit(std::iter::once(command_encoder.finish()));
        stager.recall();

        self.staging_belts.push(stager);

        tokio::task::spawn_blocking(|| {
            device.poll(wgpu::MaintainBase::WaitForSubmissionIndex(idx));
        })
        .await
        .unwrap();
    }
}

impl<B1> Compute<(B1,)>
where
    B1: CreateBuffer,
{
    pub async fn new(
        shader_module_decsriptor: wgpu::ShaderModuleDescriptor<'_>,
        type1: B1,
    ) -> anyhow::Result<Self> {
        let GpuDevice { device, .. } = get_gpu_device().await?;

        Ok(Self::new_inner(
            shader_module_decsriptor,
            device,
            Box::new([type1.into_buffer(0, device)]),
            Box::new([type1.into_layout(0)]),
            [type1.size()].into_iter().max().unwrap(),
        ))
    }

    pub fn new_pass(&self, arg1: impl BufferSource<B1::WriteType>) -> ComputePass<(B1,)> {
        self.new_pass_inner(|command_encoder, arg_buffers, stager, device| {
            arg1.into_buffer(command_encoder, &arg_buffers[0], stager, device);
        })
    }

    pub async fn write_args(&self, arg1: impl BufferSource<B1::WriteType>) {
        self.write_args_inner(|command_encoder, arg_buffers, stager, device| {
            arg1.into_buffer(command_encoder, &arg_buffers[0], stager, device);
        })
        .await;
    }
}

impl<B1, B2> Compute<(B1, B2)>
where
    B1: CreateBuffer,
    B2: CreateBuffer,
{
    pub async fn new(
        shader_module_decsriptor: wgpu::ShaderModuleDescriptor<'_>,
        type1: B1,
        type2: B2,
    ) -> anyhow::Result<Self> {
        let GpuDevice { device, .. } = get_gpu_device().await?;

        Ok(Self::new_inner(
            shader_module_decsriptor,
            device,
            Box::new([type1.into_buffer(0, device), type2.into_buffer(1, device)]),
            Box::new([type1.into_layout(0), type2.into_layout(1)]),
            [type1.size(), type2.size()].into_iter().max().unwrap(),
        ))
    }

    pub fn new_pass(
        &self,
        arg1: impl BufferSource<B1::WriteType>,
        arg2: impl BufferSource<B2::WriteType>,
    ) -> ComputePass<(B1, B2)> {
        self.new_pass_inner(|command_encoder, arg_buffers, stager, device| {
            arg1.into_buffer(command_encoder, &arg_buffers[0], stager, device);
            arg2.into_buffer(command_encoder, &arg_buffers[1], stager, device);
        })
    }

    pub async fn write_args(
        &self,
        arg1: impl BufferSource<B1::WriteType>,
        arg2: impl BufferSource<B2::WriteType>,
    ) {
        self.write_args_inner(|command_encoder, arg_buffers, stager, device| {
            arg1.into_buffer(command_encoder, &arg_buffers[0], stager, device);
            arg2.into_buffer(command_encoder, &arg_buffers[1], stager, device);
        })
        .await;
    }
}

impl<B1, B2, B3> Compute<(B1, B2, B3)>
where
    B1: CreateBuffer,
    B2: CreateBuffer,
    B3: CreateBuffer,
{
    pub async fn new(
        shader_module_decsriptor: wgpu::ShaderModuleDescriptor<'_>,
        type1: B1,
        type2: B2,
        type3: B3,
    ) -> anyhow::Result<Self> {
        let GpuDevice { device, .. } = get_gpu_device().await?;

        Ok(Self::new_inner(
            shader_module_decsriptor,
            device,
            Box::new([
                type1.into_buffer(0, device),
                type2.into_buffer(1, device),
                type3.into_buffer(2, device),
            ]),
            Box::new([
                type1.into_layout(0),
                type2.into_layout(1),
                type3.into_layout(2),
            ]),
            [type1.size(), type2.size(), type3.size()]
                .into_iter()
                .max()
                .unwrap(),
        ))
    }

    pub fn new_pass(
        &self,
        arg1: impl BufferSource<B1::WriteType>,
        arg2: impl BufferSource<B2::WriteType>,
        arg3: impl BufferSource<B3::WriteType>,
    ) -> ComputePass<(B1, B2, B3)> {
        self.new_pass_inner(|command_encoder, arg_buffers, stager, device| {
            arg1.into_buffer(command_encoder, &arg_buffers[0], stager, device);
            arg2.into_buffer(command_encoder, &arg_buffers[1], stager, device);
            arg3.into_buffer(command_encoder, &arg_buffers[2], stager, device);
        })
    }

    pub async fn write_args(
        &self,
        arg1: impl BufferSource<B1::WriteType>,
        arg2: impl BufferSource<B2::WriteType>,
        arg3: impl BufferSource<B3::WriteType>,
    ) {
        self.write_args_inner(|command_encoder, arg_buffers, stager, device| {
            arg1.into_buffer(command_encoder, &arg_buffers[0], stager, device);
            arg2.into_buffer(command_encoder, &arg_buffers[1], stager, device);
            arg3.into_buffer(command_encoder, &arg_buffers[2], stager, device);
        })
        .await;
    }
}
impl<B1, B2, B3, B4> Compute<(B1, B2, B3, B4)>
where
    B1: CreateBuffer,
    B2: CreateBuffer,
    B3: CreateBuffer,
    B4: CreateBuffer,
{
    pub async fn new(
        shader_module_decsriptor: wgpu::ShaderModuleDescriptor<'_>,
        type1: B1,
        type2: B2,
        type3: B3,
        type4: B4,
    ) -> anyhow::Result<Self> {
        let GpuDevice { device, .. } = get_gpu_device().await?;

        Ok(Self::new_inner(
            shader_module_decsriptor,
            device,
            Box::new([
                type1.into_buffer(0, device),
                type2.into_buffer(1, device),
                type3.into_buffer(2, device),
                type4.into_buffer(3, device),
            ]),
            Box::new([
                type1.into_layout(0),
                type2.into_layout(1),
                type3.into_layout(2),
                type4.into_layout(3),
            ]),
            [type1.size(), type2.size(), type3.size(), type4.size()]
                .into_iter()
                .max()
                .unwrap(),
        ))
    }

    pub fn new_pass(
        &self,
        arg1: impl BufferSource<B1::WriteType>,
        arg2: impl BufferSource<B2::WriteType>,
        arg3: impl BufferSource<B3::WriteType>,
        arg4: impl BufferSource<B4::WriteType>,
    ) -> ComputePass<(B1, B2, B3, B4)> {
        self.new_pass_inner(|command_encoder, arg_buffers, stager, device| {
            arg1.into_buffer(command_encoder, &arg_buffers[0], stager, device);
            arg2.into_buffer(command_encoder, &arg_buffers[1], stager, device);
            arg3.into_buffer(command_encoder, &arg_buffers[2], stager, device);
            arg4.into_buffer(command_encoder, &arg_buffers[3], stager, device);
        })
    }

    pub async fn write_args(
        &self,
        arg1: impl BufferSource<B1::WriteType>,
        arg2: impl BufferSource<B2::WriteType>,
        arg3: impl BufferSource<B3::WriteType>,
        arg4: impl BufferSource<B4::WriteType>,
    ) {
        self.write_args_inner(|command_encoder, arg_buffers, stager, device| {
            arg1.into_buffer(command_encoder, &arg_buffers[0], stager, device);
            arg2.into_buffer(command_encoder, &arg_buffers[1], stager, device);
            arg3.into_buffer(command_encoder, &arg_buffers[2], stager, device);
            arg4.into_buffer(command_encoder, &arg_buffers[3], stager, device);
        })
        .await;
    }
}
impl<B1, B2, B3, B4, B5> Compute<(B1, B2, B3, B4, B5)>
where
    B1: CreateBuffer,
    B2: CreateBuffer,
    B3: CreateBuffer,
    B4: CreateBuffer,
    B5: CreateBuffer,
{
    pub async fn new(
        shader_module_decsriptor: wgpu::ShaderModuleDescriptor<'_>,
        type1: B1,
        type2: B2,
        type3: B3,
        type4: B4,
        type5: B5,
    ) -> anyhow::Result<Self> {
        let GpuDevice { device, .. } = get_gpu_device().await?;

        Ok(Self::new_inner(
            shader_module_decsriptor,
            device,
            Box::new([
                type1.into_buffer(0, device),
                type2.into_buffer(1, device),
                type3.into_buffer(2, device),
                type4.into_buffer(3, device),
                type5.into_buffer(4, device),
            ]),
            Box::new([
                type1.into_layout(0),
                type2.into_layout(1),
                type3.into_layout(2),
                type4.into_layout(3),
                type5.into_layout(4),
            ]),
            [
                type1.size(),
                type2.size(),
                type3.size(),
                type4.size(),
                type5.size(),
            ]
            .into_iter()
            .max()
            .unwrap(),
        ))
    }

    pub fn new_pass(
        &self,
        arg1: impl BufferSource<B1::WriteType>,
        arg2: impl BufferSource<B2::WriteType>,
        arg3: impl BufferSource<B3::WriteType>,
        arg4: impl BufferSource<B4::WriteType>,
        arg5: impl BufferSource<B5::WriteType>,
    ) -> ComputePass<(B1, B2, B3, B4, B5)> {
        self.new_pass_inner(|command_encoder, arg_buffers, stager, device| {
            arg1.into_buffer(command_encoder, &arg_buffers[0], stager, device);
            arg2.into_buffer(command_encoder, &arg_buffers[1], stager, device);
            arg3.into_buffer(command_encoder, &arg_buffers[2], stager, device);
            arg4.into_buffer(command_encoder, &arg_buffers[3], stager, device);
            arg5.into_buffer(command_encoder, &arg_buffers[4], stager, device);
        })
    }

    pub async fn write_args(
        &self,
        arg1: impl BufferSource<B1::WriteType>,
        arg2: impl BufferSource<B2::WriteType>,
        arg3: impl BufferSource<B3::WriteType>,
        arg4: impl BufferSource<B4::WriteType>,
        arg5: impl BufferSource<B5::WriteType>,
    ) {
        self.write_args_inner(|command_encoder, arg_buffers, stager, device| {
            arg1.into_buffer(command_encoder, &arg_buffers[0], stager, device);
            arg2.into_buffer(command_encoder, &arg_buffers[1], stager, device);
            arg3.into_buffer(command_encoder, &arg_buffers[2], stager, device);
            arg4.into_buffer(command_encoder, &arg_buffers[3], stager, device);
            arg5.into_buffer(command_encoder, &arg_buffers[4], stager, device);
        })
        .await;
    }
}

pub struct ComputePass<'a, A> {
    command_encoder: CommandEncoder,
    compute: &'a Compute<A>,
    stager: StagingBelt,
    pub workgroup_size: (u32, u32, u32),
}

impl<'a, A> ComputePass<'a, A> {
    async fn call_inner<T>(
        mut self,
        after_dispatch: impl FnOnce(
            &mut CommandEncoder,
            &[wgpu::Buffer],
            &RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
            &wgpu::Device,
        ) -> T,
    ) -> (
        &'static wgpu::Device,
        &'a RwLock<FxHashMap<u64, SegQueue<wgpu::Buffer>>>,
        T,
    ) {
        let GpuDevice { queue, device } = get_gpu_device().await.unwrap();
        let Compute {
            compute_pipeline,
            bind_group,
            arg_buffers,
            buffers,
            staging_belts,
            ..
        } = self.compute;

        self.stager.finish();
        {
            let mut compute_pass =
                self.command_encoder
                    .begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("Render Pass"),
                        timestamp_writes: None,
                    });

            compute_pass.set_pipeline(compute_pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups(
                self.workgroup_size.0,
                self.workgroup_size.1,
                self.workgroup_size.2,
            );
        }

        let state = after_dispatch(&mut self.command_encoder, arg_buffers, &buffers, device);

        let idx = queue.submit(std::iter::once(self.command_encoder.finish()));
        self.stager.recall();
        staging_belts.push(self.stager);

        let _ = tokio::task::spawn_blocking(|| {
            device.poll(wgpu::MaintainBase::WaitForSubmissionIndex(idx));
        })
        .await;

        (device, buffers, state)
    }

    pub fn workgroup_size(mut self, x: u32, y: u32, z: u32) -> Self {
        self.workgroup_size = (x, y, z);
        self
    }
}

impl<'a, B1> ComputePass<'a, (B1,)>
where
    B1: ValidBufferType,
{
    pub async fn call(self, mut arg1: impl BufferDestination<B1::ReadType>) {
        let (device, buffers, (state1,)) = self
            .call_inner(|command_encoder, arg_buffers, buffers, device| {
                (arg1.enqueue(command_encoder, &arg_buffers[0], &buffers, device),)
            })
            .await;

        arg1.from_buffer(state1, device, buffers).await;
    }
}

impl<'a, B1, B2> ComputePass<'a, (B1, B2)>
where
    B1: ValidBufferType,
    B2: ValidBufferType,
{
    pub async fn call(
        self,
        mut arg1: impl BufferDestination<B1::ReadType>,
        mut arg2: impl BufferDestination<B2::ReadType>,
    ) {
        let (device, buffers, (state1, state2)) = self
            .call_inner(|command_encoder, arg_buffers, buffers, device| {
                (
                    arg1.enqueue(command_encoder, &arg_buffers[0], &buffers, device),
                    arg2.enqueue(command_encoder, &arg_buffers[1], &buffers, device),
                )
            })
            .await;

        arg1.from_buffer(state1, device, buffers).await;
        arg2.from_buffer(state2, device, buffers).await;
    }
}
impl<'a, B1, B2, B3> ComputePass<'a, (B1, B2, B3)>
where
    B1: ValidBufferType,
    B2: ValidBufferType,
    B3: ValidBufferType,
{
    pub async fn call(
        self,
        mut arg1: impl BufferDestination<B1::ReadType>,
        mut arg2: impl BufferDestination<B2::ReadType>,
        mut arg3: impl BufferDestination<B3::ReadType>,
    ) {
        let (device, buffers, (state1, state2, state3)) = self
            .call_inner(|command_encoder, arg_buffers, buffers, device| {
                (
                    arg1.enqueue(command_encoder, &arg_buffers[0], &buffers, device),
                    arg2.enqueue(command_encoder, &arg_buffers[1], &buffers, device),
                    arg3.enqueue(command_encoder, &arg_buffers[2], &buffers, device),
                )
            })
            .await;

        arg1.from_buffer(state1, device, buffers).await;
        arg2.from_buffer(state2, device, buffers).await;
        arg3.from_buffer(state3, device, buffers).await;
    }
}

impl<'a, B1, B2, B3, B4> ComputePass<'a, (B1, B2, B3, B4)>
where
    B1: ValidBufferType,
    B2: ValidBufferType,
    B3: ValidBufferType,
    B4: ValidBufferType,
{
    pub async fn call(
        self,
        mut arg1: impl BufferDestination<B1::ReadType>,
        mut arg2: impl BufferDestination<B2::ReadType>,
        mut arg3: impl BufferDestination<B3::ReadType>,
        mut arg4: impl BufferDestination<B4::ReadType>,
    ) {
        let (device, buffers, (state1, state2, state3, state4)) = self
            .call_inner(|command_encoder, arg_buffers, buffers, device| {
                (
                    arg1.enqueue(command_encoder, &arg_buffers[0], &buffers, device),
                    arg2.enqueue(command_encoder, &arg_buffers[1], &buffers, device),
                    arg3.enqueue(command_encoder, &arg_buffers[2], &buffers, device),
                    arg4.enqueue(command_encoder, &arg_buffers[3], &buffers, device),
                )
            })
            .await;

        arg1.from_buffer(state1, device, buffers).await;
        arg2.from_buffer(state2, device, buffers).await;
        arg3.from_buffer(state3, device, buffers).await;
        arg4.from_buffer(state4, device, buffers).await;
    }
}

impl<'a, B1, B2, B3, B4, B5> ComputePass<'a, (B1, B2, B3, B4, B5)>
where
    B1: ValidBufferType,
    B2: ValidBufferType,
    B3: ValidBufferType,
    B4: ValidBufferType,
    B5: ValidBufferType,
{
    pub async fn call(
        self,
        mut arg1: impl BufferDestination<B1::ReadType>,
        mut arg2: impl BufferDestination<B2::ReadType>,
        mut arg3: impl BufferDestination<B3::ReadType>,
        mut arg4: impl BufferDestination<B4::ReadType>,
        mut arg5: impl BufferDestination<B5::ReadType>,
    ) {
        let (device, buffers, (state1, state2, state3, state4, state5)) = self
            .call_inner(|command_encoder, arg_buffers, buffers, device| {
                (
                    arg1.enqueue(command_encoder, &arg_buffers[0], &buffers, device),
                    arg2.enqueue(command_encoder, &arg_buffers[1], &buffers, device),
                    arg3.enqueue(command_encoder, &arg_buffers[2], &buffers, device),
                    arg4.enqueue(command_encoder, &arg_buffers[3], &buffers, device),
                    arg5.enqueue(command_encoder, &arg_buffers[4], &buffers, device),
                )
            })
            .await;

        arg1.from_buffer(state1, device, buffers).await;
        arg2.from_buffer(state2, device, buffers).await;
        arg3.from_buffer(state3, device, buffers).await;
        arg4.from_buffer(state4, device, buffers).await;
        arg5.from_buffer(state5, device, buffers).await;
    }
}
