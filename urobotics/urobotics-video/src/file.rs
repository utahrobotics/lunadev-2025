use std::{borrow::Cow, path::Path};

use ffmpeg_sidecar::command::FfmpegCommand;

use crate::{
    error::VideoDumpInitError,
    properties::{Audio, ScalingFilter, VideoQuality},
    VideoDataDump, VideoDataDumpType,
};

pub struct VideoFileBuilder<P> {
    pub in_width: u32,
    pub in_height: u32,
    pub out_width: u32,
    pub out_height: u32,
    pub scale_filter: ScalingFilter,
    pub path: P,
    pub fps: usize,
    pub codec: Cow<'static, str>,
    pub max_bitrate: Option<usize>,
    pub quality: VideoQuality,
    pub audio_sample_rate: usize,
    pub audio: Audio,
    pub audio_format: Cow<'static, str>,
    pub audio_source: Cow<'static, str>,
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
            quality: VideoQuality::Average,
            max_bitrate: None,
            audio_sample_rate: 48000,
            audio: Audio::None,
            audio_format: "s16le".into(),
            audio_source: "".into(),
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
            quality: VideoQuality::Average,
            max_bitrate: None,
            audio_sample_rate: 48000,
            audio: Audio::None,
            audio_format: "s16le".into(),
            audio_source: "".into(),
        }
    }
}

impl<P: AsRef<Path>> VideoFileBuilder<P> {
    pub fn build(&self) -> Result<VideoDataDump, VideoDumpInitError> {
        ffmpeg_sidecar::download::auto_download()
            .map_err(|e| VideoDumpInitError::FFMPEGInstallError(e.to_string()))?;

        let mut cmd = FfmpegCommand::new();

        cmd.hwaccel("auto")
            .format("rawvideo")
            .pix_fmt("rgb24")
            .size(self.in_width, self.in_height)
            .input("-")
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
            .codec_video(&self.codec);

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
