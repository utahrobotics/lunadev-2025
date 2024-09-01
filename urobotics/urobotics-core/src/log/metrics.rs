//! Utilities for logging metrics such as CPU temperature and CPU usage.

use std::time::Instant;

use futures::{stream::FuturesUnordered, StreamExt};
use fxhash::FxHashSet;
use sysinfo::{Components, Pid};
use tasker::task::AsyncTask;

/// Configuration for the temperature monitoring task.
#[derive(Clone)]
pub struct Temperature {
    /// The minimum temperature (in celsius) that will trigger a warning.
    pub temperature_warning_threshold: f32,
    /// Component names to ignore when checking temperature.
    pub ignore_component_temperature: FxHashSet<String>,
}

impl AsyncTask for Temperature {
    type Output = !;

    async fn run(self) -> Self::Output {
        let mut components = Components::new_with_refreshed_list();
        let mut tasks = FuturesUnordered::new();

        for component in components.list_mut() {
            if self
                .ignore_component_temperature
                .contains(component.label())
            {
                continue;
            }
            component.refresh();
            tasks.push(async {
                let mut last_temp_check = Instant::now();
                loop {
                    tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
                    if last_temp_check.elapsed().as_secs() < 3 {
                        continue;
                    }
                    component.refresh();
                    let temp = component.temperature();
                    if temp >= self.temperature_warning_threshold {
                        log::warn!("{} at {temp:.1} °C", component.label());
                        last_temp_check = Instant::now();
                    }
                }
            });
        }

        match tasks.next().await {
            None => {
                std::future::pending::<()>().await;
                unreachable!()
            }
            Some(x) => x,
        }
    }
}

/// Configuration for the CPU usage monitoring task.
#[derive(Clone, Copy)]
pub struct CpuUsage {
    /// The minimum CPU usage (out of 100) that will trigger a warning.
    pub cpu_usage_warning_threshold: f32,
}

impl AsyncTask for CpuUsage {
    type Output = !;

    async fn run(self) -> Self::Output {
        let mut sys = sysinfo::System::new();
        let mut last_cpu_check = Instant::now();
        let pid = Pid::from_u32(std::process::id());

        loop {
            tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
            if last_cpu_check.elapsed().as_secs() < 3 {
                continue;
            }
            sys.refresh_cpu();
            sys.refresh_process(pid);
            let cpus = sys.cpus();
            let usage = cpus.iter().map(sysinfo::Cpu::cpu_usage).sum::<f32>() / cpus.len() as f32;
            if usage >= self.cpu_usage_warning_threshold {
                if let Some(proc) = sys.process(pid) {
                    log::warn!(
                        "CPU Usage at {usage:.1}%. Process Usage: {:.1}%",
                        proc.cpu_usage() / cpus.len() as f32
                    );
                } else {
                    log::warn!("CPU Usage at {usage:.1}%. Err checking process");
                }
                last_cpu_check = Instant::now();
            }
        }
    }
}
