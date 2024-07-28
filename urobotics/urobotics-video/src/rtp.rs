use std::{borrow::Cow, net::SocketAddrV4, path::PathBuf, time::Duration};

use ffmpeg_sidecar::command::FfmpegCommand;

use crate::{
    error::VideoDumpInitError,
    properties::{Audio, RtspTransport, ScalingFilter, VideoQuality},
    VideoDataDump, VideoDataDumpType,
};

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
    pub quality: VideoQuality,
    pub max_bitrate: Option<usize>,
    pub audio_sample_rate: usize,
    pub audio: Audio,
    pub audio_format: Cow<'static, str>,
    pub audio_source: Cow<'static, str>,
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
            quality: VideoQuality::Low,
            max_bitrate: None,
            audio_sample_rate: 48000,
            audio: Audio::None,
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
            quality: VideoQuality::Average,
            max_bitrate: None,
            audio_sample_rate: 48000,
            audio: Audio::None,
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
            quality: VideoQuality::High,
            max_bitrate: Some(1000),
            audio_sample_rate: 48000,
            audio: Audio::None,
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
            Audio::None => {}
            Audio::Stereo | Audio::Mono => {
                let channels = if self.audio == Audio::Stereo { 2 } else { 1 };
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
