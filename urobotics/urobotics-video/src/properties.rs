use std::fmt::Display;

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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Audio {
    None,
    Mono,
    Stereo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoQuality {
    VeryLow,
    Low,
    Average,
    High,
    VeryHigh,
    Custom(usize),
}

impl From<VideoQuality> for usize {
    fn from(value: VideoQuality) -> Self {
        match value {
            VideoQuality::VeryLow => 37,
            VideoQuality::Low => 33,
            VideoQuality::Average => 28,
            VideoQuality::High => 23,
            VideoQuality::VeryHigh => 17,
            VideoQuality::Custom(x) => x,
        }
    }
}
