use std::marker::PhantomData;

use nalgebra::Vector3;

use crate::{
    buffers::GpuWriteLock,
    get_device,
    shader::{ComputeFn, GpuBufferTupleList},
    GpuDevice,
};

pub struct ComputePipeline<S, const SIZE: usize> {
    compute_pipelines: [wgpu::ComputePipeline; SIZE],
    pub workgroups: [Vector3<u32>; SIZE],
    phantom: PhantomData<fn() -> S>,
}

impl<S: GpuBufferTupleList, const SIZE: usize> ComputePipeline<S, SIZE> {
    pub fn new(compute_fns: [&ComputeFn<S>; SIZE]) -> Self {
        let GpuDevice { device, .. } = get_device();
        let layout_entries = S::create_layout_entries();

        let bind_group_layouts: Box<[_]> = layout_entries
            .iter()
            .map(|entries| {
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries,
                    label: None,
                })
            })
            .collect();
        let bind_group_layouts: Box<[_]> = bind_group_layouts.iter().collect();

        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &bind_group_layouts,
                push_constant_ranges: &[],
            });

        let compute_pipelines = compute_fns.map(|compute_fn| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: Some(&compute_pipeline_layout),
                module: &compute_fn.shader,
                entry_point: compute_fn.name,
            })
        });

        Self {
            compute_pipelines,
            workgroups: [Vector3::new(1, 1, 1); SIZE],
            phantom: PhantomData,
        }
    }

    pub fn new_pass<'a, 'b>(&'a self, bind_group_fn: impl FnOnce(GpuWriteLock) -> &'b mut S) -> ComputePass<'a, 'b, S, SIZE> {
        let GpuDevice { device, .. } = get_device();
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let bind_group_set = bind_group_fn(GpuWriteLock {
            encoder: &mut encoder,
            device,
        });
        ComputePass {
            bind_group_set,
            encoder,
            compute_pipelines: &self.compute_pipelines,
            workgroups: self.workgroups,
        }
    }
}

pub struct ComputePass<'a, 'b, S, const SIZE: usize> {
    bind_group_set: &'b mut S,
    encoder: wgpu::CommandEncoder,
    compute_pipelines: &'a [wgpu::ComputePipeline; SIZE],
    pub workgroups: [Vector3<u32>; SIZE],
}

impl<'a, 'b, S: GpuBufferTupleList, const SIZE: usize> ComputePass<'a, 'b, S, SIZE> {
    pub fn finish(mut self) {
        let GpuDevice { queue, device } = get_device();
        for (pipeline, workgroups) in self.compute_pipelines.iter().zip(self.workgroups) {
            let mut compute_pass = self
                .encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: None,
                });

            compute_pass.set_pipeline(pipeline);
            self.bind_group_set.set_into_compute_pass(&mut compute_pass);
            compute_pass.dispatch_workgroups(workgroups.x, workgroups.y, workgroups.z);
        }
        self.bind_group_set.pre_submission(&mut self.encoder);
        let idx = queue.submit(Some(self.encoder.finish()));
        self.bind_group_set.post_submission(device, idx);
    }
}
