//! A good logging solution is instrumental to big projects with rapid
//! prototyping cycles.
//!
//! Having plenty sources of data while remaining highly configurable is
//! the goal of Unros, and this module provides that.

use std::{
    fmt::Write,
    sync::{Arc, Mutex, Once, OnceLock},
    time::Instant,
};

use fern::colors::{Color, ColoredLevelConfig};
use log::Level;

use crate::{pubsub::caller::{ImmutCallbacks, ImmutCallbacksRef}, 
    runtime::{has_repl, MainRuntimeContext, RuntimeContextExt}};


pub mod dump;
pub mod rate;

/// Sets up a locally available set of logging macros.
///
/// The `log` crate allows users to configure the `target`
/// parameter of a log, allowing developers to better filter
/// messages by file. Unros takes this one step further by
/// automatically setting this `target` parameter to be the name
/// of the current node (as passed by the context). This allows
/// two nodes of the same class to have different log targets
/// if their names differ, which should help you to identify
/// issues faster.
#[macro_export]
macro_rules! setup_logging {
    ($context: ident) => {
        setup_logging!($context $)
    };
    ($context: ident $dol:tt) => {
        use $crate::runtime::RuntimeContextExt;
        let _context = &$context;
        #[allow(unused_macros)]
        macro_rules! info {
            ($dol($dol arg:tt)+) => {
                $crate::log::info!(target: $context.get_name(), $dol ($dol arg)+)
            };
        }
        #[allow(unused_macros)]
        macro_rules! warn {
            ($dol ($dol arg:tt)+) => {
                $crate::log::warn!(target: $context.get_name(), $dol ($dol arg)+)
            };
        }
        #[allow(unused_macros)]
        macro_rules! error {
            ($dol ($dol arg:tt)+) => {
                $crate::log::error!(target: $context.get_name(), $dol ($dol arg)+)
            };
        }
        #[allow(unused_macros)]
        macro_rules! debug {
            ($dol ($dol arg:tt)+) => {
                $crate::log::debug!(target: $context.get_name(), $dol ($dol arg)+)
            };
        }
        #[allow(unused_macros)]
        macro_rules! trace {
            ($dol ($dol arg:tt)+) => {
                $crate::log::trace!(target: $context.get_name(), $dol ($dol arg)+)
            };
        }
    };
}

pub(crate) static START_TIME: OnceLock<Instant> = OnceLock::new();
static LOG_PUB: OnceLock<ImmutCallbacksRef<Arc<str>>> = OnceLock::new();

/// Gets a reference to the `Publisher` for logs.
///
/// # Panics
/// Panics if the logger has not been initialized. If this method
/// is called inside of or after `start_unros_runtime`, the logger is
/// always initialized.
pub fn get_log_pub() -> ImmutCallbacksRef<Arc<str>> {
    LOG_PUB.get().unwrap().clone()
}

#[derive(Default)]
struct LogPub {
    publisher: Mutex<ImmutCallbacks<Arc<str>>>,
}

impl log::Log for LogPub {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        !(metadata.target() == "unros_core::logging::dump" && metadata.level() == Level::Info)
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        self.publisher
            .lock()
            .unwrap()
            .call(format!("{}", record.args()).into_boxed_str().into());
    }

    fn flush(&self) {}
}

static LOG_INIT: Once = Once::new();

/// Initializes the default logging implementation.
///
/// This is called automatically in `run_all` and `async_run_all`, but
/// there may be additional logs produced before these methods that would
/// be ignored if the logger was not set up yet. As such, you may call this
/// method manually, when needed. Calling this multiple times is safe and
/// will not return errors.
pub(crate) fn init_default_logger(context: &MainRuntimeContext) {
    LOG_INIT.call_once(|| {
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
        let _ = LOG_PUB.set(log_pub.publisher.get_mut().unwrap().get_ref());
        let log_pub: Box<dyn log::Log> = Box::new(log_pub);

        let _ = fern::Dispatch::new()
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
            // Apply globally
            .apply();
    });
}
