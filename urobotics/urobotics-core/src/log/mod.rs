//! A good logging solution is instrumental to big projects with rapid
//! prototyping cycles.
//!
//! Having plenty sources of data while remaining highly configurable is
//! the goal of Unros, and this module provides that.

use std::{
    fs::File,
    panic::PanicHookInfo,
    path::Path,
    sync::{Arc, LazyLock, OnceLock},
    time::{Duration, Instant},
};

pub use color_eyre::owo_colors::OwoColorize;
use crossbeam::atomic::AtomicCell;
use log::set_boxed_logger;
pub use log::{debug, error, info, trace, warn, Level, LevelFilter, Log, Record};
use parking_lot::Mutex;

use crate::{define_callbacks, fn_alias};

pub mod metrics;
pub mod rate;

pub static START_TIME: OnceLock<Instant> = OnceLock::new();

pub fn get_program_time() -> Duration {
    START_TIME.get_or_init(Instant::now).elapsed()
}

fn_alias! {
    pub type LogCallbacksRef = CallbacksRef(&log::Record) + Send + Sync
}
define_callbacks!(LogCallbacks => Fn(record: &log::Record) + Send + Sync);
static LOG_CALLBACKS: LazyLock<LogCallbacksRef> = LazyLock::new(|| {
    let log_pub = LogPub::default();
    let log_pub_ref = log_pub.callbacks.get_ref();
    log::set_max_level(LevelFilter::Trace);
    let _ = set_boxed_logger(Box::new(log_pub));
    log_pub_ref
});

fn_alias! {
    pub type PanicCallbacksRef = CallbacksRef(&PanicHookInfo) + Send + Sync
}
define_callbacks!(PanicCallbacks => Fn(panic_info: &PanicHookInfo) + Send + Sync);
static PANIC_CALLBACKS: LazyLock<PanicCallbacksRef> = LazyLock::new(|| {
    let _ = rayon::ThreadPoolBuilder::default()
        .panic_handler(|_| {
            // Panics in rayon still get logged, but this prevents
            // the thread pool from aborting the entire process
        })
        .build_global();

    let (panic_hook, eyre_hook) = color_eyre::config::HookBuilder::default().into_hooks();
    let panic_hook = panic_hook.into_panic_hook();
    eyre_hook.install().expect("Failed to install eyre hook");

    let panic_pub = PanicCallbacks::default();
    let panic_pub_ref = panic_pub.get_ref();
    std::panic::set_hook(Box::new(move |panic_info| {
        panic_pub.call_immut(panic_info);
        panic_hook(panic_info);
    }));
    panic_pub_ref
});

#[inline(always)]
pub fn get_log_callbacks() -> &'static LogCallbacksRef {
    &LOG_CALLBACKS
}

/// Appends the given logger to the list of loggers.
///
/// The given logger will *never* be flushed, and it is
/// not guaranteed to be dropped.
#[inline(always)]
pub fn add_logger(logger: impl log::Log + 'static) {
    LOG_CALLBACKS.add_dyn_fn(Box::new(move |record| logger.log(record)));
}

#[inline(always)]
pub fn get_panic_hook_callbacks() -> &'static PanicCallbacksRef {
    &PANIC_CALLBACKS
}

pub fn init_panic_hook() {
    LazyLock::force(&PANIC_CALLBACKS);
}

#[derive(Default)]
struct LogPub {
    callbacks: LogCallbacks,
}

impl log::Log for LogPub {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        !self.callbacks.is_empty()
    }

    fn log(&self, record: &log::Record) {
        self.callbacks.call_immut(record);
    }

    fn flush(&self) {}
}

pub fn log_panics() {
    get_panic_hook_callbacks().add_dyn_fn(Box::new(|panic_info| {
        use std::fmt::Write;
        let mut log = String::new();
        writeln!(log, "The application panicked (crashed).").unwrap();

        // Print panic message.
        let payload = panic_info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic_info.payload().downcast_ref::<&str>().copied())
            .unwrap_or("<non string panic payload>");

        writeln!(log, "\tMessage:  {payload}").unwrap();

        // If known, print panic location.
        write!(log, "\tLocation: ").unwrap();

        if let Some(loc) = panic_info.location() {
            write!(log, "{}:{}", loc.file(), loc.line())
        } else {
            write!(log, "<unknown>")
        }
        .unwrap();

        log::error!(target: "panic", "{log}");
    }));
}

#[derive(Clone)]
pub struct LogFilter(Arc<AtomicCell<LevelFilter>>);

impl LogFilter {
    fn new() -> Self {
        Self(Arc::new(AtomicCell::new(LevelFilter::Info)))
    }

    pub fn set(&self, level: LevelFilter) {
        self.0.store(level);
    }

    pub fn get(&self) -> LevelFilter {
        self.0.load()
    }
}

pub fn log_to_file(path: impl AsRef<Path>) -> std::io::Result<LogFilter> {
    use std::io::Write;
    get_program_time();

    let file = File::create(path)?;
    let file = Mutex::new(file);
    let filter = LogFilter::new();
    filter.set(LevelFilter::Debug);
    let filter2 = filter.clone();

    get_log_callbacks().add_dyn_fn(Box::new(move |record| {
        if record.level() > filter.get() {
            return;
        }
        let secs = get_program_time().as_secs_f32();
        writeln!(
            file.lock(),
            "[{:0>2}:{:.2} {} {}] {}",
            (secs / 60.0).floor() as usize,
            secs % 60.0,
            record.level(),
            record.target(),
            record.args()
        )
        .expect("Failed to write to log file");
    }));

    Ok(filter2)
}

pub fn log_to_console() -> LogFilter {
    get_program_time();
    let filter = LogFilter::new();
    let filter2 = filter.clone();

    get_log_callbacks().add_dyn_fn(Box::new(move |record| {
        if record.target() == "panic" || record.level() > filter.get() {
            return;
        }
        let secs = get_program_time().as_secs_f32();
        let msg = record.args().to_string();
        let mins = (secs / 60.0).floor() as usize;
        let secs = secs % 60.0;

        match record.level() {
            Level::Error => println!(
                "{}",
                format!("[{:0>2}:{:.2} {}] {}", mins, secs, record.target(), msg).red()
            ),
            Level::Warn => println!(
                "{}",
                format!("[{:0>2}:{:.2} {}] {}", mins, secs, record.target(), msg).yellow()
            ),
            _ => println!("[{:0>2}:{:.2} {}] {}", mins, secs, record.target(), msg),
        }
    }));

    filter2
}
