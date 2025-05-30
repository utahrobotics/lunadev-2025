pub mod archetypes;
use iceoryx2::prelude::ZeroCopySend;
use iceoryx2_bb_container::byte_string::FixedSizeByteString;
use iceoryx2_bb_container::vec::FixedSizeVec;
use serde::{Serialize, Deserialize};
use iceoryx2::prelude::*;
use iceoryx2::port::publisher::Publisher;
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct Color {
    #[serde(default = "default_color")]
    pub r: u8,
    #[serde(default = "default_color")]
    pub g: u8,
    #[serde(default = "default_color")]
    pub b: u8,
    #[serde(default = "default_color")]
    pub a: u8,
}

fn default_color() -> u8 {
    0
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct Position3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct Quaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl From<&str> for Level {
    fn from(s: &str) -> Self {
        match s {
            "info" => Self::Info,
            "warning" => Self::Warning,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub struct ImageFormat {
    pub resolution: [u32; 2],
    pub channel_datatype: ChannelDatatype
}


#[repr(C)]
#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
pub enum ChannelDatatype {
    U8 = 6,
    I8 = 7,
    U16 = 8,
    I16 = 9,
    U32 = 10,
    I32 = 11,
    U64 = 12,
    I64 = 13,
    F16 = 33,
    F32 = 34,
    F64 = 35,
}

/// log path for rerun, component
#[derive(Serialize, Deserialize, Debug, ZeroCopySend)]
#[repr(C)]
pub enum RerunMessage<const N: usize> {
    Points3D(FixedSizeByteString<N>, archetypes::Points3D<N>),
    Boxes3D(FixedSizeByteString<N>, archetypes::Boxes3D<N>),
    TextLog(FixedSizeByteString<N>, archetypes::TextLog<N>),
    Transform3D(FixedSizeByteString<N>, archetypes::Transform3D),
    Pinhole(FixedSizeByteString<N>, archetypes::Pinhole),
    DepthImage(FixedSizeByteString<N>, archetypes::DepthImage<N>),
    Arrows3D(FixedSizeByteString<N>, archetypes::Arrows3D<N>),
    ViewCoordinates(FixedSizeByteString<N>, archetypes::ViewCoordinates),
    Asset3D(FixedSizeByteString<N>, archetypes::Asset3D<N>),
}

impl<const N: usize> RerunMessage<N> {
    pub fn points_3d(log_path: &str, points: archetypes::Points3D<N>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::Points3D(FixedSizeByteString::from_bytes(log_path.as_bytes())?, points))
    }

    pub fn boxes_3d(log_path: &str, boxes: archetypes::Boxes3D<N>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::Boxes3D(FixedSizeByteString::from_bytes(log_path.as_bytes())?, boxes))
    }

    pub fn text_log(log_path: &str, text_log: archetypes::TextLog<N>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::TextLog(FixedSizeByteString::from_bytes(log_path.as_bytes())?, text_log))
    }

    pub fn transform_3d(log_path: &str, transform: archetypes::Transform3D) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::Transform3D(FixedSizeByteString::from_bytes(log_path.as_bytes())?, transform))
    }

    pub fn pinhole(log_path: &str, pinhole: archetypes::Pinhole) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::Pinhole(FixedSizeByteString::from_bytes(log_path.as_bytes())?, pinhole))
    }

    pub fn depth_image(log_path: &str, depth_image: archetypes::DepthImage<N>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::DepthImage(FixedSizeByteString::from_bytes(log_path.as_bytes())?, depth_image))
    }

    pub fn arrows_3d(log_path: &str, arrows: archetypes::Arrows3D<N>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::Arrows3D(FixedSizeByteString::from_bytes(log_path.as_bytes())?, arrows))
    }

    pub fn view_coordinates(log_path: &str, view_coords: archetypes::ViewCoordinates) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::ViewCoordinates(FixedSizeByteString::from_bytes(log_path.as_bytes())?, view_coords))
    }

    pub fn asset_3d(log_path: &str, asset: archetypes::Asset3D<N>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::Asset3D(FixedSizeByteString::from_bytes(log_path.as_bytes())?, asset))
    }
}

/// Error type for Recorder operations
#[derive(Debug)]
pub enum RecorderError {
    IpcError(String),
    PublishError(String),
    SendError(String),
    SerializationError(String),
    NotConnected,
}

impl std::fmt::Display for RecorderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecorderError::IpcError(e) => write!(f, "IPC error: {}", e),
            RecorderError::PublishError(e) => write!(f, "Publisher loan error: {}", e),
            RecorderError::SendError(e) => write!(f, "Publisher send error: {}", e),
            RecorderError::SerializationError(e) => write!(f, "Serialization error: {}", e),
            RecorderError::NotConnected => write!(f, "Recorder not connected"),
        }
    }
}

impl std::error::Error for RecorderError {}

type RecorderResult<T> = Result<T, RecorderError>;

/// A drop-in replacement for rerun's RecordingStream that sends messages over IPC
pub struct Recorder {
    publisher: Option<Publisher<RerunMessage<1024>>>,
}

impl Recorder {
    /// Create a new recorder that connects to the rerun-ipc service
    pub fn new() -> RecorderResult<Self> {
        let node = NodeBuilder::new().create::<ipc::Service>()
            .map_err(|e| RecorderError::SerializationError(format!("Failed to create node: {}", e)))?;

        let service = node.service_builder(&"rerun/messages".try_into().unwrap())
            .publish_subscribe::<RerunMessage<1024>>()
            .open_or_create()
            .map_err(|e| RecorderError::IpcError(format!("Failed to open service: {}", e)))?;

        let publisher = service.publisher_builder().create()
            .map_err(|e| RecorderError::SerializationError(format!("Failed to create publisher: {}", e)))?;

        Ok(Self {
            publisher: Some(publisher),
        })
    }

    /// Create a disabled recorder that does nothing (similar to rerun's disabled state)
    pub fn disabled() -> Self {
        Self {
            publisher: None,
        }
    }

    /// Send a message over IPC
    fn send_message(&self, message: RerunMessage<1024>) -> RecorderResult<()> {
        let publisher = self.publisher.as_ref().ok_or(RecorderError::NotConnected)?;
        
        let sample = publisher.loan_uninit()
            .map_err(|e| RecorderError::PublishError(format!("Failed to loan sample: {}", e)))?;
        let sample = sample.write_payload(message);
        sample.send()
            .map_err(|e| RecorderError::SendError(format!("Failed to send sample: {}", e)))?;
        
        Ok(())
    }

    /// Log any rerun-compatible archetype (non-static)
    pub fn log<T>(&self, entity_path: &str, archetype: &T) -> RecorderResult<()>
    where
        T: IntoRerunMessage,
    {
        let message = archetype.into_rerun_message(entity_path)?;
        self.send_message(message)
    }

    /// Log any rerun-compatible archetype (static)
    pub fn log_static<T>(&self, entity_path: &str, archetype: &T) -> RecorderResult<()>
    where
        T: IntoRerunMessage,
    {
        // For now, treat static the same as regular log
        self.log(entity_path, archetype)
    }
}

/// Trait for converting rerun-like types to our IPC message format
pub trait IntoRerunMessage {
    fn into_rerun_message(&self, entity_path: &str) -> RecorderResult<RerunMessage<1024>>;
}

// Conversion helpers for common rerun-like types
impl IntoRerunMessage for ViewCoordinatesLike {
    fn into_rerun_message(&self, entity_path: &str) -> RecorderResult<RerunMessage<1024>> {
        let view_coords = archetypes::ViewCoordinates::right_hand_y_up();
        RerunMessage::view_coordinates(entity_path, view_coords)
            .map_err(|e| RecorderError::SerializationError(e.to_string()))
    }
}

impl IntoRerunMessage for Arrows3DLike {
    fn into_rerun_message(&self, entity_path: &str) -> RecorderResult<RerunMessage<1024>> {
        let mut vectors = FixedSizeVec::new();
        let mut colors = FixedSizeVec::new();
        
        for vector in &self.vectors {
            vectors.push(Position3D { x: vector[0], y: vector[1], z: vector[2] })
                .map_err(|_| RecorderError::SerializationError("Too many vectors".to_string()))?;
        }
        
        for color in &self.colors {
            colors.push(Color { r: color[0], g: color[1], b: color[2], a: color[3] })
                .map_err(|_| RecorderError::SerializationError("Too many colors".to_string()))?;
        }
        
        let arrows = archetypes::Arrows3D::from_vectors(vectors).with_colors(colors);
        RerunMessage::arrows_3d(entity_path, arrows)
            .map_err(|e| RecorderError::SerializationError(e.to_string()))
    }
}

impl IntoRerunMessage for Transform3DLike {
    fn into_rerun_message(&self, entity_path: &str) -> RecorderResult<RerunMessage<1024>> {
        let transform = archetypes::Transform3D {
            position: Position3D { x: self.translation[0], y: self.translation[1], z: self.translation[2] },
            rotation: Quaternion { x: self.rotation[0], y: self.rotation[1], z: self.rotation[2], w: self.rotation[3] },
            scale: self.scale.unwrap_or(1.0),
        };
        RerunMessage::transform_3d(entity_path, transform)
            .map_err(|e| RecorderError::SerializationError(e.to_string()))
    }
}

impl IntoRerunMessage for Asset3DLike {
    fn into_rerun_message(&self, entity_path: &str) -> RecorderResult<RerunMessage<1024>> {
        let asset = archetypes::Asset3D::from_bytes(&self.data, &self.media_type)
            .map_err(|e| RecorderError::SerializationError(e.to_string()))?;
        RerunMessage::asset_3d(entity_path, asset)
            .map_err(|e| RecorderError::SerializationError(e.to_string()))
    }
}

impl IntoRerunMessage for TextLogLike {
    fn into_rerun_message(&self, entity_path: &str) -> RecorderResult<RerunMessage<1024>> {
        let level = match self.level.as_str() {
            "INFO" => Level::Info,
            "WARN" => Level::Warning,
            "ERROR" => Level::Error,
            _ => Level::Info,
        };
        
        let text_log = archetypes::TextLog {
            text: FixedSizeByteString::from_bytes(self.text.as_bytes())
                .map_err(|e| RecorderError::SerializationError(e.to_string()))?,
            level,
        };
        RerunMessage::text_log(entity_path, text_log)
            .map_err(|e| RecorderError::SerializationError(e.to_string()))
    }
}

// Helper structs to represent rerun-like data
pub struct ViewCoordinatesLike;

pub struct Arrows3DLike {
    pub vectors: Vec<[f32; 3]>,
    pub colors: Vec<[u8; 4]>,
}

impl Arrows3DLike {
    pub fn from_vectors(vectors: Vec<[f32; 3]>) -> Self {
        Self { vectors, colors: Vec::new() }
    }
    
    pub fn with_colors(mut self, colors: Vec<[u8; 4]>) -> Self {
        self.colors = colors;
        self
    }
}

pub struct Transform3DLike {
    pub translation: [f32; 3],
    pub rotation: [f32; 4], // xyzw quaternion
    pub scale: Option<f32>,
}

impl Transform3DLike {
    pub fn from_translation_rotation(translation: [f32; 3], rotation: [f32; 4]) -> Self {
        Self { translation, rotation, scale: None }
    }
    
    pub fn from_translation_rotation_scale(translation: [f32; 3], rotation: [f32; 4], scale: f32) -> Self {
        Self { translation, rotation, scale: Some(scale) }
    }
}

pub struct Asset3DLike {
    pub data: Vec<u8>,
    pub media_type: String,
}

impl Asset3DLike {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, media_type: "application/octet-stream".to_string() }
    }
    
    pub fn from_file_path(path: &str) -> Result<Self, std::io::Error> {
        let data = std::fs::read(path)?;
        let media_type = if path.ends_with(".stl") {
            "model/stl".to_string()
        } else if path.ends_with(".obj") {
            "model/obj".to_string()
        } else {
            "application/octet-stream".to_string()
        };
        Ok(Self { data, media_type })
    }
}

pub struct TextLogLike {
    pub text: String,
    pub level: String,
}

impl TextLogLike {
    pub fn new(text: String) -> Self {
        Self { text, level: "INFO".to_string() }
    }
    
    pub fn with_level(mut self, level: &str) -> Self {
        self.level = level.to_string();
        self
    }
}