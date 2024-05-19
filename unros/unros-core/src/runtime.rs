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
    task::JoinHandle as AsyncJoinHandle,
};

use crate::{
    logging::init_default_logger, pubsub::{callee::Subscriber, caller::{Callbacks, ImmutCallbacks, ImmutCallbacksRef, SingleCallback}}, setup_logging
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
    name: Arc<str>,
    pub quit_on_drop: bool,
}

impl RuntimeContext {
    pub fn clone_new_name(&self, name: impl Into<Arc<str>>) -> Self {
        Self {
            inner: self.inner.clone(),
            name: name.into(),
            quit_on_drop: self.quit_on_drop,
        }
    }

    pub async fn wait_for_exit(&self) {
        let _ = self.inner.exiting.clone().changed().await;
    }
}

impl Drop for RuntimeContext {
    fn drop(&mut self) {
        setup_logging!(self);
        if Arc::strong_count(&self.inner) == 1 {
            self.inner.end_pub.call(EndCondition::AllContextDropped);
        } else if self.quit_on_drop {
            warn!("Exiting from {}...", Backtrace::capture());
            self.inner.end_pub.call(EndCondition::QuitOnDrop);
        }
    }
}

pub struct MainRuntimeContext {
    inner: Arc<RuntimeContextInner>,
}

impl MainRuntimeContext {
    pub fn make_context(&self, name: impl Into<Arc<str>>) -> RuntimeContext {
        RuntimeContext {
            inner: self.inner.clone(),
            name: name.into(),
            quit_on_drop: false,
        }
    }

    pub async fn wait_for_exit(self) {
        let _ = self.inner.exiting.clone().changed().await;
    }

    pub async fn wait_for_exit_with_repl(self, mut repl: impl FnMut(&str) + Send + 'static) {
        HAS_REPL.store(true, Ordering::Release);
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut stdin = stdin.lock();
            let mut buffer = String::new();

            loop {
                buffer.clear();
                if stdin.read_line(&mut buffer).is_err() {
                    break;
                }
                repl(buffer.trim());
            }
        });
        let _ = self.inner.exiting.clone().changed().await;
    }
}

pub struct AbortHandle {
    pub(crate) inner: tokio::task::AbortHandle,
    pub(crate) spawn_context: Arc<str>,
}

impl AbortHandle {
    pub fn abort(&self, context: &RuntimeContext) {
        self.inner.abort();
        setup_logging!(context);
        info!("Aborted {} from {}", self.spawn_context, context.name);
    }
}

pub trait RuntimeContextExt: Send + Sync {
    fn spawn_persistent_sync(&self, f: impl FnOnce() + Send + 'static);
    fn spawn_persistent_async(&self, f: impl Future<Output = ()> + Send + 'static) -> AbortHandle;
    fn get_name(&self) -> &str;
    fn get_dump_path(&self) -> &Path;
    fn is_runtime_exiting(&self) -> bool;
    fn spawn_async<T: Send + 'static>(
        &self,
        f: impl Future<Output = T> + Send + 'static,
    ) -> AsyncJoinHandle<T>;
}

impl RuntimeContextExt for RuntimeContext {
    fn spawn_persistent_sync(&self, f: impl FnOnce() + Send + 'static) {
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

    fn spawn_persistent_async(&self, f: impl Future<Output = ()> + Send + 'static) -> AbortHandle {
        let backtrace = Arc::new(Backtrace::force_capture());
        let weak_backtrace = Arc::downgrade(&backtrace);

        let join_handle = tokio::spawn(async move {
            let _backtrace = backtrace;
            f.await;
        });
        let abort = join_handle.abort_handle();

        self.inner.async_persistent_threads.push(join_handle);
        self.inner.persistent_backtraces.push(weak_backtrace);

        AbortHandle {
            inner: abort,
            spawn_context: self.name.clone(),
        }
    }

    fn is_runtime_exiting(&self) -> bool {
        *self.inner.exiting.clone().borrow_and_update()
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn get_dump_path(&self) -> &Path {
        &self.inner.dump_path
    }

    fn spawn_async<T: Send + 'static>(
        &self,
        f: impl Future<Output = T> + Send + 'static,
    ) -> AsyncJoinHandle<T> {
        self.inner.runtime_handle.spawn(f)
    }
}

impl RuntimeContextExt for MainRuntimeContext {
    fn spawn_persistent_sync(&self, f: impl FnOnce() + Send + 'static) {
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

    fn spawn_persistent_async(&self, f: impl Future<Output = ()> + Send + 'static) -> AbortHandle {
        let backtrace = Arc::new(Backtrace::force_capture());
        let weak_backtrace = Arc::downgrade(&backtrace);

        let join_handle = tokio::spawn(async move {
            let _backtrace = backtrace;
            f.await;
        });
        let abort = join_handle.abort_handle();

        self.inner.async_persistent_threads.push(join_handle);
        self.inner.persistent_backtraces.push(weak_backtrace);

        AbortHandle {
            inner: abort,
            spawn_context: self.get_name().into(),
        }
    }

    fn is_runtime_exiting(&self) -> bool {
        *self.inner.exiting.clone().borrow_and_update()
    }

    fn get_name(&self) -> &str {
        "main"
    }

    fn get_dump_path(&self) -> &Path {
        &self.inner.dump_path
    }

    fn spawn_async<T: Send + 'static>(
        &self,
        f: impl Future<Output = T> + Send + 'static,
    ) -> AsyncJoinHandle<T> {
        self.inner.runtime_handle.spawn(f)
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
static HAS_REPL: AtomicBool = AtomicBool::new(false);

pub fn has_repl() -> bool {
    HAS_REPL.load(Ordering::Acquire)
}

pub fn start_unros_runtime<T: Send + 'static, F: Future<Output = T> + Send + 'static>(
    main: impl FnOnce(MainRuntimeContext) -> F,
    builder: impl FnOnce(&mut RuntimeBuilder),
) -> Option<T> {
    let pid = std::process::id();

    let _ = rayon::ThreadPoolBuilder::default()
        .panic_handler(|_| {
            // Panics in rayon still get logged, but this prevents
            // the thread pool from aborting the entire process
        })
        .build_global();

    let mut tokio_builder = TokioBuilder::new_multi_thread();
    tokio_builder.enable_all();
    let mut runtime_builder = RuntimeBuilder {
        tokio_builder,
        dump_path: DumpPath::Default {
            application_name: Cow::Borrowed("default"),
        },
        cpu_usage_warning_threshold: 80.0,
        max_persistent_drop_duration: Duration::from_secs(5),
    };
    builder(&mut runtime_builder);

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

    let dump_path = runtime_builder.get_dump_path();

    if let Err(e) = std::fs::DirBuilder::new()
        .recursive(true)
        .create(&dump_path)
    {
        panic!("Failed to create dump directory {dump_path:?}: {e}")
    }

    let runtime = runtime_builder.tokio_builder.build().unwrap();
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
    let main_run_ctx = MainRuntimeContext {
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
            let usage = cpus.iter().map(sysinfo::Cpu::cpu_usage).sum::<f32>() / cpus.len() as f32;
            if usage >= runtime_builder.cpu_usage_warning_threshold {
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
                HAS_REPL.store(false, Ordering::Release);
                log::warn!("Exiting from main");
                res.ok()
            }
            _ = cpu_fut => unreachable!(),
            end = end_sub.recv_or_never() => {
                HAS_REPL.store(false, Ordering::Release);
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
        std::thread::sleep(runtime_builder.max_persistent_drop_duration);
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
