use std::path::Path;

pub use clap;
use clap::{arg, Arg, ArgMatches};
use fxhash::FxHashMap;
use std::str::FromStr;
use urobotics_core::{
    function::{FunctionConfig, SendAsyncFunctionConfig},
    runtime::RuntimeContext,
};

#[macro_export]
macro_rules! command {
    () => {
        $crate::Command::from($crate::clap::command!())
    };
}

pub struct Command {
    pub command: clap::Command,
    functions: FxHashMap<String, Box<dyn FnOnce(toml::Table, RuntimeContext) + Send>>,
}

impl From<clap::Command> for Command {
    fn from(mut command: clap::Command) -> Self {
        command = command.arg(arg!([config] "optional path to a config file"));
        Self {
            command,
            functions: FxHashMap::default(),
        }
    }
}

impl Command {
    pub async fn get_matches(mut self, context: RuntimeContext) -> ArgMatches {
        let matches = self.command.get_matches();
        let config_path: Option<&String> = matches.get_one("config");
        let mut config_path = config_path.map(String::as_str);
        if config_path.is_none() && Path::new("roboconfig.toml").exists() {
            config_path = Some("roboconfig.toml");
        }

        let mut table = if let Some(config_path) = config_path {
            let config_values: toml::Value = toml::from_str(
                &tokio::fs::read_to_string(config_path)
                    .await
                    .expect("Failed to read config file"),
            )
            .unwrap();
            let toml::Value::Table(table) = config_values else {
                panic!("Config file is not a table")
            };
            table
        } else {
            toml::Table::new()
        };

        let Some((sub_cmd, sub_matches)) = matches.subcommand() else {
            return matches;
        };

        let sub_cmd_table = table
            .remove(sub_cmd)
            .unwrap_or(toml::Value::Table(toml::Table::new()));
        let toml::Value::Table(mut table) = sub_cmd_table else {
            panic!(
                "Subcommand config in {} is not a table",
                config_path.unwrap()
            )
        };

        if let Some(params) = sub_matches.get_many::<String>("parameters") {
            for param in params {
                let Some(index) = param.find('=') else {
                    panic!("{param} is not a key value association")
                };
                let (key, mut value_str) = param.split_at(index);
                value_str = value_str.split_at(1).1;
                if let Ok(value) = bool::from_str(value_str) {
                    table.insert(key.into(), toml::Value::Boolean(value));
                } else if let Ok(number) = i64::from_str(value_str) {
                    table.insert(key.into(), toml::Value::Integer(number));
                } else if let Ok(number) = f64::from_str(value_str) {
                    table.insert(key.into(), toml::Value::Float(number));
                } else {
                    table.insert(key.into(), value_str.into());
                }
            }
        }

        if let Some(func) = self.functions.remove(sub_cmd) {
            func(table, context);
        }

        matches
    }

    pub fn add_function<F: FunctionConfig + Send + 'static>(mut self) -> Self {
        self.command = self.command.subcommand(
            clap::Command::new(F::NAME)
                .arg(Arg::new("parameters").num_args(..))
                .about(F::DESCRIPTION),
        );
        self.functions.insert(
            F::NAME.into(),
            Box::new(move |table, context| {
                let mut config: F = toml::Value::Table(table)
                    .try_into()
                    .expect("Failed to parse config");
                config = config.standalone(true);
                config.spawn(context);
            }),
        );
        self
    }

    pub fn add_async_function<F: SendAsyncFunctionConfig + Send + 'static>(mut self) -> Self {
        self.command = self.command.subcommand(
            clap::Command::new(F::NAME)
                .arg(Arg::new("parameters").num_args(..))
                .about(F::DESCRIPTION),
        );
        self.functions.insert(
            F::NAME.into(),
            Box::new(move |table, context| {
                let mut config: F = toml::Value::Table(table)
                    .try_into()
                    .expect("Failed to parse config");
                config = config.standalone(true);
                config.spawn(context);
            }),
        );
        self
    }
}
