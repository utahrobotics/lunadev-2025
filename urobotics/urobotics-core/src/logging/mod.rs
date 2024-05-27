//! A good logging solution is instrumental to big projects with rapid
//! prototyping cycles.
//!
//! Having plenty sources of data while remaining highly configurable is
//! the goal of Unros, and this module provides that.

use std::{
    fmt::Write,
    ops::Deref,
    sync::{Arc, Once, OnceLock},
    time::Instant,
};

use fern::colors::{Color, ColoredLevelConfig};
use log::Level;
use parking_lot::Mutex;

use crate::{
    callbacks::caller::{Callbacks, CallbacksRef, SharedCallbacks},
    runtime::RuntimeContext,
};

pub mod rate;

pub(crate) static START_TIME: OnceLock<Instant> = OnceLock::new();
static LOG_PUB: OnceLock<CallbacksRef<dyn Fn(&log::Record) + Send>> = OnceLock::new();

/// Gets a reference to the `Publisher` for logs.
///
/// # Panics
/// Panics if the logger has not been initialized. If this method
/// is called inside of or after `start_unros_runtime`, the logger is
/// always initialized.
pub fn get_log_pub() -> CallbacksRef<dyn Fn(&log::Record) + Send> {
    LOG_PUB.get().unwrap().clone()
}

#[derive(Default)]
struct LogPub {
    publisher: SharedCallbacks<dyn Fn(&log::Record)>,
}

impl log::Log for LogPub {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        !(metadata.target() == "unros_core::logging::dump" && metadata.level() == Level::Info)
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let _ = (record,).clone();
        let c = self.publisher.storage.pop().unwrap();

        (self.publisher.storage.pop().unwrap())(record);
        self.publisher.call(record);
        (self.publisher)(record);
    }

    fn flush(&self) {}
}

/// Initializes the default logging implementation.
///
/// This is called automatically in `run_all` and `async_run_all`, but
/// there may be additional logs produced before these methods that would
/// be ignored if the logger was not set up yet. As such, you may call this
/// method manually, when needed. Calling this multiple times is safe and
/// will not return errors.
pub fn init_default_logger(context: &RuntimeContext) -> fern::Dispatch {
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

    let colors = ColoredLevelConfig::new()
        .warn(Color::Yellow)
        .error(Color::Red)
        .trace(Color::BrightBlack);

    let _ = START_TIME.set(Instant::now());

    let mut log_pub = LogPub::default();
    let _ = LOG_PUB.set(log_pub.publisher.get_mut().get_ref());
    let log_pub: Box<dyn log::Log> = Box::new(log_pub);

    fern::Dispatch::new()
        // Add blanket level filter -
        .level(log::LevelFilter::Debug)
        .filter(|x| !(x.target().starts_with("wgpu") && x.level() >= Level::Info))
        // Output to stdout, files, and other Dispatch configurations
        .chain(
            fern::Dispatch::new()
                .format(move |out, message, record| {
                    let secs = START_TIME.get().unwrap().elapsed().as_secs_f32();
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
                )
                .chain(log_pub),
        )
        .chain(
            fern::Dispatch::new()
                .level(log::LevelFilter::Info)
                .filter(|_| !has_repl())
                // This filter is to avoid logging panics to the console, since rust already does that.
                // Note that the 'panic' target is set by us in eyre.rs.
                .filter(|x| x.target() != "panic")
                .filter(|x| {
                    !(x.target() == "unros_core::logging::dump" && x.level() == Level::Info)
                })
                .format(move |out, message, record| {
                    let secs = START_TIME.get().unwrap().elapsed().as_secs_f32();
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
