use std::{error::Error, fmt::Display};

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
