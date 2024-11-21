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
    log::error,
    service::ServiceExt,
    shared::{DataHandle, UninitOwnedData},
    tokio::sync::{Mutex, OnceCell},
};
use urobotics_py::{PyRepl, PythonValue, PythonVenvBuilder};

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
pub struct CameraConnectionBuilder {
    pub identifier: CameraIdentifier,
    #[serde(default)]
    pub fps: u32,
    #[serde(default)]
    pub image_width: u32,
    #[serde(default)]
    pub image_height: u32,
    #[serde(default)]
    pub py_venv_builder: PythonVenvBuilder,
    #[serde(skip)]
    image_received: UninitOwnedData<DynamicImage>,
    #[serde(skip)]
    camera_info: Arc<OnceLock<CameraInfo>>,
}

pub struct PendingCameraInfo(Arc<OnceLock<CameraInfo>>);

impl PendingCameraInfo {
    pub fn try_get(&self) -> Option<&CameraInfo> {
        self.0.get()
    }
}

macro_rules! cam_impl {
    ($self: ident) => {{
        let repl = PY_REPL
            .get_or_init(|| async {
                $self
                    .py_venv_builder
                    .packages_to_install
                    .push("cv2_enumerate_cameras".to_string());
                $self
                    .py_venv_builder
                    .packages_to_install
                    .push("opencv-python".to_string());
                let mut repl = $self
                    .py_venv_builder
                    .build()
                    .await
                    .expect("Failed to build Python venv")
                    .repl()
                    .await
                    .expect("Failed to start Python REPL");
                repl.call("from cv2_enumerate_cameras import enumerate_cameras")
                    .await
                    .expect("Failed to import cv2_enumerate_cameras");
                Mutex::new(repl)
            })
            .await;

        let result = repl
            .lock()
            .await
            .call(CODE)
            .await
            .expect("Failed to enumerate cameras");
        let result = match result {
            PythonValue::String(s) => s,
            PythonValue::None => String::new(),
            _ => panic!("Unexpected result while enumerating cameras: {result:?}"),
        };

        let lines = result.lines();

        let index = match &$self.identifier {
            CameraIdentifier::Index(index) => CameraIndex::Index(*index),
            CameraIdentifier::Name(name) => 'index: {
                for line in lines {
                    let Some((index, camera_name, _)) = unformat!("{};{};{}", line) else {
                        panic!("Failed to parse line: {line}")
                    };
                    if camera_name == name {
                        break 'index CameraIndex::Index(
                            index.parse().expect("Failed to parse camera index"),
                        );
                    }
                }
                return Err(nokhwa::NokhwaError::OpenDeviceError(
                    name.to_string(),
                    "Camera not found".into(),
                ));
            }
            CameraIdentifier::Path(path) => 'index: {
                for line in lines {
                    let Some((index, _, camera_path)) = unformat!("{};{};{}", line) else {
                        panic!("Failed to parse line: {line}")
                    };
                    if camera_path == path.to_string_lossy() {
                        break 'index CameraIndex::Index(
                            index.parse().expect("Failed to parse camera index"),
                        );
                    }
                }
                return Err(nokhwa::NokhwaError::OpenDeviceError(
                    path.to_string_lossy().to_string(),
                    "Camera not found".into(),
                ));
            }
        };

        let requested = if $self.fps > 0 {
            if $self.image_width > 0 && $self.image_height > 0 {
                RequestedFormat::new::<RgbFormat>(RequestedFormatType::Exact(CameraFormat::new(
                    Resolution::new($self.image_width, $self.image_height),
                    FrameFormat::RAWRGB,
                    $self.fps,
                )))
            } else {
                RequestedFormat::new::<RgbFormat>(RequestedFormatType::HighestFrameRate($self.fps))
            }
        } else if $self.image_width > 0 && $self.image_height > 0 {
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::HighestResolution(
                Resolution::new($self.image_width, $self.image_height),
            ))
        } else {
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate)
        };

        (index, requested)
    }};
}

impl CameraConnectionBuilder {
    /// Creates a pending connection to the camera with the given index.
    pub fn new(identifier: CameraIdentifier) -> Self {
        Self {
            identifier,
            fps: 0,
            image_width: 0,
            image_height: 0,
            image_received: UninitOwnedData::default(),
            camera_info: Arc::default(),
            py_venv_builder: PythonVenvBuilder::default(),
        }
    }

    pub async fn get_camera_info(&mut self) -> PendingCameraInfo {
        PendingCameraInfo(self.camera_info.clone())
    }

    pub fn image_received_handle(&self) -> &DataHandle<DynamicImage> {
        self.image_received.get_data_handle()
    }

    pub async fn resolve(mut self) -> Result<PendingCameraConnection, nokhwa::NokhwaError> {
        let (camera_index, requested) = cam_impl!(self);

        Ok(PendingCameraConnection {
            camera_index,
            requested,
            image_received: self.image_received,
        })
    }
}

pub struct PendingCameraConnection {
    camera_index: CameraIndex,
    requested: RequestedFormat<'static>,
    image_received: UninitOwnedData<DynamicImage>,
}

impl PendingCameraConnection {
    pub fn spawn(self) -> Result<CameraInfo, nokhwa::NokhwaError> {
        let (info_tx, info_rx) = std::sync::mpsc::sync_channel(1);

        std::thread::spawn(move || {
            macro_rules! unwrap {
                ($result: expr) => {
                    match $result {
                        Ok(x) => x,
                        Err(e) => {
                            let _ = info_tx.send(Err(e));
                            return;
                        }
                    }
                };
            }
            let mut camera = unwrap!(nokhwa::Camera::new(self.camera_index, self.requested));
            let camera_info = CameraInfo {
                camera_name: camera.info().human_name(),
            };
            unwrap!(camera.open_stream());
            let _ = info_tx.send(Ok(camera_info));

            macro_rules! get_img {
                () => {
                    {
                        let frame = match camera.frame() {
                            Ok(x) => x,
                            Err(e) => {
                                error!(target: &camera.info().human_name(), "Failed to get frame: {:?}", e);
                                return;
                            }
                        };
                        let decoded = frame.decode_image::<RgbFormat>().unwrap();
                        DynamicImage::ImageRgb8(
                            ImageBuffer::from_raw(decoded.width(), decoded.height(), decoded.into_raw())
                                .unwrap(),
                        )
                    }
                }
            }

            let owned = self.image_received.init(get_img!());
            let mut unowned = owned.pessimistic_share();

            loop {
                let img = get_img!();
                unowned = unowned.replace(img).pessimistic_share();
            }
        });

        info_rx.recv().unwrap()
    }
}

#[cfg(feature = "standalone")]
impl urobotics_app::Runnable for CameraConnectionBuilder {
    fn run(mut self) {
        use urobotics_core::{task::Loggable, BlockOn};
        (async move {
                let (index, requested) = cam_impl!(self);

                let mut camera = nokhwa::Camera::new(
                    index,
                    requested,
                ).expect("Failed to open camera");

                let camera_info = CameraInfo {
                    camera_name: camera.info().human_name()
                };
                #[cfg(debug_assertions)]
                urobotics_core::log::warn!(target: "camera", "Release mode is recommended when using camera as an app");

                let mut dump = urobotics_video::VideoDataDump::new_display(camera_info.camera_name, camera.camera_format().width(), camera.camera_format().height(), true).expect("Failed to initialize video data dump");

                camera.open_stream().expect("Failed to open camera stream");
                loop {
                    let frame = camera.frame().expect("Failed to get frame");
                    // if context.is_runtime_exiting() {
                    //     break;
                    // }
                    let decoded = frame.decode_image::<RgbFormat>().unwrap();
                    let img = DynamicImage::ImageRgb8(ImageBuffer::from_raw(
                        decoded.width(),
                        decoded.height(),
                        decoded.into_raw(),
                    ).unwrap());

                    dump.write_frame(&img).await.expect("Failed to write frame to video data dump");
                }
                // For type inference
                #[allow(unreachable_code)]
                Ok(())
            }).block_on().log();
    }
}

/// Returns an iterator over all the cameras that were identified on this computer.
pub fn discover_all_cameras(
) -> Result<impl Iterator<Item = CameraConnectionBuilder>, nokhwa::NokhwaError> {
    Ok(query(nokhwa::utils::ApiBackend::Auto)?
        .into_iter()
        .filter_map(|info| {
            let CameraIndex::Index(n) = info.index() else {
                return None;
            };
            Some(CameraConnectionBuilder::new(CameraIdentifier::Index(*n)))
        }))
}

#[cfg(target_os = "windows")]
const CODE: &str = "for camera_info in enumerate_cameras(1400):\r\tprint(f'{camera_info.index};{camera_info.name};{camera_info.path}')";
#[cfg(target_os = "linux")]
const CODE: &str = "for camera_info in enumerate_cameras(200):\r\tprint(f'{camera_info.index};{camera_info.name};{camera_info.path}')";
#[cfg(target_os = "macos")]
const CODE: &str = "for camera_info in enumerate_cameras(1200):\r\tprint(f'{camera_info.index};{camera_info.name};{camera_info.path}')";

pub mod app {
    use urobotics_app::define_app;

    use crate::CameraConnectionBuilder;

    define_app!(pub Camera(CameraConnectionBuilder): "Displays a camera feed");
}