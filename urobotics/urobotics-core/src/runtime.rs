use std::{
    backtrace::Backtrace,
    borrow::Cow,
    future::Future,
    io::BufRead,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock, Weak,
    },
    thread::JoinHandle as SyncJoinHandle,
    time::{Duration, Instant},
};

use chrono::{DateTime, Datelike, Local, Timelike};
use crossbeam::queue::SegQueue;
use sysinfo::Pid;
use tokio::{
    runtime::{Builder as TokioBuilder, Handle},
    sync::watch,
    task::{AbortHandle, JoinHandle as AsyncJoinHandle},
};

use crate::{
    callbacks::{
        callee::Subscriber,
        caller::{Callbacks, SingleCallback},
    },
    logging::init_default_logger,
};

pub enum DumpPath {
    Default { application_name: Cow<'static, str> },
    Custom(PathBuf),
}

pub struct RuntimeBuilder {
    pub tokio_builder: TokioBuilder,
    pub dump_path: DumpPath,
    pub cpu_usage_warning_threshold: f32,
    pub max_persistent_drop_duration: Duration,
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
        self,
        main: impl FnOnce(RuntimeContext) -> F,
    ) -> Option<T> {
        let pid = std::process::id();

        let _ = rayon::ThreadPoolBuilder::default()
            .panic_handler(|_| {
                // Panics in rayon still get logged, but this prevents
                // the thread pool from aborting the entire process
            })
            .build_global();

        let ctrl_c_ref = CTRL_C_PUB.get_or_init(|| {
            let publisher = ImmutCallbacks::default();
            let publisher_ref = publisher.get_ref();

            ctrlc::set_handler(move || {
                publisher.call(EndCondition::CtrlC);
            })
            .expect("Failed to initialize Ctrl-C handler");

            publisher_ref
        });
        let end_sub = Subscriber::new(32);
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
        let run_ctx_inner = RuntimeContextInner {
            sync_persistent_threads: SegQueue::new(),
            async_persistent_threads: SegQueue::new(),
            exiting,
            end_pub: Callbacks::default(),
            dump_path,
            persistent_backtraces: SegQueue::new(),
            runtime_handle: runtime.handle().clone(),
        };
        let run_ctx_inner = Arc::new(run_ctx_inner);
        let main_run_ctx = RuntimeContext {
            inner: run_ctx_inner.clone(),
        };

        init_default_logger(&main_run_ctx);

        let cpu_fut = async {
            let mut sys = sysinfo::System::new();
            let mut last_cpu_check = Instant::now();
            let pid = Pid::from_u32(pid);
            loop {
                tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
                sys.refresh_cpu();
                sys.refresh_process(pid);
                if last_cpu_check.elapsed().as_secs() < 3 {
                    continue;
                }
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
                _ = cpu_fut => unreachable!(),
                end = end_sub.recv_or_never() => {
                    match end {
                        EndCondition::CtrlC => log::warn!("Ctrl-C received. Exiting..."),
                        EndCondition::QuitOnDrop => {}
                        EndCondition::AllContextDropped => log::warn!("All RuntimeContexts dropped. Exiting..."),
                        EndCondition::RuntimeDropped => unreachable!(),
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
        let mut end_pub = SingleCallback::default();
        end_pub.add_callback(Box::new(end_sub.create_mut_callback()));
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
        Self {
            tokio_builder: TokioBuilder::new_multi_thread(),
            dump_path: DumpPath::Default {
                application_name: Cow::Borrowed("default"),
            },
            cpu_usage_warning_threshold: 80.0,
            max_persistent_drop_duration: Duration::from_secs(5),
        }
    }
}

pub(crate) struct RuntimeContextInner {
    async_persistent_threads: SegQueue<AsyncJoinHandle<()>>,
    sync_persistent_threads: SegQueue<SyncJoinHandle<()>>,
    persistent_backtraces: SegQueue<Weak<Backtrace>>,
    exiting: watch::Receiver<bool>,
    end_pub: ImmutCallbacks<EndCondition>,
    dump_path: PathBuf,
    pub(crate) runtime_handle: Handle,
}

#[derive(Clone)]
pub struct RuntimeContext {
    pub(crate) inner: Arc<RuntimeContextInner>,
}

impl RuntimeContext {
    pub async fn wait_for_exit(&self) {
        let _ = self.inner.exiting.clone().changed().await;
    }
}

impl Drop for RuntimeContext {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.inner.end_pub.call(EndCondition::AllContextDropped);
        }
    }
}

impl RuntimeContextInner {
    fn spawn_persistent_sync(&self, f: impl FnOnce() + Send + 'static) {
        let backtrace = Arc::new(Backtrace::force_capture());
        let weak_backtrace = Arc::downgrade(&backtrace);
        let handle = self.runtime_handle.clone();

        let join_handle = std::thread::spawn(move || {
            let _guard = handle.enter();
            let _backtrace = backtrace;
            f();
        });

        self.sync_persistent_threads.push(join_handle);
        self.persistent_backtraces.push(weak_backtrace);
    }

    fn spawn_persistent_async(&self, f: impl Future<Output = ()> + Send + 'static) -> AbortHandle {
        let backtrace = Arc::new(Backtrace::force_capture());
        let weak_backtrace = Arc::downgrade(&backtrace);

        let join_handle = tokio::spawn(async move {
            let _backtrace = backtrace;
            f.await;
        });
        let abort = join_handle.abort_handle();

        self.async_persistent_threads.push(join_handle);
        self.persistent_backtraces.push(weak_backtrace);

        abort
    }

    fn is_runtime_exiting(&self) -> bool {
        *self.exiting.clone().borrow_and_update()
    }

    fn get_dump_path(&self) -> &Path {
        &self.dump_path
    }

    fn spawn_async<T: Send + 'static>(
        &self,
        f: impl Future<Output = T> + Send + 'static,
    ) -> AsyncJoinHandle<T> {
        self.runtime_handle.spawn(f)
    }
}

impl RuntimeContext {
    #[inline(always)]
    pub fn spawn_persistent_sync(&self, f: impl FnOnce() + Send + 'static) {
        self.inner.spawn_persistent_sync(f)
    }

    #[inline(always)]
    pub fn spawn_persistent_async(
        &self,
        f: impl Future<Output = ()> + Send + 'static,
    ) -> AbortHandle {
        self.inner.spawn_persistent_async(f)
    }

    #[inline(always)]
    pub fn is_runtime_exiting(&self) -> bool {
        self.inner.is_runtime_exiting()
    }

    #[inline(always)]
    pub fn get_dump_path(&self) -> &Path {
        self.inner.get_dump_path()
    }

    #[inline(always)]
    pub fn spawn_async<T: Send + 'static>(
        &self,
        f: impl Future<Output = T> + Send + 'static,
    ) -> AsyncJoinHandle<T> {
        self.inner.spawn_async(f)
    }
}

#[derive(Clone, Copy)]
enum EndCondition {
    CtrlC,
    QuitOnDrop,
    AllContextDropped,
    RuntimeDropped,
}

static CTRL_C_PUB: OnceLock<ImmutCallbacksRef<EndCondition>> = OnceLock::new();
