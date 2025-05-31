use crate::*;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Points3D {
    pub positions: Vec<Position3D>,
    pub colors: Vec<Color>,
    pub radii: Vec<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Boxes3D {
    pub centers: Vec<Position3D>,
    pub half_sizes: Vec<Position3D>,
    pub quaternions: Vec<Quaternion>,
    pub colors: Vec<Color>,
    pub labels: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Arrows3D {
    pub vectors: Vec<Position3D>,
    pub origins: Vec<Position3D>,
    pub colors: Vec<Color>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct ViewCoordinates {
    pub coordinates: [u8; 3], // XYZ representation as bytes
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Asset3D {
    pub data: Vec<u8>,
    pub media_type: String, // MIME type like "model/stl"
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TextLog {
    pub text: String,
    pub level: Level
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Transform3D {
    pub translation: Position3D,
    pub rotation: Quaternion,
    #[serde(default = "default_scale")]
    pub scale: f32
}

fn default_scale() -> f32 {
    1.0
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Pinhole {
    pub focal_length: [f32; 2],
    pub resolution: [f32; 2],
}

impl Pinhole {
    pub fn from_focal_length_and_resolution(focal_length: [f32; 2], resolution: [f32; 2]) -> Self {
        Self {
            focal_length,
            resolution,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DepthImage {
    pub bytes: Vec<u8>,
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

impl Arrows3D {
    pub fn from_vectors(vectors: impl IntoIterator<Item = impl Into<Position3D>>) -> Self {
        let vectors: Vec<Position3D> = vectors.into_iter().map(|v| v.into()).collect();
        let origins = vec![Position3D { x: 0.0, y: 0.0, z: 0.0 }; vectors.len()];
        Self {
            vectors,
            origins,
            colors: Vec::new(),
        }
    }
    
    pub fn with_colors(mut self, colors: impl IntoIterator<Item = impl Into<Color>>) -> Self {
        self.colors = colors.into_iter().map(|c| c.into()).collect();
        self
    }
}

impl IntoRerunMessage for Arrows3D {
    fn into_rerun_message(self, entity_path: &str) -> RerunMessage {
        RerunMessage::Arrows3D(entity_path.to_string(), self)
    }
}

impl Points3D {
    pub fn new(positions: impl IntoIterator<Item = impl Into<Position3D>>) -> Self {
        Self {
            positions: positions.into_iter().map(|p| p.into()).collect(),
            colors: Vec::new(),
            radii: Vec::new(),
        }
    }
    
    /// Update only some specific fields of a `Points3D`.
    /// 
    /// This creates a new Points3D instance with all fields empty, 
    /// allowing you to selectively update only the fields you need.
    pub fn update_fields() -> Self {
        Self {
            positions: Vec::new(),
            colors: Vec::new(),
            radii: Vec::new(),
        }
    }
    
    pub fn with_colors(mut self, colors: impl IntoIterator<Item = impl Into<Color>>) -> Self {
        self.colors = colors.into_iter().map(|c| c.into()).collect();
        self
    }
    pub fn with_radii(mut self, radii: impl IntoIterator<Item = impl Into<f32>>) -> Self {
        self.radii = radii.into_iter().map(|r| r.into()).collect();
        self
    }
}

impl Transform3D {
    pub fn from_translation_rotation(translation: impl Into<Position3D>, rotation: Quaternion) -> Self {
        Self {
            translation: translation.into(),
            rotation,
            scale: 1.0,
        }
    }
    
    pub fn from_translation_rotation_scale(translation: impl Into<Position3D>, rotation: Quaternion, scale: f32) -> Self {
        Self {
            translation: translation.into(),
            rotation,
            scale,
        }
    }
}

impl IntoRerunMessage for Transform3D {
    fn into_rerun_message(self, entity_path: &str) -> RerunMessage {
        RerunMessage::Transform3D(entity_path.to_string(), self)
    }
}

impl Boxes3D {
    pub fn with_colors(mut self, colors: impl IntoIterator<Item = impl Into<Color>>) -> Self {
        self.colors = colors.into_iter().map(|c| c.into()).collect();
        self
    }
    
    pub fn with_quaternions(mut self, quaternions: impl IntoIterator<Item = impl Into<Quaternion>>) -> Self {
        self.quaternions = quaternions.into_iter().map(|q| q.into()).collect();
        self
    }
    
    pub fn from_centers_and_half_sizes(
        centers: impl IntoIterator<Item = impl Into<Position3D>>,
        half_sizes: impl IntoIterator<Item = impl Into<Position3D>>,
    ) -> Self {
        Self {
            centers: centers.into_iter().map(|c| c.into()).collect(),
            half_sizes: half_sizes.into_iter().map(|h| h.into()).collect(),
            quaternions: Vec::new(),
            colors: Vec::new(),
            labels: Vec::new(),
        }
    }
    
    pub fn with_labels(mut self, labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.labels = labels.into_iter().map(|l| l.into()).collect();
        self
    }
}

impl IntoRerunMessage for Boxes3D {
    fn into_rerun_message(self, entity_path: &str) -> RerunMessage {
        RerunMessage::Boxes3D(entity_path.to_string(), self)
    }
}

impl ViewCoordinates {
    pub fn right_hand_y_up() -> Self {
        Self {
            coordinates: [0, 1, 2], // RUF (Right, Up, Forward)
        }
    }
}

impl Asset3D {
    pub fn from_bytes(data: &[u8], media_type: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            data: data.to_vec(),
            media_type: media_type.to_string(),
        })
    }
}

impl TextLog {
    pub fn new(text: String) -> Self {
        Self {
            text,
            level: Level::Info,
        }
    }

    pub fn with_level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }
}

impl DepthImage {
    pub fn new(bytes: Vec<u8>, image_format: ImageFormat) -> Self {
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

impl IntoRerunMessage for Points3D {
    fn into_rerun_message(self, entity_path: &str) -> RerunMessage {
        RerunMessage::Points3D(entity_path.to_string(), self)
    }
}

impl IntoRerunMessage for ViewCoordinates {
    fn into_rerun_message(self, entity_path: &str) -> RerunMessage {
        RerunMessage::ViewCoordinates(entity_path.to_string(), self)
    }
}

impl IntoRerunMessage for Asset3D {
    fn into_rerun_message(self, entity_path: &str) -> RerunMessage {
        RerunMessage::Asset3D(entity_path.to_string(), self)
    }
}

impl IntoRerunMessage for TextLog {
    fn into_rerun_message(self, entity_path: &str) -> RerunMessage {
        RerunMessage::TextLog(entity_path.to_string(), self)
    }
}

impl IntoRerunMessage for Pinhole {
    fn into_rerun_message(self, entity_path: &str) -> RerunMessage {
        RerunMessage::Pinhole(entity_path.to_string(), self)
    }
}

impl IntoRerunMessage for DepthImage {
    fn into_rerun_message(self, entity_path: &str) -> RerunMessage {
        RerunMessage::DepthImage(entity_path.to_string(), self)
    }
}