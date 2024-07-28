//! Data dumps are an alternative way of logging that is more suited to
//! large collections of data.
//!
//! Data dumps offer a way to write data to some location such that the
//! code producing the data does not get blocked by writing. If the write
//! is queued successfully, then the write is guaranteed to occur, as long
//! as the current program is not forcefully terminated.

use std::{
    borrow::Cow,
    error::Error,
    fmt::Display,
    net::SocketAddrV4,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crossbeam::{atomic::AtomicCell, utils::Backoff};
pub use ffmpeg_sidecar;
use ffmpeg_sidecar::{child::FfmpegChild, command::FfmpegCommand, event::FfmpegEvent};
use image::{DynamicImage, EncodableLayout};
use log::{error, info, warn};
use minifb::{Window, WindowOptions};
use tokio::{io::AsyncWriteExt, process::ChildStdin};
use urobotics_core::RuntimeDropGuard;

/// An error faced while writing video frames.
#[derive(Debug)]
pub enum VideoWriteError {
    /// The size of the given image is incorrect.
    IncorrectDimensions {
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
    IOError(std::io::Error),
    /// There is no information attached to the error.
    Unknown,
}

impl Display for VideoWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "The video writing thread has failed for some reason"),
            Self::IncorrectDimensions { expected_width: expected_x, expected_height: expected_y, actual_width: actual_x, actual_height: actual_y } => write!(f, "Image dimensions are wrong. Expected {expected_x}x{expected_y}, got {actual_x}x{actual_y}"),
            VideoWriteError::IOError(e) => write!(f, "An IO error occurred while writing the video frame: {e}"),
        }
    }
}
impl Error for VideoWriteError {}

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

/// An error faced while initializing a `VideoDataDump`.
#[derive(Debug)]
pub enum VideoDumpInitError {
    /// An error writing to or reading from `ffmpeg`.
    IOError(std::io::Error),
    /// An error from `ffmpeg` while it was encoding the video.
    VideoError(String),
    /// An error setting up the logging for the dump.
    FFMPEGInstallError(String),
}

impl Error for VideoDumpInitError {}
impl Display for VideoDumpInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoDumpInitError::IOError(e) => {
                write!(f, "Faced an error initializing the video encoder: {e}")
            }
            VideoDumpInitError::FFMPEGInstallError(e) => write!(
                f,
                "Faced an error installing FFMPEG for the video encoder: {e}"
            ),
            VideoDumpInitError::VideoError(e) => {
                write!(f, "Faced an error while encoding video: {e}")
            }
        }
    }
}

/// The type of filter used when scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalingFilter {
    /// Nearest neighbor. Excellent for performance.
    ///
    /// This adds no blurring whatsoever when upscaling, and mediocre quality when downscaling.
    Neighbor,
    /// Uses a fast bilinear algorithm. Good for performance.
    ///
    /// This adds some blurring when upscaling, and average quality when downscaling.
    FastBilinear,
}

impl std::fmt::Display for ScalingFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Neighbor => write!(f, "neighbor"),
            Self::FastBilinear => write!(f, "fast_bilinear"),
        }
    }
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

pub struct VideoFileBuilder<P> {
    pub in_width: u32,
    pub in_height: u32,
    pub out_width: u32,
    pub out_height: u32,
    pub scale_filter: ScalingFilter,
    pub path: P,
    pub fps: usize,
    pub codec: Cow<'static, str>,
}

impl<P> VideoFileBuilder<P> {
    pub fn new(in_width: u32, in_height: u32, path: P) -> Self {
        Self {
            in_width,
            in_height,
            out_width: in_width,
            out_height: in_height,
            scale_filter: ScalingFilter::FastBilinear,
            path,
            fps: 30,
            codec: "libx265".into(),
        }
    }

    pub fn path<P2>(self, path: P2) -> VideoFileBuilder<P2> {
        VideoFileBuilder {
            in_width: self.in_width,
            in_height: self.in_height,
            out_width: self.out_width,
            out_height: self.out_height,
            scale_filter: self.scale_filter,
            path,
            fps: self.fps,
            codec: self.codec,
        }
    }
}

impl<P: AsRef<Path>> VideoFileBuilder<P> {
    pub fn build(&self) -> Result<VideoDataDump, VideoDumpInitError> {
        ffmpeg_sidecar::download::auto_download()
            .map_err(|e| VideoDumpInitError::FFMPEGInstallError(e.to_string()))?;

        let output = FfmpegCommand::new()
            .hwaccel("auto")
            .format("rawvideo")
            .pix_fmt("rgb24")
            .size(self.in_width, self.in_height)
            .input("-")
            .args([
                "-vf",
                &format!(
                    "fps={},scale={}:{}",
                    self.fps, self.out_width, self.out_height
                ),
                "-sws_flags",
                &self.scale_filter.to_string(),
            ])
            .codec_video(&self.codec)
            .args(["-y".as_ref(), self.path.as_ref().as_os_str()])
            .spawn()
            .map_err(VideoDumpInitError::IOError)?;

        VideoDataDump::new(
            self.in_width,
            self.in_height,
            VideoDataDumpType::File(self.path.as_ref().into()),
            output,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RtspTransport {
    Tcp,
    Udp,
}

impl Display for RtspTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RtspTransport::Tcp => write!(f, "tcp"),
            RtspTransport::Udp => write!(f, "udp"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RtpQuality {
    VeryLow,
    Low,
    Average,
    High,
    VeryHigh,
    Custom(usize),
}

impl From<RtpQuality> for usize {
    fn from(value: RtpQuality) -> Self {
        match value {
            RtpQuality::VeryLow => 37,
            RtpQuality::Low => 33,
            RtpQuality::Average => 28,
            RtpQuality::High => 23,
            RtpQuality::VeryHigh => 17,
            RtpQuality::Custom(x) => x,
        }
    }
}

pub struct RtpVideoBuilder {
    pub in_width: u32,
    pub in_height: u32,
    pub out_width: u32,
    pub out_height: u32,
    pub scale_filter: ScalingFilter,
    pub addr: SocketAddrV4,
    pub fps: usize,
    pub codec: Cow<'static, str>,
    pub i_frame_interval: usize,
    pub rtsp_transport: RtspTransport,
    pub preset: Cow<'static, str>,
    pub tune: Option<Cow<'static, str>>,
    pub pixel_format: Cow<'static, str>,
    pub quality: RtpQuality,
    pub max_bitrate: Option<usize>,
    pub audio_sample_rate: usize,
    pub audio: RtpAudio,
    pub audio_format: Cow<'static, str>,
    pub audio_source: Cow<'static, str>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RtpAudio {
    None,
    Mono,
    Stereo,
}

impl RtpVideoBuilder {
    pub fn new_low_latency(in_width: u32, in_height: u32, addr: SocketAddrV4) -> Self {
        Self {
            in_width,
            in_height,
            out_width: in_width,
            out_height: in_height,
            scale_filter: ScalingFilter::FastBilinear,
            addr,
            fps: 24,
            codec: "libx264".into(),
            tune: Some("zerolatency".into()),
            preset: "ultrafast".into(),
            i_frame_interval: 10,
            rtsp_transport: RtspTransport::Udp,
            pixel_format: "yuv420p".into(),
            quality: RtpQuality::Low,
            max_bitrate: None,
            audio_sample_rate: 48000,
            audio: RtpAudio::None,
            audio_format: "s16le".into(),
            audio_source: "".into(),
        }
    }
    pub fn new(in_width: u32, in_height: u32, addr: SocketAddrV4) -> Self {
        Self {
            in_width,
            in_height,
            out_width: in_width,
            out_height: in_height,
            scale_filter: ScalingFilter::FastBilinear,
            addr,
            fps: 24,
            codec: "libx265".into(),
            i_frame_interval: 200,
            rtsp_transport: RtspTransport::Udp,
            preset: "fast".into(),
            tune: None,
            pixel_format: "yuv420p".into(),
            quality: RtpQuality::Average,
            max_bitrate: None,
            audio_sample_rate: 48000,
            audio: RtpAudio::None,
            audio_format: "s16le".into(),
            audio_source: "".into(),
        }
    }
    pub fn new_reliable(in_width: u32, in_height: u32, addr: SocketAddrV4) -> Self {
        Self {
            in_width,
            in_height,
            out_width: in_width,
            out_height: in_height,
            scale_filter: ScalingFilter::FastBilinear,
            addr,
            fps: 24,
            codec: "libx265".into(),
            i_frame_interval: 50,
            rtsp_transport: RtspTransport::Tcp,
            preset: "medium".into(),
            tune: None,
            pixel_format: "yuv420p".into(),
            quality: RtpQuality::High,
            max_bitrate: Some(1000),
            audio_sample_rate: 48000,
            audio: RtpAudio::None,
            audio_format: "s16le".into(),
            audio_source: "".into(),
        }
    }
}

impl RtpVideoBuilder {
    pub async fn build(&self) -> Result<(VideoDataDump, String), VideoDumpInitError> {
        ffmpeg_sidecar::download::auto_download()
            .map_err(|e| VideoDumpInitError::FFMPEGInstallError(e.to_string()))?;

        let mut cmd = FfmpegCommand::new();

        cmd.hwaccel("auto")
            .format("rawvideo")
            .pix_fmt("rgb24")
            .size(self.in_width, self.in_height)
            .input("-")
            .codec_video(&self.codec)
            .pix_fmt(&self.pixel_format)
            .args([
                "-crf",
                &usize::from(self.quality).to_string(),
                "-vf",
                &format!(
                    "fps={},scale={}:{}",
                    self.fps, self.out_width, self.out_height
                ),
                "-sws_flags",
                &self.scale_filter.to_string(),
            ])
            .preset(&self.preset);

        if let Some(tune) = &self.tune {
            cmd.args(["-tune", tune]);
        }

        if let Some(max_bitrate) = self.max_bitrate {
            cmd.args([
                "-maxrate",
                &format!("{}K", max_bitrate),
                "-bufsize",
                &format!("{}K", max_bitrate * 2),
            ]);
        }

        match self.audio {
            RtpAudio::None => {}
            RtpAudio::Stereo | RtpAudio::Mono => {
                let channels = if self.audio == RtpAudio::Stereo { 2 } else { 1 };
                cmd.args([
                    "-ar",
                    &self.audio_sample_rate.to_string(),
                    "-ac",
                    &channels.to_string(),
                    "-f",
                    self.audio_format.as_ref(),
                    "-i",
                    self.audio_source.as_ref(),
                ]);
            }
        }

        let output = cmd
            .args([
                "-strict",
                "2",
                "-avioflags",
                "direct",
                "-rtsp_transport",
                &self.rtsp_transport.to_string(),
                "-g",
                &self.i_frame_interval.to_string(),
                "-sdp_file",
                "rtp.sdp",
            ])
            .format("rtp")
            .output(format!("rtp://{}", self.addr))
            .spawn()
            .map_err(VideoDumpInitError::IOError)?;

        let sdp_path = PathBuf::from("rtp.sdp");
        while !sdp_path.exists() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let sdp = tokio::fs::read_to_string(&sdp_path)
            .await
            .map_err(VideoDumpInitError::IOError)?;

        VideoDataDump::new(
            self.in_width,
            self.in_height,
            VideoDataDumpType::Rtp(self.addr),
            output,
        )
        .map(|dump| (dump, sdp))
    }
}
