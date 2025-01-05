use std::{
    collections::BTreeMap,
    io::{LineWriter, Write},
    path::{Path, PathBuf},
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    },
};

use fxhash::FxHashMap;
use tracing::Level;

#[derive(serde::Deserialize, Clone, Copy, Debug)]
enum SerdeLevel {
    ERROR,
    WARN,
    INFO,
    DEBUG,
    TRACE,
}

impl From<SerdeLevel> for Level {
    fn from(level: SerdeLevel) -> Self {
        match level {
            SerdeLevel::ERROR => Level::ERROR,
            SerdeLevel::WARN => Level::WARN,
            SerdeLevel::INFO => Level::INFO,
            SerdeLevel::DEBUG => Level::DEBUG,
            SerdeLevel::TRACE => Level::TRACE,
        }
    }
}

pub(crate) enum LogMessage {
    Stdio {
        level: Level,
        stdio: String,
        message: String,
    },
    Standard {
        timestamp: f32,
        level: Level,
        thread_name: String,
        target: String,
        filename: String,
        line_number: usize,
        fields: BTreeMap<String, serde_json::Value>,
    },
}

impl LogMessage {
    pub(crate) fn aggregate(&self) -> String {
        match self {
            LogMessage::Stdio {
                level,
                stdio,
                message,
            } => format!("{level}{stdio}{message}"),
            LogMessage::Standard {
                level,
                thread_name,
                target,
                filename,
                line_number,
                fields,
                ..
            } => format!("{level}{thread_name}{target}{filename}{line_number}{fields:?}"),
        }
    }

    pub(crate) fn create_ui_message(&self) -> String {
        match self {
            LogMessage::Standard {
                timestamp,
                level,
                fields,
                ..
            } => {
                let message = fields
                    .get("message")
                    .map(|v| {
                        if let Some(msg) = v.as_str() {
                            msg.replace('\n', "\n    ")
                        } else {
                            v.to_string()
                        }
                    })
                    .unwrap_or_else(|| format!("{fields:?}"));
                format!("[{timestamp: >7.2}s {level: <5}] {message}")
            }
            LogMessage::Stdio { level, message, .. } => {
                format!("[         {level: <5}] {message}")
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct RawLogMessage {
    timestamp: String,
    level: SerdeLevel,
    #[serde(rename = "threadName")]
    thread_name: String,
    target: String,
    filename: String,
    line_number: usize,
    fields: BTreeMap<String, serde_json::Value>,
}

pub(crate) fn make_line_f(
    console_tx: Sender<Arc<LogMessage>>,
    write_tx: Sender<Arc<LogMessage>>,
    stdio_level: Level,
    stdio_name: &'static str,
    current_dir: &'static Path,
    ignores: &'static FxHashMap<String, Level>,
) -> impl Fn(String) {
    move |line: String| {
        let log = match serde_json::from_str::<RawLogMessage>(&line) {
            Ok(log) => log,
            Err(_) => {
                let log = Arc::new(LogMessage::Stdio {
                    level: stdio_level,
                    stdio: stdio_name.into(),
                    message: line,
                });
                let _ = console_tx.send(log.clone());
                let _ = write_tx.send(log);
                return;
            }
        };
        let level = log.level.into();
        let log = Arc::new(LogMessage::Standard {
            timestamp: {
                log.timestamp
                    .trim_start()
                    .strip_suffix('s')
                    .unwrap()
                    .parse()
                    .unwrap_or(f32::NAN)
            },
            level,
            thread_name: log.thread_name,
            target: log.target,
            filename: {
                // This corrects the slashes in the filename
                let filename = Path::new(&log.filename)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&log.filename));
                filename
                    .strip_prefix(&current_dir)
                    .unwrap_or(&filename)
                    .to_string_lossy()
                    .into_owned()
            },
            line_number: log.line_number,
            fields: log.fields,
        });
        let LogMessage::Standard { target, .. } = &*log else {
            unreachable!()
        };

        let _ = write_tx.send(log.clone());
        if level <= Level::INFO {
            if let Some(ignore_level) = ignores.get(target) {
                if level >= *ignore_level {
                    return;
                }
            }
            let _ = console_tx.send(log);
        }
    }
}

pub(crate) fn log_write_thread(
    write_rx: Receiver<Arc<LogMessage>>,
    mut log_file: LineWriter<std::fs::File>,
) {
    while let Ok(msg) = write_rx.recv() {
        match &*msg {
            LogMessage::Stdio {
                level,
                stdio,
                message,
            } => {
                let _ = writeln!(log_file, "[         {level: <5} {stdio}] {message}");
            }
            LogMessage::Standard {
                timestamp,
                level,
                thread_name,
                target,
                filename,
                line_number,
                fields,
            } => {
                let mut message = fields
                    .get("message")
                    .map(|v| {
                        if let Some(msg) = v.as_str() {
                            msg.replace('\n', "\n    ")
                        } else {
                            v.to_string()
                        }
                    })
                    .unwrap_or_default();

                if message.is_empty() || fields.len() > 1 {
                    message += "    {";
                    for (k, v) in fields {
                        if k == "message" {
                            continue;
                        }
                        message += &format!(" {k}: {v},");
                    }
                    message += " }";
                }
                let _ = writeln!(log_file, "[{timestamp: >7.2}s {level: <5} {target: <10} {thread_name: <12} {filename}:{line_number}] {message}");
            }
        }
    }
    let _ = log_file.flush();
}
