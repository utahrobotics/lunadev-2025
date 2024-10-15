//! This crate offers several ways to interface with serial ports under
//! the Unros framwork.

use std::borrow::Cow;

pub use bytes::Bytes;
use bytes::BytesMut;
use serde::Deserialize;
use tokio_serial::{SerialPort, SerialPortBuilderExt, SerialStream};
use urobotics_app::Application;
use urobotics_core::{
    define_callbacks, log::error, tokio::{
        self,
        io::{AsyncReadExt, WriteHalf},
    }, BlockOn
};

define_callbacks!(BytesCallbacks => Fn(bytes: &[u8]) + Send + Sync);

/// A single duplex connection to a serial port.
#[derive(Deserialize)]
pub struct SerialConnection {
    /// The path to the serial port.
    pub path: Cow<'static, str>,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,

    #[serde(skip)]
    serial_output: BytesCallbacks,
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
            path: path.into(),
            buffer_size: default_buffer_size(),
        }
    }

    pub fn spawn(mut self) -> std::io::Result<WriteHalf<SerialStream>> {
        #[allow(unused_mut)]
        let mut stream = tokio_serial::new(self.path, self.baud_rate).open_native_async()?;
        #[cfg(unix)]
        stream.set_exclusive(true)?;
        stream.clear(tokio_serial::ClearBuffer::All)?;
        let (mut reader, writer) = tokio::io::split(stream);

        let mut buf = BytesMut::with_capacity(self.buffer_size);

        tokio::spawn(async move {
            loop {
                if let Err(e) = reader.read_buf(&mut buf).await {
                    error!("Error reading from serial port: {e}");
                }
                self.serial_output.call(&buf);
                buf.clear();
            }
        });

        Ok(writer)
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
