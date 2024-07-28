use std::{borrow::Cow, net::SocketAddrV4, path::PathBuf, sync::Arc};

use crossbeam::{atomic::AtomicCell, utils::Backoff};
pub use ffmpeg_sidecar;
use ffmpeg_sidecar::{child::FfmpegChild, event::FfmpegEvent};
use image::{DynamicImage, EncodableLayout};
use log::{error, info, warn};
use minifb::{Window, WindowOptions};
use tokio::{io::AsyncWriteExt, process::ChildStdin};
use urobotics_core::RuntimeDropGuard;

pub mod error;
pub mod file;
pub mod info;
pub mod properties;
pub mod rtp;

use error::{VideoDumpInitError, VideoWriteError};

#[derive(Clone)]
pub enum VideoDataDumpType {
    Rtp(SocketAddrV4),
    File(PathBuf),
    Custom(Cow<'static, str>),
}

impl std::fmt::Display for VideoDataDumpType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoDataDumpType::Rtp(addr) => write!(f, "{addr}"),
            VideoDataDumpType::File(path) => {
                write!(f, "{}", path.file_name().unwrap().to_string_lossy())
            }
            VideoDataDumpType::Custom(name) => write!(f, "{name}"),
        }
    }
}

enum VideoWriter {
    Queue(Arc<AtomicCell<Option<DynamicImage>>>),
    Ffmpeg(ChildStdin),
}

/// A dump for writing images into videos using `ffmpeg`.
///
/// If `ffmpeg` is not installed, it will be downloaded locally
/// automatically.
pub struct VideoDataDump {
    video_writer: VideoWriter,
    width: u32,
    height: u32,
}

impl VideoDataDump {
    /// Creates a new `VideoDataDump` that displays to a window.
    pub fn new_display(
        window_name: impl Into<Cow<'static, str>>,
        in_width: u32,
        in_height: u32,
        bgr: bool,
    ) -> Result<Self, VideoDumpInitError> {
        let window_name = window_name.into();
        let queue_sender = Arc::new(AtomicCell::<Option<DynamicImage>>::new(None));
        let queue_receiver = queue_sender.clone();

        let backoff = Backoff::new();

        std::thread::spawn(move || {
            let mut window = match Window::new(
                &window_name,
                in_width as usize,
                in_height as usize,
                WindowOptions {
                    resize: true,
                    scale_mode: minifb::ScaleMode::AspectRatioStretch,
                    ..Default::default()
                },
            ) {
                Ok(x) => x,
                Err(e) => {
                    error!("Faced the following error while creating display: {e}");
                    return;
                }
            };
            let mut buffer = Vec::with_capacity(in_width as usize * in_height as usize);
            loop {
                let Some(frame) = queue_receiver.take() else {
                    if Arc::strong_count(&queue_receiver) == 1 {
                        break;
                    }
                    backoff.snooze();
                    continue;
                };
                backoff.reset();
                if bgr {
                    buffer.extend(
                        frame
                            .to_rgb8()
                            .chunks(3)
                            .map(|x| [x[2], x[1], x[0], 0])
                            .map(u32::from_ne_bytes),
                    );
                } else {
                    buffer.extend(
                        frame
                            .to_rgb8()
                            .chunks(3)
                            .map(|x| [x[0], x[1], x[2], 0])
                            .map(u32::from_ne_bytes),
                    );
                }

                if let Err(e) =
                    window.update_with_buffer(&buffer, in_width as usize, in_height as usize)
                {
                    error!("Faced the following error while writing video frame to display: {e}");
                    break;
                }
                buffer.clear();
            }
        });

        Ok(Self {
            video_writer: VideoWriter::Queue(queue_sender),
            width: in_width,
            height: in_height,
        })
    }

    pub fn new(
        in_width: u32,
        in_height: u32,
        dump_type: VideoDataDumpType,
        mut output: FfmpegChild,
    ) -> Result<Self, VideoDumpInitError> {
        let events = output
            .iter()
            .map_err(|e| VideoDumpInitError::VideoError(e.to_string()))?;

        let dump_type2 = dump_type.clone();

        std::thread::spawn(move || {
            let _drop = RuntimeDropGuard::default();
            events.for_each(|event| {
                if let FfmpegEvent::Log(level, msg) = event {
                    match level {
                        ffmpeg_sidecar::event::LogLevel::Info => info!("[{dump_type2}] {msg}"),
                        ffmpeg_sidecar::event::LogLevel::Warning => warn!("[{dump_type2}] {msg}"),
                        ffmpeg_sidecar::event::LogLevel::Unknown => {}
                        _ => error!("[{dump_type2}] {msg}"),
                    }
                }
            });
        });

        Ok(Self {
            video_writer: VideoWriter::Ffmpeg(
                ChildStdin::from_std(output.take_stdin().unwrap())
                    .expect("Failed to convert stdin to async"),
            ),
            width: in_width,
            height: in_height,
        })
    }

    /// Writes an image into this dump.
    pub async fn write_frame(&mut self, frame: &DynamicImage) -> Result<(), VideoWriteError> {
        if frame.width() != self.width || frame.height() != self.height {
            return Err(VideoWriteError::IncorrectDimensions {
                expected_width: self.width,
                expected_height: self.height,
                actual_width: frame.width(),
                actual_height: frame.height(),
            });
        }

        match &mut self.video_writer {
            VideoWriter::Queue(queue) => {
                if Arc::strong_count(queue) == 1 {
                    return Err(VideoWriteError::Unknown);
                }
                queue.store(Some(frame.clone()));
                Ok(())
            }
            VideoWriter::Ffmpeg(child) => {
                let tmp;
                let frame_rgb;
                match frame {
                    DynamicImage::ImageRgb8(img) => frame_rgb = img,
                    _ => {
                        tmp = frame.to_rgb8();
                        frame_rgb = &tmp;
                    }
                }
                child
                    .write_all(frame_rgb.as_bytes())
                    .await
                    .map_err(VideoWriteError::IOError)
            }
        }
    }

    /// Writes an image into this dump.
    pub async fn write_raw(&mut self, frame: &[u8]) -> Result<(), VideoWriteError> {
        match &mut self.video_writer {
            VideoWriter::Queue(_) => unimplemented!(),
            VideoWriter::Ffmpeg(child) => child
                .write_all(frame)
                .await
                .map_err(VideoWriteError::IOError),
        }
    }
}
