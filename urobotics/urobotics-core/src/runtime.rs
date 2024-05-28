use std::{
    backtrace::Backtrace,
    borrow::Cow,
    future::Future,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, Weak},
    thread::JoinHandle as SyncJoinHandle,
    time::{Duration, Instant},
};

use chrono::{DateTime, Datelike, Local, Timelike};
use crossbeam::queue::SegQueue;
use futures::{stream::FuturesUnordered, StreamExt};
use fxhash::FxHashSet;
use sysinfo::{Components, Pid};
use tokio::{
    runtime::{Builder as TokioBuilder, Handle},
    sync::watch,
    task::{AbortHandle, JoinHandle as AsyncJoinHandle},
};

use crate::{
    callbacks::{callee::Subscriber, caller::SharedCallbacksRef},
    define_shared_callbacks,
    logging::create_default_logger,
};

pub enum DumpPath {
    Default { application_name: Cow<'static, str> },
    Custom(PathBuf),
}

pub struct RuntimeBuilder {
    pub tokio_builder: TokioBuilder,
    pub dump_path: DumpPath,
    pub cpu_usage_warning_threshold: f32,
    pub temperature_warning_threshold: f32,
    pub max_persistent_drop_duration: Duration,
    pub ignore_component_temperature: FxHashSet<String>,
}

impl RuntimeBuilder {
    pub fn get_dump_path(&self) -> PathBuf {
        static START_DATE_TIME: OnceLock<DateTime<Local>> = OnceLock::new();

        match &self.dump_path {
            DumpPath::Default { application_name } => {
                let datetime = START_DATE_TIME.get_or_init(|| Local::now());
                let log_folder_name = format!(
                    "{}-{:0>2}-{:0>2}={:0>2}-{:0>2}-{:0>2}",
                    datetime.year(),
                    datetime.month(),
                    datetime.day(),
                    datetime.hour(),
                    datetime.minute(),
                    datetime.second(),
                );
                PathBuf::from("dump")
                    .join(application_name.deref())
                    .join(log_folder_name)
            }
            DumpPath::Custom(path) => path.clone(),
        }
    }

    pub fn start<T: Send + 'static, F: Future<Output = T> + Send + 'static>(
        mut self,
        main: impl FnOnce(RuntimeContext) -> F,
    ) -> Option<T> {
        let pid = std::process::id();

        let ctrl_c_ref = CTRL_C_PUB.get_or_init(|| {
            let publisher = EndCallbacks::default();
            let publisher_ref = publisher.get_ref();

            match ctrlc::try_set_handler(move || {
                publisher.call(EndCondition::CtrlC);
            }) {
                Ok(_) => {}
                Err(ctrlc::Error::MultipleHandlers) => {}
                Err(e) => {
                    log::error!("Failed to set Ctrl-C handler: {e:?}");
                }
            }

            publisher_ref
        });
        let end_sub = Subscriber::new_unbounded();
        ctrl_c_ref.add_callback(Box::new(end_sub.create_callback()));

        let dump_path = self.get_dump_path();

        if let Err(e) = std::fs::DirBuilder::new()
            .recursive(true)
            .create(&dump_path)
        {
            panic!("Failed to create dump directory {dump_path:?}: {e}")
        }

        let runtime = self.tokio_builder.build().unwrap();
        let (exiting_sender, exiting) = watch::channel(false);
        let end_pub = EndCallbacks::default();
        end_pub.add_callback(end_sub.create_callback());
        let run_ctx_inner = RuntimeContextInner {
            sync_persistent_threads: SegQueue::new(),
            async_persistent_threads: SegQueue::new(),
            exiting,
            end_pub,
            dump_path,
            persistent_backtraces: SegQueue::new(),
            runtime_handle: runtime.handle().clone(),
            waiting_for_exit: Arc::new(()),
        };
        let run_ctx_inner = Arc::new(run_ctx_inner);
        let main_run_ctx = RuntimeContext {
            inner: run_ctx_inner.clone(),
        };

        let _ = create_default_logger(&main_run_ctx).apply();

        let temp_fut = async {
            let mut components = Components::new_with_refreshed_list();

            let mut tasks = FuturesUnordered::new();

            for component in &mut components {
                if self
                    .ignore_component_temperature
                    .contains(component.label())
                {
                    continue;
                }
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

            tasks.next().await;
            std::future::pending().await
        };

        let cpu_usage_fut = async {
            let mut sys = sysinfo::System::new();
            let mut last_cpu_check = Instant::now();
            let pid = Pid::from_u32(pid);
            loop {
                tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
                if last_cpu_check.elapsed().as_secs() < 3 {
                    continue;
                }
                sys.refresh_cpu();
                sys.refresh_process(pid);
                let cpus = sys.cpus();
                let usage =
                    cpus.iter().map(sysinfo::Cpu::cpu_usage).sum::<f32>() / cpus.len() as f32;
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
        };

        let result = runtime.block_on(async {
            log::info!("Runtime started with pid: {pid}");
            tokio::select! {
                res = tokio::spawn(main(main_run_ctx)) => {
                    log::warn!("Exiting from main");
                    res.ok()
                }
                _ = cpu_usage_fut => unreachable!(),
                _ = temp_fut => unreachable!(),
                end = end_sub.recv_or_never() => {
                    match end {
                        EndCondition::CtrlC => log::warn!("Ctrl-C received. Exiting..."),
                        EndCondition::AllContextDropped => log::warn!("All RuntimeContexts dropped. Exiting..."),
                        EndCondition::RuntimeDropped => unreachable!(),
                        EndCondition::EndRequested => log::warn!("End requested. Exiting..."),
                    }
                    None
                }
            }
        });
        let _ = exiting_sender.send(true);

        let run_ctx_inner2 = run_ctx_inner.clone();

        std::thread::spawn(move || {
            std::thread::sleep(self.max_persistent_drop_duration);
            while let Some(backtrace) = run_ctx_inner2.persistent_backtraces.pop() {
                if let Some(backtrace) = backtrace.upgrade() {
                    log::warn!("The following persistent thread has not exited yet:\n{backtrace}");
                }
            }
        });
        let end_pub = EndCallbacks::default();
        end_pub.add_callback(end_sub.create_callback());
        let dropper = std::thread::spawn(move || {
            runtime.block_on(async {
                while let Some(handle) = run_ctx_inner.async_persistent_threads.pop() {
                    if let Err(e) = handle.await {
                        log::error!("Failed to join thread: {e:?}");
                    }
                }
            });
            drop(runtime);
            while let Some(handle) = run_ctx_inner.sync_persistent_threads.pop() {
                if let Err(e) = handle.join() {
                    log::error!("Failed to join thread: {e:?}");
                }
            }
            end_pub.call(EndCondition::RuntimeDropped);
        });

        let runtime = TokioBuilder::new_current_thread().build().unwrap();

        runtime.block_on(async {
            loop {
                match end_sub.recv_or_never().await {
                    EndCondition::CtrlC => log::warn!("Ctrl-C received. Force exiting..."),
                    EndCondition::RuntimeDropped => {
                        let _ = dropper.join();
                    }
                    _ => continue,
                }
                break;
            }
        });

        result
    }
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        let mut tokio_builder = TokioBuilder::new_multi_thread();
        tokio_builder.enable_all();
        Self {
            tokio_builder,
            dump_path: DumpPath::Default {
                application_name: Cow::Borrowed("default"),
            },
            cpu_usage_warning_threshold: 80.0,
            max_persistent_drop_duration: Duration::from_secs(5),
            temperature_warning_threshold: 80.0,
            ignore_component_temperature: FxHashSet::default(),
        }
    }
}

define_shared_callbacks!(EndCallbacks => Fn(condition: EndCondition) + Send + Sync);

pub(crate) struct RuntimeContextInner {
    async_persistent_threads: SegQueue<AsyncJoinHandle<()>>,
    sync_persistent_threads: SegQueue<SyncJoinHandle<()>>,
    persistent_backtraces: SegQueue<Weak<Backtrace>>,
    exiting: watch::Receiver<bool>,
    end_pub: EndCallbacks,
    dump_path: PathBuf,
    waiting_for_exit: Arc<()>,
    pub(crate) runtime_handle: Handle,
}

#[derive(Clone)]
pub struct RuntimeContext {
    pub(crate) inner: Arc<RuntimeContextInner>,
}

impl Drop for RuntimeContext {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == Arc::strong_count(&self.inner.waiting_for_exit) {
            self.inner.end_pub.call(EndCondition::AllContextDropped);
        }
    }
}

impl RuntimeContext {
    pub fn spawn_persistent_sync(&self, f: impl FnOnce() + Send + 'static) {
        let backtrace = Arc::new(Backtrace::force_capture());
        let weak_backtrace = Arc::downgrade(&backtrace);
        let handle = self.inner.runtime_handle.clone();

        let join_handle = std::thread::spawn(move || {
            let _guard = handle.enter();
            let _backtrace = backtrace;
            f();
        });

        self.inner.sync_persistent_threads.push(join_handle);
        self.inner.persistent_backtraces.push(weak_backtrace);
    }

    pub fn spawn_persistent_async(
        &self,
        f: impl Future<Output = ()> + Send + 'static,
    ) -> AbortHandle {
        let backtrace = Arc::new(Backtrace::force_capture());
        let weak_backtrace = Arc::downgrade(&backtrace);

        let join_handle = tokio::spawn(async move {
            let _backtrace = backtrace;
            f.await;
        });
        let abort = join_handle.abort_handle();

        self.inner.async_persistent_threads.push(join_handle);
        self.inner.persistent_backtraces.push(weak_backtrace);

        abort
    }

    pub fn is_runtime_exiting(&self) -> bool {
        *self.inner.exiting.clone().borrow_and_update()
    }

    pub fn get_dump_path(&self) -> &Path {
        &self.inner.dump_path
    }

    pub fn spawn_async<T: Send + 'static>(
        &self,
        f: impl Future<Output = T> + Send + 'static,
    ) -> AsyncJoinHandle<T> {
        self.inner.runtime_handle.spawn(f)
    }

    pub fn end_runtime(&self) {
        self.inner.end_pub.call(EndCondition::EndRequested);
    }

    pub async fn wait_for_exit(&self) {
        let _waiting = self.inner.waiting_for_exit.clone();
        if Arc::strong_count(&self.inner) == Arc::strong_count(&self.inner.waiting_for_exit) {
            self.inner.end_pub.call(EndCondition::AllContextDropped);
        }
        let _ = self.inner.exiting.clone().changed().await;
    }
}

#[derive(Clone, Copy)]
enum EndCondition {
    CtrlC,
    AllContextDropped,
    RuntimeDropped,
    EndRequested,
}

static CTRL_C_PUB: OnceLock<SharedCallbacksRef<dyn Fn(EndCondition) + Send + Sync>> =
    OnceLock::new();
