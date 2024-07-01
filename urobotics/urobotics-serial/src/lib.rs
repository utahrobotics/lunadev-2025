#![feature(exclusive_wrapper, never_type)]
//! This crate offers several ways to interface with serial ports under
//! the Unros framwork.

use std::{
    borrow::Cow,
    sync::{Arc, Exclusive, OnceLock},
};

pub use bytes::Bytes;
use bytes::BytesMut;
use crossbeam::utils::Backoff;
use serde::Deserialize;
use tokio_serial::{SerialPort, SerialPortBuilderExt, SerialStream};
use urobotics_app::Application;
use urobotics_core::{
    define_callbacks,
    task::AsyncTask,
    tokio::{
        self,
        io::{AsyncReadExt, WriteHalf},
    },
    BlockOn,
};

define_callbacks!(BytesCallbacks => Fn(bytes: &[u8]) + Send + Sync);

/// A single duplex connection to a serial port
#[derive(Deserialize)]
pub struct SerialConnection {
    pub path: Cow<'static, str>,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,

    #[serde(skip)]
    serial_output: BytesCallbacks,
    #[serde(skip)]
    serial_input: Arc<OnceLock<Exclusive<WriteHalf<SerialStream>>>>,
}

fn default_baud_rate() -> u32 {
    115200
}

fn default_buffer_size() -> usize {
    1024
}

impl SerialConnection {
    /// Creates a pending connection to a serial port.
    ///
    /// The connection is not actually made until this node is ran.
    /// If `tolerate_error` is `true`, then errors are ignored and
    /// actions are retried.
    pub fn new(path: impl Into<Cow<'static, str>>) -> Self {
        Self {
            baud_rate: default_baud_rate(),
            serial_output: BytesCallbacks::default(),
            serial_input: Arc::default(),
            path: path.into(),
            buffer_size: default_buffer_size(),
        }
    }

    pub fn take_writer(&mut self) -> Option<PendingWriter> {
        if Arc::strong_count(&self.serial_input) > 1 {
            None
        } else {
            Some(PendingWriter(self.serial_input.clone()))
        }
    }
}

pub struct PendingWriter(Arc<OnceLock<Exclusive<WriteHalf<SerialStream>>>>);

impl PendingWriter {
    pub fn try_unwrap(self) -> Result<WriteHalf<SerialStream>, Self> {
        if Arc::strong_count(&self.0) == 1 && self.0.get().is_some() {
            Ok(Arc::try_unwrap(self.0)
                .unwrap()
                .into_inner()
                .unwrap()
                .into_inner())
        } else {
            Err(self)
        }
    }

    pub fn blocking_unwrap(mut self) -> WriteHalf<SerialStream> {
        let backoff = Backoff::default();
        loop {
            match self.try_unwrap() {
                Ok(writer) => break writer,
                Err(pending) => {
                    self = pending;
                    backoff.snooze();
                }
            }
        }
    }

    pub async fn unwrap(mut self) -> WriteHalf<SerialStream> {
        loop {
            match self.try_unwrap() {
                Ok(writer) => break writer,
                Err(pending) => {
                    self = pending;
                    tokio::task::yield_now().await;
                }
            }
        }
    }
}

impl AsyncTask for SerialConnection {
    type Output = std::io::Result<!>;

    async fn run(mut self) -> Self::Output {
        #[allow(unused_mut)]
        let mut stream = tokio_serial::new(self.path, self.baud_rate).open_native_async()?;
        #[cfg(unix)]
        stream.set_exclusive(true)?;
        stream.clear(tokio_serial::ClearBuffer::All)?;
        let (mut reader, writer) = tokio::io::split(stream);

        let mut buf = BytesMut::with_capacity(self.buffer_size);

        let _ = self.serial_input.set(Exclusive::new(writer));
        drop(self.serial_input);
        loop {
            reader.read_buf(&mut buf).await?;
            self.serial_output.call(&buf);
            buf.clear();
        }
    }
}

impl Application for SerialConnection {
    const APP_NAME: &'static str = "serial";
    const DESCRIPTION: &'static str = "Connects to a serial port and reads data from it";

    fn run(self) {
        let fut = async move {
            macro_rules! expect {
                ($e:expr) => {
                    match $e {
                        Ok(x) => x,
                        Err(e) => {
                            urobotics_core::log::error!("{}", e);
                            return;
                        }
                    }
                };
            }
            #[allow(unused_mut)]
            let mut stream =
                expect!(tokio_serial::new(self.path, self.baud_rate).open_native_async());
            #[cfg(unix)]
            expect!(stream.set_exclusive(true));
            expect!(stream.clear(tokio_serial::ClearBuffer::All));
            let (mut reader, mut writer) = tokio::io::split(stream);

            let mut buf = BytesMut::with_capacity(self.buffer_size);
            std::thread::spawn(move || {
                let mut builder = tokio::runtime::Builder::new_current_thread();
                builder.enable_all();
                builder.build().unwrap().block_on(async {
                    expect!(tokio::io::copy(&mut tokio::io::stdin(), &mut writer).await);
                });
            });
            loop {
                expect!(reader.read_buf(&mut buf).await);
                if let Ok(msg) = std::str::from_utf8(&buf) {
                    print!("{msg}");
                    buf.clear();
                }
            }
        };
        fut.block_on();
    }
}
