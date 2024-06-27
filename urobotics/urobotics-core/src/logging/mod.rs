//! A good logging solution is instrumental to big projects with rapid
//! prototyping cycles.
//!
//! Having plenty sources of data while remaining highly configurable is
//! the goal of Unros, and this module provides that.

use std::{
    fmt::Write,
    sync::OnceLock,
    time::{Duration, Instant},
};

use fern::colors::{Color, ColoredLevelConfig};
use log::set_boxed_logger;

use crate::{define_callbacks, fn_alias, runtime::RuntimeContext};

pub mod rate;

pub static START_TIME: OnceLock<Instant> = OnceLock::new();

pub fn get_program_time() -> Duration {
    START_TIME.get_or_init(Instant::now).elapsed()
}

fn_alias! {
    type LogCallbacksRef = CallbacksRef(&log::Record) + Send + Sync
}
define_callbacks!(pub LogCallbacks => Fn(record: &log::Record) + Send + Sync);
static LOG_PUB: OnceLock<LogCallbacksRef> = OnceLock::new();

#[inline(always)]
pub fn get_logger_ref() -> &'static LogCallbacksRef {
    LOG_PUB.get_or_init(|| {
        let log_pub = LogPub::default();
        let log_pub_ref = log_pub.callbacks.get_ref();
        let _ = set_boxed_logger(Box::new(log_pub));
        log_pub_ref
    })
}

#[inline(always)]
pub fn add_logger(logger: impl log::Log + 'static) {
    get_logger_ref().add_dyn_fn(Box::new(move |record| logger.log(record)));
}

#[derive(Default)]
struct LogPub {
    callbacks: LogCallbacks,
}

impl log::Log for LogPub {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        self.callbacks.call_immut(record);
    }

    fn flush(&self) {}
}

pub fn init_panic_hook() {
    let _ = rayon::ThreadPoolBuilder::default()
        .panic_handler(|_| {
            // Panics in rayon still get logged, but this prevents
            // the thread pool from aborting the entire process
        })
        .build_global();

    let (panic_hook, eyre_hook) = color_eyre::config::HookBuilder::default().into_hooks();
    let panic_hook = panic_hook.into_panic_hook();
    eyre_hook.install().expect("Failed to install eyre hook");
    std::panic::set_hook(Box::new(move |panic_info| {
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
        panic_hook(panic_info);
    }));
}

/// Initializes the default logging implementation.
///
/// This is called automatically in `run_all` and `async_run_all`, but
/// there may be additional logs produced before these methods that would
/// be ignored if the logger was not set up yet. As such, you may call this
/// method manually, when needed. Calling this multiple times is safe and
/// will not return errors.
pub fn create_default_logger(context: &RuntimeContext) -> fern::Dispatch {
    let colors = ColoredLevelConfig::new()
        .warn(Color::Yellow)
        .error(Color::Red)
        .trace(Color::BrightBlack);

    get_program_time();

    fern::Dispatch::new()
        // Add blanket level filter -
        .level(log::LevelFilter::Debug)
        // .filter(|x| !(x.target().starts_with("wgpu") && x.level() >= Level::Info))
        // Output to stdout, files, and other Dispatch configurations
        .chain(
            fern::Dispatch::new()
                .format(move |out, message, record| {
                    let secs = get_program_time().as_secs_f32();
                    out.finish(format_args!(
                        "[{:0>1}:{:.2} {} {}] {}",
                        (secs / 60.0).floor(),
                        secs % 60.0,
                        record.level(),
                        record.target(),
                        message
                    ));
                })
                .chain(
                    fern::log_file(context.get_dump_path().join(".log"))
                        .expect("Failed to create log file. Do we have permissions?"),
                ),
        )
        .chain(
            fern::Dispatch::new()
                .level(log::LevelFilter::Info)
                // This filter is to avoid logging panics to the console, since rust already does that.
                // Note that the 'panic' target is set by us in eyre.rs.
                .filter(|x| x.target() != "panic")
                .format(move |out, message, record| {
                    let secs = get_program_time().as_secs_f32();
                    out.finish(format_args!(
                        "\x1B[{}m[{:0>1}:{:.2} {}] {}\x1B[0m",
                        colors.get_color(&record.level()).to_fg_str(),
                        (secs / 60.0).floor(),
                        secs % 60.0,
                        record.target(),
                        message
                    ));
                })
                .chain(std::io::stdout()),
        )
}
