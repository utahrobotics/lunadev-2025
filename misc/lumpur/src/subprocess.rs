use std::{backtrace::Backtrace, panic::set_hook, path::Path, sync::Mutex};

use raw_sync::{events::EventInit, Timeout};
use shared_memory::ShmemConf;
use tracing::Level;
use tracing_subscriber::fmt::time::Uptime;

use crate::config::Configuration;

pub(crate) const EMBEDDED_KEY: &str = "__LUMPUR_EMBEDDED";
pub(crate) const EMBEDDED_VAL: &str = "1";
pub(crate) const SHMEM_VAR_KEY: &str = "__LUMPUR_SHMEM_FLINK";

static ON_EXIT: Mutex<Option<Box<dyn FnOnce() -> () + Send>>> = Mutex::new(None);

pub fn set_on_exit(f: impl FnOnce() -> () + Send + 'static) {
    *ON_EXIT.lock().unwrap() = Some(Box::new(f));
}

pub(crate) fn subprocess_fn<C: Configuration>() -> C {
    tracing_log::LogTracer::init().expect("Failed to initialize log tracer");
    let sub = tracing_subscriber::FmtSubscriber::builder()
        .with_file(true)
        .with_level(true)
        .with_line_number(true)
        .with_ansi(false)
        .json()
        .with_thread_names(true)
        .with_timer(Uptime::default())
        .with_max_level(Level::TRACE)
        .with_writer(std::io::stderr)
        .finish();

    tracing::subscriber::set_global_default(sub)
        .expect("Failed to set global default tracing subscriber");

    set_hook(Box::new(move |info| {
        let backtrace = Backtrace::capture();
        tracing::error!("{info}\n{backtrace}");
    }));

    let flink = std::env::var(SHMEM_VAR_KEY).unwrap();

    // Ctrl-C listener
    std::thread::spawn(move || {
        let shmem = match ShmemConf::new().size(4096).flink(&flink).open() {
            Ok(shmem) => shmem,
            Err(e) => {
                tracing::error!("Failed to open shared memory segment: {e}");
                return;
            }
        };
        let result = unsafe { raw_sync::events::Event::from_existing(shmem.as_ptr()) };
        let (evt, _used_bytes) = match result {
            Ok(evt) => evt,
            Err(e) => {
                tracing::error!("Failed to create ctrl-c event listener: {e}");
                return;
            }
        };
        if let Err(e) = evt.wait(Timeout::Infinite) {
            tracing::error!("Failed to wait for ctrl-c event: {e}");
        }
        tracing::warn!("Ctrl-C event received. Exiting...");
        if let Some(f) = ON_EXIT.lock().unwrap().take() {
            f();
        } else {
            std::process::exit(0);
        }
    });

    if !Path::new("app-config.toml").exists() {
        tracing::error!("app-config.toml not found");
        std::process::exit(1);
    }
    let data = match std::fs::read_to_string("app-config.toml") {
        Ok(data) => data,
        Err(e) => {
            tracing::error!("{e:?}");
            std::process::exit(1);
        }
    };
    let value = match toml::from_str(&data) {
        Ok(value) => value,
        Err(e) => {
            tracing::error!("{e:?}");
            std::process::exit(1);
        }
    };
    match C::from_config_file(value) {
        Some(out) => out,
        None => {
            std::process::exit(1);
        }
    }
}
