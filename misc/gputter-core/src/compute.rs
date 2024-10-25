use std::marker::PhantomData;

use crate::{get_device, shader::{ComputeFn, GpuBufferTupleList}, GpuDevice};

// pub struct PendingComputePipeline<S> {
//     bind_group_set: S,
//     compute_pipeline_layout: wgpu::PipelineLayout,
// }


// impl<S: GpuBufferTupleList> PendingComputePipeline<S> {

//     pub fn finish<const SIZE: usize>(self, compute_fns: [&ComputeFn<S>; SIZE]) -> ComputePipeline<S, SIZE> {
//         let GpuDevice { device, .. } = get_device();
//         let compute_pipelines = compute_fns
//             .map(|compute_fn| {
//                 device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
//                     label: None,
//                     layout: Some(&self.compute_pipeline_layout),
//                     module: &compute_fn.shader,
//                     entry_point: compute_fn.name,
//                 })
//             });

//         ComputePipeline {
//             bind_group_set: self.bind_group_set,
//             compute_pipelines,
//             workgroups: [(1, 1, 1); SIZE],
//         }
//     }
// }

pub struct ComputePipeline<S, const SIZE: usize> {
    compute_pipelines: [wgpu::ComputePipeline; SIZE],
    pub workgroups: [(u32, u32, u32); SIZE],
    phantom: PhantomData<fn() -> S>
}

impl<S: GpuBufferTupleList, const SIZE: usize> ComputePipeline<S, SIZE> {
    pub fn new(compute_fns: [&ComputeFn<S>; SIZE]) -> Self {
        let GpuDevice { device, .. } = get_device();
        let layout_entries = S::create_layout_entries();
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &layout_entries,
            label: None,
        });

        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        

        let compute_pipelines = compute_fns
            .map(|compute_fn| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: None,
                    layout: Some(&compute_pipeline_layout),
                    module: &compute_fn.shader,
                    entry_point: compute_fn.name,
                })
            });

        Self {
            compute_pipelines,
            workgroups: [(1, 1, 1); SIZE],
            phantom: PhantomData
        }
    }

    pub fn new_pass(&self, bind_group_fn: impl FnOnce() -> S) {
        let GpuDevice { device, .. } = get_device();
        let mut command_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: None,
        });
        for (pipeline, workgroups) in self.compute_pipelines.iter().zip(self.workgroups) {
            let mut compute_pass =
                command_encoder
                    .begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("Render Pass"),
                        timestamp_writes: None,
                    });

            compute_pass.set_pipeline(pipeline);
            self.bind_group_set.set_into_compute_pass(&mut compute_pass);
            compute_pass.dispatch_workgroups(
                workgroups.0,
                workgroups.1,
                workgroups.2,
            );
        }
    }
}

pub struct ComputePass<'a, S, const SIZE: usize> {
    bind_group_set: S,
    compute_pipelines: [wgpu::ComputePipeline; SIZE],
    pub workgroups: [(u32, u32, u32); SIZE],
}