pub mod archetypes;
use ipc::IPCSender;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::time::Instant;
use crossbeam::atomic::AtomicCell;
use crossbeam::queue::ArrayQueue;
use std::thread::{self, JoinHandle};
use std::sync::atomic::{AtomicBool, Ordering};

// Re-export public types from archetypes
pub use archetypes::{
    Points3D, Boxes3D, TextLog, Transform3D, Pinhole, 
    DepthImage, Arrows3D, ViewCoordinates, Asset3D
};

pub static RECORDER_SERVICE_PATH:&'static str = "rerun_ipc";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RerunViz {
    /// Enable IPC-based rerun visualization with specified level
    Enabled(RerunLevel),
    /// Disabled - no visualization
    Disabled,
}

impl Default for RerunViz {
    fn default() -> Self {
        Self::Disabled
    }
}

#[derive(Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
pub enum RerunLevel {
    /// Only logs robots isometry, expanded obstacle map, and april tags.
    #[default]
    Minimal,
    /// Logs everything including height maps and depth camera point cloud.
    All,
}

impl RerunLevel {
    /// returns true if the log level is All
    pub fn is_all(&self) -> bool {
        *self == RerunLevel::All
    }
}



#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
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

impl Color {
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

impl From<[u8; 3]> for Color {
    fn from([r, g, b]: [u8; 3]) -> Self {
        Self::new(r, g, b, 255)
    }
}

impl From<[u8; 4]> for Color {
    fn from([r, g, b, a]: [u8; 4]) -> Self {
        Self::new(r, g, b, a)
    }
}

fn default_color() -> u8 {
    0
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Position3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Position3D {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

impl From<(f32, f32, f32)> for Position3D {
    fn from((x, y, z): (f32, f32, f32)) -> Self {
        Self::new(x, y, z)
    }
}

impl From<[f32; 3]> for Position3D {
    fn from([x, y, z]: [f32; 3]) -> Self {
        Self::new(x, y, z)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Quaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quaternion {
    pub fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }
    
    pub fn from_xyzw(values: [f32; 4]) -> Self {
        Self::new(values[0], values[1], values[2], values[3])
    }
}

impl From<[f32; 4]> for Quaternion {
    fn from([x, y, z, w]: [f32; 4]) -> Self {
        Self::new(x, y, z, w)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct ImageFormat {
    pub resolution: [u32; 2],
    pub channel_datatype: ChannelDatatype
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
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
#[derive(Serialize, Deserialize, Debug)]
pub enum RerunMessage {
    Points3D(String, Points3D),
    Boxes3D(String, Boxes3D),
    TextLog(String, TextLog),
    Transform3D(String, Transform3D),
    Pinhole(String, Pinhole),
    DepthImage(String, DepthImage),
    Arrows3D(String, Arrows3D),
    ViewCoordinates(String, ViewCoordinates),
    Asset3D(String, Asset3D),
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

pub type RecordingStreamResult<T> = Result<T, RecorderError>;
type RecorderResult<T> = Result<T, RecorderError>;

pub trait IntoRerunMessage {
    fn into_rerun_message(self, entity_path: &str) -> RerunMessage;
}

pub struct Vec3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3D {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

impl From<[f32; 3]> for Vec3D {
    fn from(arr: [f32; 3]) -> Self {
        Self::new(arr[0], arr[1], arr[2])
    }
}

// Legacy compatibility types (keeping old API)
pub use ViewCoordinates as ViewCoordinatesLike;
pub use Arrows3D as Arrows3DLike;
pub use Transform3D as Transform3DLike;
pub use Asset3D as Asset3DLike;
pub use TextLog as TextLogLike;

/// A thread-safe queued recorder that uses an ArrayQueue for message passing
pub struct QueuedRecorder {
    /// Queue for messages to be sent
    message_queue: Arc<ArrayQueue<(RerunMessage, bool)>>,
    /// Current logging level
    level: RerunLevel,
    /// Whether recording is enabled
    enabled: bool,
    /// Throttle timer for obstacle map logging
    obstacle_map_throttle: Arc<AtomicCell<Instant>>,
    /// Flag to signal the worker thread to stop
    stop_flag: Arc<AtomicBool>,
    /// Handle to the worker thread
    worker_handle: Option<JoinHandle<()>>,
}

impl QueuedRecorder {
    /// Create a new queued recorder with the specified configuration
    pub fn new_with_config(rerun_viz: RerunViz) -> RecorderResult<Self> {
        let (level, enabled) = match rerun_viz {
            RerunViz::Enabled(level) => (level, true),
            RerunViz::Disabled => (RerunLevel::Minimal, false),
        };

        // Create a large queue - adjust size as needed
        const QUEUE_SIZE: usize = 10000;
        let message_queue = Arc::new(ArrayQueue::new(QUEUE_SIZE));
        let stop_flag = Arc::new(AtomicBool::new(false));
        
        let worker_handle = if enabled {
            let queue_clone = message_queue.clone();
            let stop_clone = stop_flag.clone();
            
            Some(thread::spawn(move || {
                Self::worker_thread(queue_clone, stop_clone);
            }))
        } else {
            None
        };

        let recorder = Self {
            message_queue,
            level,
            enabled,
            obstacle_map_throttle: Arc::new(AtomicCell::new(Instant::now())),
            stop_flag,
            worker_handle,
        };

        // Set up the coordinate system and basic structure if enabled
        if enabled {
            recorder.setup_initial_scene()?;
        }

        Ok(recorder)
    }
    
    /// Worker thread that processes messages from the queue
    fn worker_thread(queue: Arc<ArrayQueue<(RerunMessage, bool)>>, stop_flag: Arc<AtomicBool>) {
        println!("🔧 DEBUG: Worker thread started");
        // Create the IPC sender within the worker thread
        let sender = match IPCSender::new("rerun/messages") {
            Ok(sender) => {
                println!("🔧 DEBUG: IPC sender created successfully");
                sender
            }
            Err(e) => {
                eprintln!("Failed to create IPC sender in worker thread: {}", e);
                println!("🔧 DEBUG: Failed to create IPC sender: {}", e);
                return;
            }
        };

        println!("🔧 DEBUG: Waiting for receiver to be ready...");
        sender.wait_until_ready().expect("Failed to wait for receiver to be ready");
        println!("🔧 DEBUG: Receiver is ready!");
        
        let mut message_count = 0;
        while !stop_flag.load(Ordering::Relaxed) {
            // Try to pop a message from the queue
            if let Some((message, is_static)) = queue.pop() {
                message_count += 1;
                let msg_type = match &message {
                    RerunMessage::Points3D(path, _) => format!("Points3D to {}", path),
                    RerunMessage::Boxes3D(path, _) => format!("Boxes3D to {}", path),
                    RerunMessage::TextLog(path, _) => format!("TextLog to {}", path),
                    RerunMessage::Transform3D(path, _) => format!("Transform3D to {}", path),
                    RerunMessage::Pinhole(path, _) => format!("Pinhole to {}", path),
                    RerunMessage::DepthImage(path, _) => format!("DepthImage to {}", path),
                    RerunMessage::Arrows3D(path, _) => format!("Arrows3D to {}", path),
                    RerunMessage::ViewCoordinates(path, _) => format!("ViewCoordinates to {}", path),
                    RerunMessage::Asset3D(path, _) => format!("Asset3D to {}", path),
                };
                
                if let Err(e) = sender.send(&(message, is_static)) {
                    eprintln!("Failed to send message via IPC: {}", e);
                    println!("🔧 DEBUG: Failed to send message via IPC: {}", e);
                } else {
                    if !msg_type.contains("Transform3D") {
                        println!("🔧 DEBUG: Successfully sent message #{} via IPC", message_count);
                    }
                }
            } else {
                // No messages available, sleep briefly to avoid busy waiting
                thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        println!("🔧 DEBUG: Worker thread stopping, total messages processed: {}", message_count);
    }

    /// Create a new enabled recorder with minimal logging
    pub fn new() -> RecorderResult<Self> {
        Self::new_with_config(RerunViz::Enabled(RerunLevel::Minimal))
    }

    /// Create a disabled recorder that does nothing
    pub fn disabled() -> Self {
        Self {
            message_queue: Arc::new(ArrayQueue::new(1)), // Minimal queue for disabled recorder
            level: RerunLevel::Minimal,
            enabled: false,
            obstacle_map_throttle: Arc::new(AtomicCell::new(Instant::now())),
            stop_flag: Arc::new(AtomicBool::new(true)),
            worker_handle: None,
        }
    }

    /// Get the current log level if recording is enabled
    pub fn get_log_level(&self) -> Option<RerunLevel> {
        if self.enabled {
            Some(self.level.clone())
        } else {
            None
        }
    }

    /// Check if recording is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the throttle timer for obstacle map logging
    pub fn get_obstacle_map_throttle(&self) -> &AtomicCell<Instant> {
        &self.obstacle_map_throttle
    }

    /// Set up the initial scene with coordinate system and robot mesh
    fn setup_initial_scene(&self) -> RecorderResult<()> {
        std::thread::sleep(std::time::Duration::from_secs(5));
        // Set up the coordinate system and basic structure
        self.log_static("/", ViewCoordinates::right_hand_y_up())?;
        self.log_static(
            "/robot/structure/xyz",
            archetypes::Arrows3D::from_vectors([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
                .with_colors([[255, 0, 0], [0, 255, 0], [0, 0, 255]]),
        )?;

        // Load robot mesh synchronously
        if let Err(e) = self.setup_robot_mesh() {
            eprintln!("Failed to setup robot mesh: {e}");
        }

        Ok(())
    }

    /// Setup the robot mesh
    fn setup_robot_mesh(&self) -> RecorderResult<()> {
        // Load robot mesh from file
        let robot_mesh_path = std::env::var("LUNABOT_MESH_PATH")
            .unwrap_or_else(|_| "3d-models/simplify_lunabot.stl".to_string());
        
        if let Ok(asset) = archetypes::Asset3D::from_bytes(
            &std::fs::read(&robot_mesh_path).unwrap_or_default(),
            "model/stl"
        ) {
            self.log_static("/robot/structure/mesh", asset)?;
        } else {
            eprintln!("Failed to load robot mesh from file: {}", robot_mesh_path);
        }

        // Add transform for robot pose
        let translation = Position3D::new(0.0, 0.0, 0.0);
        let rotation = Quaternion::new(0.0, 0.0, 0.0, 1.0);
        self.log(
            "/robot/structure/mesh",
            archetypes::Transform3D::from_translation_rotation(translation, rotation),
        )?;

        Ok(())
    }

    /// Send a message to the queue
    fn send_message(&self, message: RerunMessage, is_static: bool) -> RecorderResult<()> {
        if !self.enabled {
            println!("🔧 DEBUG: send_message called but recorder is disabled");
            return Ok(());
        }
        
        let msg_type = match &message {
            RerunMessage::Points3D(path, _) => format!("Points3D to {}", path),
            RerunMessage::Boxes3D(path, _) => format!("Boxes3D to {}", path),
            RerunMessage::TextLog(path, _) => format!("TextLog to {}", path),
            RerunMessage::Transform3D(path, _) => format!("Transform3D to {}", path),
            RerunMessage::Pinhole(path, _) => format!("Pinhole to {}", path),
            RerunMessage::DepthImage(path, _) => format!("DepthImage to {}", path),
            RerunMessage::Arrows3D(path, _) => format!("Arrows3D to {}", path),
            RerunMessage::ViewCoordinates(path, _) => format!("ViewCoordinates to {}", path),
            RerunMessage::Asset3D(path, _) => format!("Asset3D to {}", path),
        };
        
        // Try to push to queue, drop message if queue is full
        if self.message_queue.push((message, is_static)).is_err() {
            eprintln!("Warning: Message queue is full, dropping message");
            println!("🔧 DEBUG: Message queue is full, dropping message: {}", msg_type);
        } else {
            if !msg_type.contains("Transform3D") {
                println!("🔧 DEBUG: Successfully queued message: {}", msg_type);
            }
        }
        
        Ok(())
    }

    /// Log an archetype to the specified entity path
    pub fn log<T>(&self, entity_path: &str, archetype: T) -> RecorderResult<()>
    where
        T: IntoRerunMessage,
    {
        if !self.enabled {
            println!("🔧 DEBUG: log called but recorder is disabled for path: {}", entity_path);
            return Ok(());
        }
        let message = archetype.into_rerun_message(entity_path);
        self.send_message(message, false)
    }

    /// Log a static archetype to the specified entity path
    pub fn log_static<T>(&self, entity_path: &str, archetype: T) -> RecorderResult<()>
    where
        T: IntoRerunMessage,
    {
        if !self.enabled {
            println!("🔧 DEBUG: log_static called but recorder is disabled for path: {}", entity_path);
            return Ok(());
        }
        println!("🔧 DEBUG: log_static called for entity_path: {}", entity_path);
        let message = archetype.into_rerun_message(entity_path);
        println!("🔧 DEBUG: Message created for log_static, calling send_message");
        self.send_message(message, true)
    }
}

impl Drop for QueuedRecorder {
    fn drop(&mut self) {
        // Signal the worker thread to stop
        self.stop_flag.store(true, Ordering::Relaxed);
        
        // Wait for the worker thread to finish
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

// Make QueuedRecorder Send + Sync safe since we're using atomic operations and thread-safe queue
unsafe impl Send for QueuedRecorder {}
unsafe impl Sync for QueuedRecorder {}

/// Initialize the IPC-based rerun system and return a queued recorder (main function)
pub fn init_rerun(rerun_viz: RerunViz) -> RecorderResult<QueuedRecorder> {
    QueuedRecorder::new_with_config(rerun_viz)
}
