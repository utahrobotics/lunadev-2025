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

/// A simple compute shader with only one bind group.
/// 
/// More specifically, this represents a host's view of a compute shader, and this view is statically
/// verified to be correct. This helps to prevent several mistakes and provides the most amount of
/// information to the shader compiler, allowing for better optimizations.
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
            workgroups_count: (1, 1, 1),
        }
    }

    async fn write_args_inner(
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

macro_rules! compute_impl {
    ($($buf_type: ident $buf_arg: ident $index: literal,)+) => {
        impl<$($buf_type,)+> Compute<($($buf_type,)+)>
        where
            $($buf_type: CreateBuffer,)+
        {
            /// Create a new compute shader with the given arguments.
            /// 
            /// Each argument corresponds to a `var` binding in the shader, which corresponds to a Storage/Uniform buffer.
            /// Buffers are initialized to 0.
            pub async fn new(
                shader_module_decsriptor: wgpu::ShaderModuleDescriptor<'_>,
                $($buf_arg: $buf_type,)+
            ) -> anyhow::Result<Self> {
                let GpuDevice { device, .. } = get_gpu_device().await?;

                Ok(Self::new_inner(
                    shader_module_decsriptor,
                    device,
                    Box::new([
                        $($buf_arg.into_buffer($index, device),)+
                    ]),
                    Box::new([
                        $($buf_arg.into_layout($index),)+
                    ]),
                    [
                        $($buf_arg.size(),)+
                    ].into_iter().max().unwrap(),
                ))
            }

            /// Create a new compute pass with the given arguments.
            /// 
            /// A pass represents one computation of the shader, and the arguments are written to the GPU before the pass is executed.
            /// The given arguments are copied immediately, with the exception of `OpaqueBuffer`, which is only read when the pass is executed.
            pub fn new_pass(
                &self,
                $($buf_arg: impl BufferSource<$buf_type::WriteType>,)+
            ) -> ComputePass<($($buf_type,)+)> {
                self.new_pass_inner(|command_encoder, arg_buffers, stager, device| {
                    $($buf_arg.into_buffer(command_encoder, &arg_buffers[$index], stager, device);)+
                })
            }

            /// Write the given arguments to the GPU.
            /// 
            /// This is a way to write to the buffers on the GPU without running the shader.
            pub async fn write_args(
                &self,
                $($buf_arg: impl BufferSource<$buf_type::WriteType>,)+
            ) {
                self.write_args_inner(|command_encoder, arg_buffers, stager, device| {
                    $($buf_arg.into_buffer(command_encoder, &arg_buffers[$index], stager, device);)+
                })
                .await;
            }
        }
    }
}

compute_impl!(
    B1 arg1 0,
);

compute_impl!(
    B1 arg1 0,
    B2 arg2 1,
);

compute_impl!(
    B1 arg1 0,
    B2 arg2 1,
    B3 arg3 2,
);

compute_impl!(
    B1 arg1 0,
    B2 arg2 1,
    B3 arg3 2,
    B4 arg4 3,
);

compute_impl!(
    B1 arg1 0,
    B2 arg2 1,
    B3 arg3 2,
    B4 arg4 3,
    B5 arg5 4,
);

compute_impl!(
    B1 arg1 0,
    B2 arg2 1,
    B3 arg3 2,
    B4 arg4 3,
    B5 arg5 4,
    B6 arg6 5,
);

/// A pending compute shader pass.
pub struct ComputePass<'a, A> {
    command_encoder: CommandEncoder,
    compute: &'a Compute<A>,
    stager: StagingBelt,
    /// The number of workgroups to dispatch.
    pub workgroups_count: (u32, u32, u32),
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
                self.workgroups_count.0,
                self.workgroups_count.1,
                self.workgroups_count.2,
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

    /// Sets the number of workgroups.
    /// 
    /// This is just a convenience method. Setting the field directly is also possible.
    pub fn workgroups_count(mut self, x: u32, y: u32, z: u32) -> Self {
        self.workgroups_count = (x, y, z);
        self
    }
}

macro_rules! compute_pass_impl {
    ($($buf_type: ident $buf_arg: ident $index: literal $state: ident,)+) => {
        impl<'a, $($buf_type,)+> ComputePass<'a, ($($buf_type,)+)>
        where
            $($buf_type: ValidBufferType,)+
        {
            /// Executes the compute shader pass.
            /// 
            /// The final state of each `var` binding (aka Storage/Uniform buffers) is written to the given arguments.
            pub async fn call(
                self,
                $(mut $buf_arg: impl BufferDestination<$buf_type::ReadType>,)+
            ) {
                let (device, buffers, ($($state,)+)) = self
                    .call_inner(|command_encoder, arg_buffers, buffers, device| {
                        (
                            $($buf_arg.enqueue(command_encoder, &arg_buffers[$index], &buffers, device),)+
                        )
                    })
                    .await;

                $($buf_arg.from_buffer($state, device, buffers).await;)+
            }
        }
    }
}

compute_pass_impl!(
    B1 arg1 0 state1,
);

compute_pass_impl!(
    B1 arg1 0 state1,
    B2 arg2 1 state2,
);

compute_pass_impl!(
    B1 arg1 0 state1,
    B2 arg2 1 state2,
    B3 arg3 2 state3,
);

compute_pass_impl!(
    B1 arg1 0 state1,
    B2 arg2 1 state2,
    B3 arg3 2 state3,
    B4 arg4 3 state4,
);

compute_pass_impl!(
    B1 arg1 0 state1,
    B2 arg2 1 state2,
    B3 arg3 2 state3,
    B4 arg4 3 state4,
    B5 arg5 4 state5,
);