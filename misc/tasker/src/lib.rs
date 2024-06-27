// #![feature(never_type)]
#![feature(unboxed_closures)]
#![feature(tuple_trait)]
// #![feature(const_option)]

use std::{
    backtrace::Backtrace, cell::RefCell, future::Future, ops::Deref, sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock,
    }, time::Duration
};

use crossbeam::queue::SegQueue;
use parking_lot::{Mutex, RwLock, RwLockWriteGuard};
use tokio::{runtime::Handle, sync::Notify};
pub use tokio;
pub use parking_lot;

pub mod callbacks;
pub mod task;

pub struct TokioRuntimeConfig {
    pub num_threads: Option<usize>,
    pub shutdown_delayed_warning: Duration,
    pub thread_delayed_warning: Duration,
}

static TOKIO_RUNTIME_CONFIG: LazyLock<Mutex<Arc<TokioRuntimeConfig>>> = LazyLock::new(|| {
    Mutex::new(Arc::new(TokioRuntimeConfig {
        num_threads: None,
        shutdown_delayed_warning: Duration::from_secs(5),
        thread_delayed_warning: Duration::from_secs(5),
    }))
});

pub fn set_tokio_runtime_config(config: impl Into<Arc<TokioRuntimeConfig>>) {
    *TOKIO_RUNTIME_CONFIG.lock() = config.into();
}

pub fn get_tokio_runtime_config() -> Arc<TokioRuntimeConfig> {
    TOKIO_RUNTIME_CONFIG.lock().clone()
}

struct TokioRuntimeHandle {
    handle: Handle,
    runtime_ended: Arc<NotifyFlag>,
    end_runtime: Arc<NotifyFlag>,
    attached_threads: Arc<SegQueue<Arc<RuntimeDropGuardInner>>>,
}

static TOKIO_RUNTIME_HANDLE: RwLock<Option<TokioRuntimeHandle>> = RwLock::new(None);


fn with_tokio_runtime_handle<T>(f: impl FnOnce(&TokioRuntimeHandle) -> T) -> T {
    {
        let reader = TOKIO_RUNTIME_HANDLE.read();
        if let Some(handle) = reader.as_ref() {
            return f(handle);
        }
    }
    let mut writer = TOKIO_RUNTIME_HANDLE.write();
    if writer.is_some() {
        f(RwLockWriteGuard::downgrade(writer)
            .deref()
            .as_ref()
            .unwrap())
    } else {
        let config = get_tokio_runtime_config();
        let mut builder = tokio::runtime::Builder::new_multi_thread();

        if let Some(num_threads) = config.num_threads {
            builder.worker_threads(num_threads);
        }

        let runtime = builder.enable_all().build().unwrap();

        let handle = runtime.handle().clone();
        let runtime_ended: Arc<NotifyFlag> = Arc::default();
        let end_runtime: Arc<NotifyFlag> = Arc::default();
        let attached_threads: Arc<SegQueue<Arc<RuntimeDropGuardInner>>> = Arc::default();
        let tokio_runtime_handle = TokioRuntimeHandle {
            handle,
            runtime_ended: runtime_ended.clone(),
            end_runtime: end_runtime.clone(),
            attached_threads: attached_threads.clone(),
        };
        writer.replace(tokio_runtime_handle);

        std::thread::spawn(move || {
            runtime.block_on(async {
                end_runtime.notified().await;
                let mut attached_threads: Vec<RuntimeDropGuardInner> = Vec::with_capacity(attached_threads.len());
                while let Some(guard) = attached_threads.pop() {
                    attached_threads.push(guard);
                }
                tokio::select! {
                    () = async {
                        for drop_notify in &attached_threads {
                            drop_notify.notify.notified().await;
                        }
                    } => {}
                    () = tokio::time::sleep(config.thread_delayed_warning) => {
                        let mut backtraces = String::new();
                        for drop_notify in &attached_threads {
                            backtraces.push_str(&format!("{:?}\n\n", drop_notify.backtrace));
                        }
                        log::warn!(
                            "The following threads have not ended after {:.1} seconds\n\n{backtraces}",
                            config.thread_delayed_warning.as_secs_f32()
                        );
                        for drop_notify in &attached_threads {
                            drop_notify.notify.notified().await;
                        }
                    }
                }
            });
            let runtime_ended2 = runtime_ended.clone();
            std::thread::spawn(move || {
                std::thread::sleep(config.shutdown_delayed_warning);
                if !runtime_ended2.notified.load(Ordering::Acquire) {
                    log::warn!(
                        "Tokio runtime has not ended after {:.1} seconds",
                        config.shutdown_delayed_warning.as_secs_f32()
                    );
                }
            });
            drop(runtime);
            TOKIO_RUNTIME_HANDLE.write().take();
            runtime_ended.notify();
        });

        f(RwLockWriteGuard::downgrade(writer)
            .deref()
            .as_ref()
            .unwrap())
    }
}

/// Gets a `Handle` to the active tokio runtime.
/// 
/// If one does not exist, a new tokio runtime will be created using `TokioRuntimeConfig`.
/// See `set_tokio_runtime_config`.
pub fn get_tokio_handle() -> Handle {
    with_tokio_runtime_handle(|handle| handle.handle.clone())
}

pub fn end_tokio_runtime() -> impl Future<Output = ()> {
    let mut runtime_ended = None;
    if let Some(handle) = TOKIO_RUNTIME_HANDLE.read().as_ref() {
        handle.end_runtime.notify();
        runtime_ended = Some(handle.runtime_ended.clone());
    }
    async move {
        if let Some(runtime_ended) = runtime_ended {
            runtime_ended.notified().await;
        }
    }
}

/// Returns true iff there is no tokio runtime.
pub fn has_tokio_runtime_ended() -> bool {
    TOKIO_RUNTIME_HANDLE.read().is_none()
}

/// Returns true iff there is a tokio runtime and it is ending.
pub fn is_tokio_runtime_ending() -> bool {
    if let Some(handle) = TOKIO_RUNTIME_HANDLE.read().as_ref() {
        handle.end_runtime.was_notified()
    } else {
        false
    }
}

pub async fn wait_for_tokio_runtime_end() {
    if let Some(handle) = TOKIO_RUNTIME_HANDLE.read().as_ref() {
        handle.runtime_ended.notified().await;
    }
}


#[derive(Default)]
struct NotifyFlag {
    notified: AtomicBool,
    notify: Notify,
}

impl NotifyFlag {
    // const fn new() -> Self {
    //     Self {
    //         notified: AtomicBool::new(false),
    //         notify: Notify::const_new(),
    //     }
    // }

    async fn notified(&self) {
        let notified = self.notify.notified();
        if self.was_notified() {
            return;
        }
        notified.await;
    }

    fn was_notified(&self) -> bool {
        self.notified.load(Ordering::Acquire)
    }

    fn notify(&self) {
        self.notified.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }
}


struct RuntimeDropGuardInner {
    backtrace: Backtrace,
    notify: NotifyFlag
}


pub struct RuntimeDropGuard(Arc<RuntimeDropGuardInner>);


impl Default for RuntimeDropGuard {
    fn default() -> Self {
        let inner = Arc::new(RuntimeDropGuardInner { backtrace: Backtrace::force_capture(), notify: Default::default() });
        if let Some(handle) = TOKIO_RUNTIME_HANDLE.read().as_ref() {
            handle.attached_threads.push(inner.clone());
        }
        RuntimeDropGuard(inner)
    }
}


impl RuntimeDropGuard {
    pub fn is_attached(&self) -> bool {
        Arc::strong_count(&self.0) > 1
    }
}


impl Drop for RuntimeDropGuard {
    fn drop(&mut self) {
        self.0.notify.notify();
    }
}


thread_local! {
    static DROP_NOTIFY: RefCell<Option<RuntimeDropGuard>> = const { RefCell::new(None) };
}


pub fn attach_drop_guard() {
    DROP_NOTIFY.with_borrow_mut(|x| *x = Some(RuntimeDropGuard::default()));
}


pub fn detach_drop_guard() -> Option<RuntimeDropGuard> {
    DROP_NOTIFY.with_borrow_mut(Option::take)
}
