#![feature(exclusive_wrapper)]
//! This crate offers several ways to interface with serial ports under
//! the Unros framwork.

use std::{borrow::Cow, sync::{Arc, Exclusive, OnceLock}};

pub use bytes::Bytes;
use serde::Deserialize;
use tokio_serial::{SerialPort, SerialPortBuilderExt, SerialStream};
use urobotics_core::{define_shared_callbacks, function::AsyncFunctionConfig, runtime::RuntimeContext, tokio::{self, io::{AsyncReadExt, WriteHalf}}};


define_shared_callbacks!(BytesCallbacks => FnMut(bytes: &[u8]) + Send + Sync);

/// A single duplex connection to a serial port
#[derive(Deserialize)]
pub struct SerialConnection {
    pub path: Cow<'static, str>,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,

    #[serde(skip)]
    serial_output: BytesCallbacks,
    #[serde(skip)]
    serial_input: Arc<OnceLock<Exclusive<WriteHalf<SerialStream>>>>
}

fn default_baud_rate() -> u32 {
    115200
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
            Ok(Arc::try_unwrap(self.0).unwrap().into_inner().unwrap().into_inner())
        } else {
            Err(self)
        }
    }
}

impl AsyncFunctionConfig for SerialConnection {
    type Output = std::io::Result<()>;
    const PERSISTENT: bool = false;

    async fn run(self, _context: &RuntimeContext) -> Self::Output {
        #[allow(unused_mut)]
        let mut stream = tokio_serial::new(self.path, self.baud_rate).open_native_async()?;
        #[cfg(unix)]
        stream.set_exclusive(true)?;
        stream.clear(tokio_serial::ClearBuffer::All)?;
        let (mut reader, writer) = tokio::io::split(stream);
        let _ = self.serial_input.set(Exclusive::new(writer));
        drop(self.serial_input);

        loop {
            let mut buf = [0; 1024];
            let n = reader.read(&mut buf).await?;
            self.serial_output.call(buf.split_at(n).0);
        }
    }
}
