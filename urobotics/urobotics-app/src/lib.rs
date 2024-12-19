//! A library for creating applications with a command line interface.

use std::{
    path::{Path, PathBuf},
    sync::Once,
};

use fxhash::FxHashMap;
use serde::de::DeserializeOwned;
use unfmt::unformat;
use urobotics_core::{
    cabinet::CabinetBuilder,
    end_tokio_runtime_and_wait, get_tokio_handle,
    log::{
        log_panics, log_to_file,
        metrics::{CpuUsage, Temperature},
        OwoColorize,
    },
};

/// A trait that represents an application that can be run.
pub trait Runnable {
    /// Runs the application on the current thread.
    fn run(self);
}

pub trait Application: Runnable + DeserializeOwned {
    /// A description of the application as it appears in the command-line.
    const DESCRIPTION: &'static str;

    fn get_app_name() -> String;
}

#[macro_export]
macro_rules! define_app {
    ($vis: vis $app: ident($runnable: ident): $desc: literal) => {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        #[allow(non_snake_case)]
        $vis struct $app {
            $app: $runnable
        }

        impl $crate::Runnable for $app {
            fn run(self) {
                $crate::Runnable::run(self.$app);
            }
        }

        impl $crate::Application for $app {
            const DESCRIPTION: &'static str = $desc;

            fn get_app_name() -> String {
                stringify!($app).to_lowercase()
            }
        }
    }
}

struct BoxedApp {
    description: &'static str,
    func: Box<dyn FnOnce(String)>,
}

/// A collection of applications that can be run.
pub struct Applications {
    /// The name of the current crate.
    pub name: &'static str,
    /// The description of the current crate.
    pub description: &'static str,
    /// The path to the configuration file.
    pub config_path: PathBuf,
    /// The path to create the log file.
    pub log_path: PathBuf,
    /// The cabinet builder for creating a cabinet.
    pub cabinet_builder: CabinetBuilder,
    /// The CPU usage monitoring configuration.
    pub cpu_usage: Option<CpuUsage>,
    /// The temperature monitoring configuration.
    pub temperature: Option<Temperature>,
    functions: FxHashMap<String, BoxedApp>,
}

impl Default for Applications {
    /// Creates a default `Applications` with possibly inaccurate values.
    ///
    /// Refer to the `application` macro for a more accurate default.
    fn default() -> Self {
        Self {
            name: "unnamed",
            description: "Lorem ipsum",
            config_path: PathBuf::from("app-config.toml"),
            log_path: PathBuf::from(".log"),
            functions: FxHashMap::default(),
            cpu_usage: Some(CpuUsage {
                cpu_usage_warning_threshold: 80.0,
            }),
            temperature: Some(Temperature {
                temperature_warning_threshold: 80.0,
                ignore_component_temperature: Default::default(),
            }),
            cabinet_builder: CabinetBuilder::new_with_crate_name("cabinet", "unnamed"),
        }
    }
}

static APPLICATION_CONSUMED: Once = Once::new();

/// Creates a default `Applications` struct with the current crate's name and description.
#[macro_export]
macro_rules! application {
    () => {{
        let mut app = $crate::Applications::default();
        app.name = env!("CARGO_PKG_NAME");
        app.description = env!("CARGO_PKG_DESCRIPTION");
        app.cabinet_builder
            .set_cabinet_path_with_name("cabinet", app.name);
        app
    }};
}

impl Applications {
    fn pre_run_inner(&mut self) -> Option<bool> {
        if !Path::new(&self.config_path).exists() {
            eprintln!(
                "{}",
                format!("Config file does not exist ({:?})", self.config_path).red()
            );
            return Some(false);
        }

        let mut worked = None;
        APPLICATION_CONSUMED.call_once(|| {
            if let Err(e) = self
                .cabinet_builder
                .add_file_to_copy(&self.config_path)
                .build()
            {
                eprintln!("{}", format!("Failed to create cabinet: {}", e).red());
                worked = Some(false);
                return;
            }

            log_panics();

            if let Err(e) = log_to_file(&self.log_path) {
                eprintln!("{}", format!("Failed to open log file: {}", e).red());
                worked = Some(false);
                return;
            }

            let handle = get_tokio_handle();
            if let Some(cpu_usage) = self.cpu_usage.clone() {
                handle.spawn(cpu_usage.run());
            }
            if let Some(temperature) = self.temperature.clone() {
                handle.spawn(temperature.run());
            }

            worked = Some(true);
        });

        worked
    }

    /// Runs the pre-application setup, which involves the following:
    /// - Creating a cabinet (changing working directory, copying files over, and making symlinks)
    /// - Setting up logging to file and console (panics are logged as well)
    /// - Setting up CPU usage and temperature monitoring
    ///
    /// Returns `Some(true)` if setup executed successfully, `Some(false)` if there was an error (it will be printed to stderr), or `None` if the setup has already been run.
    #[inline]
    pub fn pre_run(mut self) -> Option<bool> {
        self.pre_run_inner()
    }

    /// Runs the application specified through the command line arguments.
    ///
    /// This will execute the pre-application setup if it hasn't been run yet.
    pub fn run(mut self) {
        let mut args = std::env::args();
        let _exe = args.next().expect("No executable name");
        let Some(cmd) = args.next() else {
            eprintln!("{}", "No command given".yellow());
            return;
        };
        if cmd == "help" {
            println!("{}\t-\t{}", self.name, self.description);
            for (name, app) in self.functions.iter() {
                eprintln!("{}\t-\t{}", name, app.description);
            }
            return;
        }
        let Some(app) = self.functions.remove(cmd.as_str()) else {
            eprintln!("{}", "Unknown command. Use one of the following:".yellow());
            for (name, app) in self.functions.iter() {
                eprintln!("{}", format!("{}\t-\t{}", name, app.description).yellow());
            }
            return;
        };

        if self.pre_run_inner() == Some(false) {
            return;
        }

        let config_raw = match std::fs::read_to_string(&self.config_path) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("{}", format!("Failed to read config file: {}", e).red());
                return;
            }
        };
        (app.func)(config_raw);
    }

    /// Adds an application to the collection of applications.
    ///
    /// Do note that the application is added statically as a type parameter.
    pub fn add_app<T: Application>(mut self) -> Self {
        self.functions.insert(
            T::get_app_name(),
            BoxedApp {
                description: T::DESCRIPTION,
                func: Box::new(move |config: String| match toml::from_str::<T>(&config) {
                    Ok(config) => {
                        config.run();
                        end_tokio_runtime_and_wait();
                    }
                    Err(e) => {
                        eprint!("{}", e.red());
                    }
                }),
            },
        );
        self
    }
}

/// A macro for creating an ad-hoc application from a function.
///
/// # Examples
/// ```rust
/// fn my_func() {
///     // TODO
/// }
///
/// adhoc_app!(pub MyApp(my_func): "My application");
/// ```
#[macro_export]
macro_rules! adhoc_app {
    ($vis:vis $type_name: ident($func:ident): $description:literal) => {
        #[derive(serde::Deserialize)]
        $vis struct $type_name {}
        impl $crate::Runnable for $type_name {
            fn run(self) {
                $func();
            }
        }
        impl $crate::Application for $type_name {
            const DESCRIPTION: &'static str = $description;

            fn get_app_name() -> String {
                stringify!($type_name).to_lowercase()
            }
        }
    };
}
