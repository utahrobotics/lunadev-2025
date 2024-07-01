use std::{
    ffi::OsString,
    future::Future,
    io::Write,
    path::{Path, PathBuf},
};

use bytes::BytesMut;
use serde::Deserialize;
use urobotics_app::Application;
use urobotics_core::{
    service::{Service, ServiceExt},
    tokio::{
        self,
        io::{AsyncReadExt, AsyncWriteExt},
    },
    BlockOn,
};

#[derive(Deserialize)]
pub struct PythonVenvBuilder {
    #[serde(default = "default_venv_path")]
    pub venv_path: PathBuf,
    #[serde(default = "default_system_interpreter")]
    pub system_interpreter: OsString,
    #[serde(default)]
    pub packages_to_install: Vec<String>,
}

fn default_venv_path() -> PathBuf {
    "urobotics-venv".into()
}

fn default_system_interpreter() -> OsString {
    "python".into()
}

impl Default for PythonVenvBuilder {
    fn default() -> Self {
        PythonVenvBuilder {
            venv_path: default_venv_path(),
            system_interpreter: default_system_interpreter(),
            packages_to_install: Vec::new(),
        }
    }
}

impl PythonVenvBuilder {
    pub async fn build(&self) -> std::io::Result<PythonVenv> {
        if !Path::new(&self.venv_path).exists() {
            let std::process::Output { status, stderr, .. } =
                tokio::process::Command::new(&self.system_interpreter)
                    .args(["-m", "venv"])
                    .arg(&self.venv_path)
                    .output()
                    .await?;

            if !status.success() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "Failed to create virtual environment: {}",
                        String::from_utf8(stderr).expect("Invalid UTF-8 in stderr")
                    ),
                ));
            }
        }
        let venv = PythonVenv {
            path: Path::new(&self.venv_path).join("Scripts//python"),
        };

        for package in &self.packages_to_install {
            venv.pip_install(package).await?;
        }

        Ok(venv)
    }
}

pub struct PythonVenv {
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum PythonValue {
    Int(i64),
    Float(f64),
    String(String),
    Bytes(BytesMut),
    None,
}
#[cfg(unix)]
const TERMINATOR: &'static [u8] = b">>>END=REPL<<<\n";
#[cfg(not(unix))]
const TERMINATOR: &'static [u8] = b">>>END=REPL<<<\r\n";

impl std::fmt::Display for PythonValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PythonValue::Int(value) => write!(f, "{}", value),
            PythonValue::Float(value) => write!(f, "{}", value),
            PythonValue::String(value) => write!(f, "{}", value),
            PythonValue::None => write!(f, "None"),
            PythonValue::Bytes(value) => write!(f, "{:?}", value),
        }
    }
}

impl PythonVenv {
    pub async fn pip_install(&self, package: &str) -> std::io::Result<()> {
        let std::process::Output { status, stderr, .. } = tokio::process::Command::new(&self.path)
            .args(["-m", "pip", "install", package])
            .output()
            .await?;
        if !status.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Failed to create virtual environment: {}",
                    String::from_utf8(stderr).expect("Invalid UTF-8 in stderr")
                ),
            ));
        }
        Ok(())
    }

    pub async fn repl(&self) -> std::io::Result<PyRepl> {
        let child = tokio::process::Command::new(&self.path)
            .arg("-c")
            .arg(include_str!("repl.py"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let stdin = child.stdin.unwrap();
        let stdout = child.stdout.unwrap();

        Ok(PyRepl {
            stdin,
            stdout,
            buffer: BytesMut::new(),
        })
    }
}

pub struct PyRepl {
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    buffer: BytesMut,
}

impl Service for PyRepl {
    type ScheduleData<'a> = &'a str;
    type TaskData<'a> = ();
    type Output<'a> = std::io::Result<PythonValue>;

    fn call_with_data<'a>(
        &'a mut self,
        data: Self::ScheduleData<'a>,
    ) -> (
        Self::TaskData<'a>,
        impl Future<Output = Self::Output<'a>> + Send,
    ) {
        ((), async {
            self.stdin.write_all(data.as_bytes()).await?;
            self.stdin.write_all(b"\n__end_repl__()\n").await?;
            loop {
                self.stdout.read_buf(&mut self.buffer).await?;

                if self.buffer.ends_with(TERMINATOR) {
                    self.buffer.truncate(self.buffer.len() - TERMINATOR.len());
                    if let Ok(mut msg) = std::str::from_utf8(&self.buffer) {
                        if msg.ends_with("\r\n") {
                            msg = msg.split_at(msg.len() - 2).0;
                        }
                        if let Ok(value) = msg.parse::<i64>() {
                            self.buffer.clear();
                            return Ok(PythonValue::Int(value));
                        } else if let Ok(value) = msg.parse::<f64>() {
                            self.buffer.clear();
                            return Ok(PythonValue::Float(value));
                        } else if msg.trim().is_empty() {
                            self.buffer.clear();
                            return Ok(PythonValue::None);
                        } else {
                            let msg = msg.to_string();
                            self.buffer.clear();
                            return Ok(PythonValue::String(msg));
                        }
                    } else {
                        let buffer = self.buffer.clone();
                        self.buffer.clear();
                        return Ok(PythonValue::Bytes(buffer));
                    }
                }
            }
        })
    }
}

impl Application for PythonVenvBuilder {
    const APP_NAME: &'static str = "python";
    const DESCRIPTION: &'static str = "Python virtual environment REPL";

    fn run(self) {
        let venv = self
            .build()
            .block_on()
            .expect("Failed to build Python venv");
        let mut repl = venv.repl().block_on().expect("Failed to start Python REPL");

        let _ = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let mut input = String::new();
                    loop {
                        print!(">>> ");
                        std::io::stdout().flush().unwrap();
                        std::io::stdin()
                            .read_line(&mut input)
                            .expect("Failed to read line");
                        let result = repl.call(&input).await;
                        input.clear();
                        println!("{}", result.unwrap());
                    }
                })
        })
        .join();
    }
}
