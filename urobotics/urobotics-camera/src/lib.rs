//! This crate provides a node that can connect to any generic
//! color camera. This crate is cross-platform.
//!
//! Do note that this crate should not be expected to connect
//! to RealSense cameras.

use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use image::{DynamicImage, ImageBuffer};
use nokhwa::{
    pixel_format::RgbFormat,
    query,
    utils::{
        CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
    },
};
use serde::Deserialize;
use unfmt::unformat;
use urobotics_core::{
    define_shared_callbacks,
    function::FunctionConfig,
    service::ServiceExt,
    tokio::{
        self,
        sync::{Mutex, OnceCell},
    },
};
use urobotics_py::{PyRepl, PythonValue, PythonVenvBuilder};
use urobotics_video::VideoDataDump;

define_shared_callbacks!(ImageCallbacks => FnMut(image: &Arc<DynamicImage>) + Send + Sync);

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum CameraIdentifier {
    Index(u32),
    Name(Cow<'static, str>),
    Path(PathBuf),
}

#[derive(Clone, Debug)]
pub struct CameraInfo {
    pub camera_name: String,
}

static PY_REPL: OnceCell<Mutex<PyRepl>> = OnceCell::const_new();

/// A pending connection to a camera.
///
/// The connection is not created until this `Node` is ran.
#[derive(Deserialize)]
pub struct CameraConnection {
    pub identifier: CameraIdentifier,
    #[serde(default)]
    pub fps: u32,
    #[serde(default)]
    pub image_width: u32,
    #[serde(default)]
    pub image_height: u32,
    #[serde(skip)]
    image_received: ImageCallbacks,
    #[serde(skip)]
    camera_info: Arc<OnceLock<CameraInfo>>,
    #[cfg(feature = "standalone")]
    #[serde(default)]
    pub standalone: bool,
}

pub struct PendingCameraInfo(Arc<OnceLock<CameraInfo>>);

impl PendingCameraInfo {
    pub fn try_get(&self) -> Option<&CameraInfo> {
        self.0.get()
    }
}

impl CameraConnection {
    /// Creates a pending connection to the camera with the given index.
    pub fn new(identifier: CameraIdentifier) -> Self {
        Self {
            identifier,
            fps: 0,
            image_width: 0,
            image_height: 0,
            image_received: ImageCallbacks::default(),
            camera_info: Arc::default(),
            standalone: false,
        }
    }

    pub async fn get_camera_info(&mut self) -> PendingCameraInfo {
        PendingCameraInfo(self.camera_info.clone())
    }

    /// Gets a reference to the `Signal` that represents received images.
    pub fn image_received_ref(
        &self,
    ) -> SharedCallbacksRef<dyn FnMut(&Arc<DynamicImage>) + Send + Sync> {
        self.image_received.get_ref()
    }
}

impl FunctionConfig for CameraConnection {
    type Output = Result<(), nokhwa::NokhwaError>;

    const PERSISTENT: bool = false;

    const NAME: &'static str = "camera";

    #[cfg(feature = "standalone")]
    fn standalone(mut self, value: bool) -> Self {
        self.standalone = value;
        self
    }

    fn run(self, context: &urobotics_core::runtime::RuntimeContext) -> Self::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let repl = PY_REPL.get_or_init(|| async {
                    let mut builder = PythonVenvBuilder::default();
                    builder.packages_to_install.push("cv2_enumerate_cameras".to_string());
                    let mut repl = builder.build().await.expect("Failed to build Python venv").repl().await.expect("Failed to start Python REPL");
                    repl.call("from cv2_enumerate_cameras import enumerate_cameras").await.expect("Failed to import cv2_enumerate_cameras");
                    Mutex::new(repl)
                }).await;

                #[cfg(target_os = "windows")]
                let code = "for camera_info in enumerate_cameras(1400):\r\tprint(f'{camera_info.index};{camera_info.name};{camera_info.path}')";
                #[cfg(target_os = "linux")]
                let code = "for camera_info in enumerate_cameras(200):\r\tprint(f'{camera_info.index};{camera_info.name};{camera_info.path}')";
                #[cfg(target_os = "macos")]
                let code = "for camera_info in enumerate_cameras(1200):\r\tprint(f'{camera_info.index};{camera_info.name};{camera_info.path}')";

                let result = repl.lock().await.call(code).await.expect("Failed to enumerate cameras");
                let result = match result {
                    PythonValue::String(s) => s,
                    PythonValue::None => String::new(),
                    _ => panic!("Unexpected result while enumerating cameras: {result:?}")
                };

                let lines = result.lines();

                let index = match &self.identifier {
                    CameraIdentifier::Index(index) => CameraIndex::Index(*index),
                    CameraIdentifier::Name(name) => 'index: {
                        for line in lines {
                            let Some((index, camera_name, _)) = unformat!("{};{};{}", line) else { panic!("Failed to parse line: {line}") };
                            if camera_name == name {
                                break 'index CameraIndex::Index(index.parse().expect("Failed to parse camera index"));
                            }
                        }
                        return Err(nokhwa::NokhwaError::OpenDeviceError(name.to_string(), "Camera not found".into()))
                    },
                    CameraIdentifier::Path(path) => 'index: {
                        for line in lines {
                            let Some((index, _, camera_path)) = unformat!("{};{};{}", line) else { panic!("Failed to parse line: {line}") };
                            if camera_path == path.to_string_lossy() {
                                break 'index CameraIndex::Index(index.parse().expect("Failed to parse camera index"));
                            }
                        }
                        return Err(nokhwa::NokhwaError::OpenDeviceError(path.to_string_lossy().to_string(), "Camera not found".into()))
                    },
                };

                let requested = if self.fps > 0 {
                    if self.image_width > 0 && self.image_height > 0 {
                        RequestedFormat::new::<RgbFormat>(RequestedFormatType::Exact(CameraFormat::new(
                            Resolution::new(self.image_width, self.image_height),
                            FrameFormat::RAWRGB,
                            self.fps,
                        )))
                    } else {
                        RequestedFormat::new::<RgbFormat>(RequestedFormatType::HighestFrameRate(self.fps))
                    }
                } else if self.image_width > 0 && self.image_height > 0 {
                    RequestedFormat::new::<RgbFormat>(RequestedFormatType::HighestResolution(
                        Resolution::new(self.image_width, self.image_height),
                    ))
                } else {
                    RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate)
                };

                let mut camera = nokhwa::Camera::new(
                    index,
                    requested,
                )?;

                let camera_info = CameraInfo {
                    camera_name: camera.info().human_name()
                };
                #[cfg(feature = "standalone")]
                let mut dump = if self.standalone {
                    Some(VideoDataDump::new_display(camera_info.camera_name, camera.camera_format().width(), camera.camera_format().height(), true).expect("Failed to initialize video data dump"))
                } else {
                    None
                };

                camera.open_stream()?;
                loop {
                    let frame = camera.frame()?;
                    if context.is_runtime_exiting() {
                        break Ok(());
                    }
                    let decoded = frame.decode_image::<RgbFormat>().unwrap();
                    let img = DynamicImage::ImageRgb8(ImageBuffer::from_raw(
                        decoded.width(),
                        decoded.height(),
                        decoded.into_raw(),
                    ).unwrap());
                    let img = Arc::new(img);
                    self.image_received.call(&img);

                    #[cfg(feature = "standalone")]
                    if self.standalone {
                        dump.as_mut().unwrap().write_frame(&img).await.expect("Failed to write frame to video data dump");
                    }
                }
            })
    }
}

/// Returns an iterator over all the cameras that were identified on this computer.
pub fn discover_all_cameras() -> Result<impl Iterator<Item = CameraConnection>, nokhwa::NokhwaError>
{
    Ok(query(nokhwa::utils::ApiBackend::Auto)?
        .into_iter()
        .filter_map(|info| {
            let CameraIndex::Index(n) = info.index() else {
                return None;
            };
            Some(CameraConnection::new(CameraIdentifier::Index(*n)))
        }))
}
