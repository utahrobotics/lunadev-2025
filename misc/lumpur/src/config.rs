pub use serde;
pub use toml::{Table, Value};

pub trait Configuration: Sized {
    fn from_config_file(config_file: Table) -> Option<Self>;
}

#[macro_export]
macro_rules! define_configuration {
    {
        $(#[$attr: meta])*
        $vis: vis enum $name: ident {
            $(
                $cmd_name: ident {
                    $(
                        $(#[env($var_name: ident)])?
                        $param: ident: $param_ty: ty
                    ),*
                }
            ),*
        }
    } => {
        $(#[$attr])*
        $vis enum $name {
            $(
                $cmd_name {
                    $(
                        $param: $param_ty
                    ),*
                }
            ),*
        }
        impl $crate::config::Configuration for $name {
            fn from_config_file(config_file: $crate::config::Table) -> Option<Self> {
                use $crate::config::serde;
                use $crate::config::Value;
                let Some(command_name) = std::env::args().nth(1) else {
                    tracing::error!("No command provided");
                    return None;
                };

                fn inline_parse(string: String) -> Value {
                    if let Ok(n) = string.parse() {
                        return Value::Integer(n);
                    }
                    if let Ok(n) = string.parse() {
                        return Value::Float(n);
                    }
                    Value::String(string)
                }

                $(
                    if stringify!($cmd_name).to_lowercase().as_str() == command_name {
                        #[derive(serde::Deserialize)]
                        struct Dummy {
                            $(
                                $param: $param_ty
                            ),*
                        }

                        let mut cmd_config = None;
                        let mut common_values = vec![];
                        for (k, v) in config_file {
                            if let Value::Table(v) = v {
                                if stringify!($cmd_name) == k || stringify!($cmd_name).to_lowercase().as_str() == k {
                                    cmd_config = Some(v);
                                }
                            } else {
                                common_values.push((k, v));
                            }
                        }
                        let mut cmd_config = cmd_config.unwrap_or_default();
                        for (k, v) in common_values {
                            if !cmd_config.contains_key(&k) {
                                cmd_config.insert(k, v);
                            }
                        }
                        let mut accessed_vars = std::collections::HashSet::<&'static str>::new();
                        $(
                            $(
                                if let Ok(val_str) = std::env::var(stringify!($var_name)) {
                                    cmd_config.insert(stringify!($param).to_string(), inline_parse(val_str));
                                    accessed_vars.insert(stringify!($var_name));
                                }
                            )?
                        )*
                        for arg in std::env::args().skip(2) {
                            let mut iter = arg.split('=');
                            let param_name = iter.next().unwrap();
                            let Some(param_val) = iter.next() else {
                                tracing::error!("Invalid command line argument: {}", arg);
                                return None;
                            };
                            cmd_config.insert(param_name.to_string(), inline_parse(param_val.into()));
                            accessed_vars.remove(param_name);
                        }
                        for var_name in accessed_vars {
                            tracing::debug!("Environment Variable {:?} was accessed", var_name);
                        }

                        let dummy: Dummy = match cmd_config.try_into() {
                            Ok(x) => x,
                            Err(e) => {
                                tracing::error!("Failed to deserialize config for {command_name}: {e}");
                                return None;
                            }
                        };
                        return Some(
                            $name::$cmd_name {
                                $(
                                    $param: dummy.$param
                                ),*
                            }
                        );
                    }
                )*
                tracing::error!("Unknown command: {command_name}");
                None
            }
        }
    }
}
