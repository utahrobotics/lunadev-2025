use std::path::Path;

pub use clap;
use clap::ArgMatches;
use fxhash::FxHashMap;
use urobotics_core::{function::{FunctionConfig, SendAsyncFunctionConfig}, runtime::RuntimeContext};

#[macro_export]
macro_rules! command {
    () => {
        $crate::Command::from($crate::clap::command!())
    }
}

pub struct Command {
    pub command: clap::Command,
    functions: FxHashMap<String, Box<dyn FnOnce(toml::Table, RuntimeContext)>>
}


impl From<clap::Command> for Command {
    fn from(command: clap::Command) -> Self {
        Self {
            command,
            functions: FxHashMap::default()
        }
    }
}


impl Command {
    pub async fn get_matches(mut self, context: RuntimeContext) -> ArgMatches {
        let matches = self.command.get_matches();
        let config_path: Option<&String> = matches.get_one("config");
        let config_path = config_path.map(String::as_str);
        let config_path = config_path.unwrap_or("config.toml");

        let mut table = if Path::new(config_path).exists() {
            let config_values: toml::Value = toml::from_str(&tokio::fs::read_to_string(config_path).await.expect("Failed to read config file")).unwrap();
            let toml::Value::Table(table) = config_values else {
                panic!("Config file is not a table")
            };
            table
        } else {
            toml::Table::new()
        };

        let Some((sub_cmd, sub_matches)) = matches.subcommand() else { return matches };

        let sub_cmd_table = table.remove(sub_cmd).unwrap_or(toml::Value::Table(toml::Table::new()));
        let toml::Value::Table(mut table) = sub_cmd_table else {
            panic!("Subcommand config in {config_path} is not a table")
        };

        if let Some(params) = sub_matches.get_many::<String>("parameters") {
            for param in params {
                let sub_table: toml::Value = toml::from_str(&param).expect("Failed to parse parameter");
                let toml::Value::Table(sub_table) = sub_table else {
                    panic!("Parameter is not valid")
                };
                for (key, value) in sub_table {
                    table.insert(key, value);
                }
            }
        }

        if let Some(func) = self.functions.remove(sub_cmd) {
            func(table, context);
        }

        matches
    }

    pub fn add_function<F: FunctionConfig + Send + 'static>(&mut self) -> &mut Self {
        self.functions.insert(F::NAME.into(), Box::new(move |table, context| {
            let config: F = toml::Value::Table(table).try_into().expect("Failed to parse config");
            config.spawn(context);
        }));
        self
    }

    pub fn add_async_function<F: SendAsyncFunctionConfig + Send + 'static>(&mut self) -> &mut Self {
        self.functions.insert(F::NAME.into(), Box::new(move |table, context| {
            let config: F = toml::Value::Table(table).try_into().expect("Failed to parse config");
            config.spawn(context);
        }));
        self
    }
}