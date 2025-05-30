use crate::*;
use serde::{Serialize, Deserialize};
use iceoryx2_bb_container::vec::FixedSizeVec;
use iceoryx2_bb_container::byte_string::FixedSizeByteString;

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct Points3D<const N: usize> {
    pub positions: FixedSizeVec<Position3D,N>,
    pub colors: FixedSizeVec<Color, N>,
    pub radii: FixedSizeVec<f32, N>,
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct Boxes3D<const N: usize> {
    pub centers: FixedSizeVec<Position3D, N>,
    pub half_sizes: FixedSizeVec<Position3D, N>,
    pub quaternions: FixedSizeVec<Quaternion, N>,
    pub colors: FixedSizeVec<Color, N>,
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct Arrows3D<const N: usize> {
    pub vectors: FixedSizeVec<Position3D, N>,
    pub origins: FixedSizeVec<Position3D, N>,
    pub colors: FixedSizeVec<Color, N>,
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct ViewCoordinates {
    pub coordinates: [u8; 3], // XYZ representation as bytes
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct Asset3D<const N: usize> {
    pub data: FixedSizeByteString<N>,
    pub media_type: FixedSizeByteString<64>, // MIME type like "model/stl"
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct TextLog<const N: usize> {
    pub text: FixedSizeByteString<N>,
    pub level: Level
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct Transform3D {
    pub position: Position3D,
    pub rotation: Quaternion,
    #[serde(default = "default_scale")]
    pub scale: f32
}

fn default_scale() -> f32 {
    1.0
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct Pinhole {
    pub focal_length: [f32; 2],
    pub resolution: [f32; 2],
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct DepthImage<const N: usize> {
    pub bytes: FixedSizeVec<u16, N>,
    pub image_format: ImageFormat,
    /// How many meters does one unit represent? 
    /// Used for depth-to-world conversion. Default: 1.0
    #[serde(default = "default_meter")]
    pub meter: f32,
    /// The expected range of depth values in meters
    /// Used to help the viewer optimize the display of depth data
    /// Default: [0.0, 10.0]
    #[serde(default = "default_depth_range")]
    pub depth_range: [f64; 2],
}

fn default_meter() -> f32 {
    1.0
}

fn default_depth_range() -> [f64; 2] {
    [0.0, 10.0]
}

impl<const N: usize> Points3D<N> {
    pub fn with_colors(mut self, colors: FixedSizeVec<Color, N>) -> Self {
        self.colors = colors;
        self
    }
    pub fn with_radii(mut self, radii: FixedSizeVec<f32, N>) -> Self {
        self.radii = radii;
        self
    }
}

impl<const N: usize> Boxes3D<N> {
    pub fn with_colors(mut self, colors: FixedSizeVec<Color, N>) -> Self {
        self.colors = colors;
        self
    }
}

impl<const N: usize> Arrows3D<N> {
    pub fn from_vectors(vectors: FixedSizeVec<Position3D, N>) -> Self {
        let mut origins = FixedSizeVec::new();
        for _ in 0..vectors.len() {
            let _ = origins.push(Position3D { x: 0.0, y: 0.0, z: 0.0 });
        }
        Self {
            vectors,
            origins,
            colors: FixedSizeVec::new(),
        }
    }
    
    pub fn with_colors(mut self, colors: FixedSizeVec<Color, N>) -> Self {
        self.colors = colors;
        self
    }
    
    pub fn with_origins(mut self, origins: FixedSizeVec<Position3D, N>) -> Self {
        self.origins = origins;
        self
    }
}

impl ViewCoordinates {
    pub fn right_hand_y_up() -> Self {
        Self {
            coordinates: [0, 1, 2], // RUF (Right, Up, Forward)
        }
    }
}

impl<const N: usize> Asset3D<N> {
    pub fn from_bytes(data: &[u8], media_type: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            data: FixedSizeByteString::from_bytes(data)?,
            media_type: FixedSizeByteString::from_bytes(media_type.as_bytes())?,
        })
    }
}

impl<const N: usize> TextLog<N> {
    pub fn with_level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }
}

impl<const N: usize> DepthImage<N> {
    pub fn new(bytes: FixedSizeVec<u16, N>, image_format: ImageFormat) -> Self {
        Self {
            bytes,
            image_format,
            meter: 1.0,
            depth_range: [0.0, 10.0],
        }
    }

    pub fn with_meter(mut self, meter: f32) -> Self {
        self.meter = meter;
        self
    }

    pub fn with_depth_range(mut self, depth_range: [f64; 2]) -> Self {
        self.depth_range = depth_range;
        self
    }
}