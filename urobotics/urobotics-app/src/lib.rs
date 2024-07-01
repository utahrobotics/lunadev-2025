use std::path::PathBuf;

use fxhash::FxHashMap;
use serde::de::DeserializeOwned;
use unfmt::unformat;
use urobotics_core::{cabinet::CabinetBuilder, end_tokio_runtime_and_wait, log::{log_panics, log_to_console, log_to_file, metrics::{CpuUsage, Temperature}}, task::AsyncTask};

pub trait Application: DeserializeOwned {
    const APP_NAME: &'static str;
    const DESCRIPTION: &'static str;

    fn run(self);
}


struct BoxedApp {
    description: &'static str,
    func: Box<dyn FnOnce(String)>,
}


pub struct Applications {
    pub name: &'static str,
    pub description: &'static str,
    pub config_path: PathBuf,
    pub log_path: PathBuf,
    pub cabinet_root_path: PathBuf,
    pub cpu_usage: Option<CpuUsage>,
    pub temperature: Option<Temperature>,
    functions: FxHashMap<&'static str, BoxedApp>,
}


impl Default for Applications {
    fn default() -> Self {
        Self {
            name: "unnamed",
            description: "Lorem ipsum",
            config_path: PathBuf::from("app_config.toml"),
            log_path: PathBuf::from(".log"),
            cabinet_root_path: PathBuf::from("cabinet"),
            functions: FxHashMap::default(),
            cpu_usage: Some(CpuUsage { cpu_usage_warning_threshold: 80.0 }),
            temperature: Some(Temperature {
                temperature_warning_threshold: 80.0,
                ignore_component_temperature: Default::default(),
            })
        }
    }
}


#[macro_export]
macro_rules! application {
    () => {{
        let mut app = $crate::Applications::default();
        app.name = env!("CARGO_PKG_NAME");
        app.description = env!("CARGO_PKG_DESCRIPTION");
        app
    }}
}


impl Applications {
    pub fn run(&mut self) {
        let mut args = std::env::args();
        let _exe = args.next().expect("No executable name");
        let Some(cmd) = args.next() else {
            eprintln!("No command given");
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
            eprintln!("Unknown command. Use one of the following:");
            for (name, app) in self.functions.iter() {
                eprintln!("{}\t-\t{}", name, app.description);
            }
            return;
        };
        CabinetBuilder::new_with_crate_name(&self.cabinet_root_path, self.name)
            .add_file_to_copy(&self.config_path)
            .build()
            .expect("Failed to create cabinet");
        log_panics();
        log_to_file(&self.log_path).expect("Failed to open log file");
        log_to_console();

        if let Some(cpu_usage) = self.cpu_usage.clone() {
            cpu_usage.spawn();
        }
        if let Some(temperature) = self.temperature.clone() {
            temperature.spawn();
        }

        let config_raw = std::fs::read_to_string(&self.config_path).expect("Failed to read config file");
        let mut config_parsed = String::new();
        let mut copying = false;

        for mut line in config_raw.lines() {
            line = line.trim();
            if line.starts_with('#') {
                config_parsed.push_str(line);
                config_parsed.push_str("\n");
                continue;
            }
            if let Some(app_name) = unformat!("[{}]", line) {
                copying = app_name == &cmd;
            } else if copying {
                config_parsed.push_str(line);
            }
            config_parsed.push_str("\n");
        }

        (app.func)(config_parsed);
    }

    pub fn add_app<T: Application>(&mut self) -> &mut Self {
        self.functions.insert(
            T::APP_NAME,
            BoxedApp {
                description: T::DESCRIPTION,
                func: Box::new(move |config: String| {
                    match toml::from_str::<T>(&config) {
                        Ok(config) => {
                            config.run();
                            end_tokio_runtime_and_wait();
                        }
                        Err(e) => {
                            eprintln!("{e}");
                        }
                    }
                }),
            },
        );
        self
    }
}
