//! This crate provides a node that can connect to RealSense cameras and interpret
//! depth and color images.
#![feature(never_type, once_cell_try)]

use std::{
    ffi::OsString, ops::Deref, path::{Path, PathBuf}, sync::{Arc, Mutex, OnceLock}, time::{Duration, Instant}
};

use image::{DynamicImage, ImageBuffer, Luma, Rgb};
use realsense_rust::{
    config::{Config, ConfigurationError},
    context::{Context, ContextConstructionError},
    device::Device,
    frame::{ColorFrame, DepthFrame, PixelKind},
    kind::{Rs2CameraInfo, Rs2Format, Rs2ProductLine, Rs2StreamKind},
    pipeline::{ActivePipeline, FrameWaitError, InactivePipeline},
};
pub use realsense_rust;
use urobotics_core::{define_callbacks, fn_alias, log::{error, warn}, task::SyncTask};

define_callbacks!(pub ColorCallbacks => Fn(color_img: &Arc<DynamicImage>) + Send + Sync);
define_callbacks!(pub DepthCallbacks => Fn(depth_img: &Arc<DynamicImage>) + Send + Sync);
fn_alias! {
    pub type ColorCallbacksRef = CallbacksRef(&Arc<DynamicImage>) + Send + Sync
}
fn_alias! {
    pub type DepthCallbacksRef = CallbacksRef(&Arc<DynamicImage>) + Send + Sync
}

static CONTEXT: OnceLock<Mutex<Context>> = OnceLock::new();

enum CameraSource {
    Path(PathBuf),
    Device(Device)
}

fn get_context() -> Result<&'static Mutex<Context>, ContextConstructionError> {
    CONTEXT.get_or_try_init(|| Context::new().map(Mutex::new))
}


pub struct RealSenseCameraBuilder {
    source: CameraSource,
    color_img_callbacks: ColorCallbacks,
    depth_img_callbacks: DepthCallbacks,
    pub color_image_width: u32,
    pub color_image_height: u32,
    pub color_fps: usize,
    pub depth_image_width: u32,
    pub depth_image_height: u32,
    pub depth_fps: usize,
}

impl RealSenseCameraBuilder {
    /// Attempts to connect to the camera at the given `dev` path.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            source: CameraSource::Path(path.as_ref().to_path_buf()),
            color_img_callbacks: ColorCallbacks::default(),
            depth_img_callbacks: DepthCallbacks::default(),
            color_image_width: 0,
            color_image_height: 0,
            color_fps: 0,
            depth_image_width: 0,
            depth_image_height: 0,
            depth_fps: 0,
        }
    }

    pub fn color_callbacks_ref(&self) -> ColorCallbacksRef {
        self.color_img_callbacks.get_ref()
    }

    pub fn depth_callbacks_ref(&self) -> DepthCallbacksRef {
        self.depth_img_callbacks.get_ref()
    }

    pub fn build(self) -> Result<RealSenseCamera, RealSenseBuildError> {
        let mut context = get_context()?.lock().unwrap();
        let device = match self.source {
            CameraSource::Path(path) => {
                context.add_device(path).map_err(|e| RealSenseBuildError::DeviceError(e.into()))?
            }
            CameraSource::Device(device) => device,
        };
        let pipeline = InactivePipeline::try_from(context.deref()).map_err(|e| RealSenseBuildError::PipelineError(e.into()))?;
        let mut config = Config::new();

        let usb_cstr = device.info(Rs2CameraInfo::UsbTypeDescriptor).unwrap();
        let usb_val: f32 = usb_cstr.to_str().unwrap().parse().unwrap();
        if usb_val >= 3.0 {
            config
                .enable_device_from_serial(device.info(Rs2CameraInfo::SerialNumber).unwrap())?
                .disable_all_streams()?
                .enable_stream(Rs2StreamKind::Depth, None, self.depth_image_width as usize, self.depth_image_width as usize, Rs2Format::Z16, self.depth_fps)?
                .enable_stream(
                    Rs2StreamKind::Color,
                    None,
                    self.color_image_width as usize,
                    self.color_image_height as usize,
                    Rs2Format::Rgb8,
                    self.color_fps,
                )?;
        } else {
            warn!("This Realsense camera is not attached to a USB 3.0 port");
            config
                .enable_device_from_serial(device.info(Rs2CameraInfo::SerialNumber).unwrap())?
                .disable_all_streams()?
                .enable_stream(Rs2StreamKind::Depth, None, self.depth_image_width as usize, self.depth_image_width as usize, Rs2Format::Z16, self.depth_fps)?;
        }

        let pipeline = pipeline.start(Some(config)).map_err(|e| RealSenseBuildError::PipelineError(e.into()))?;
        Ok(
            RealSenseCamera {
                color_img_callbacks: self.color_img_callbacks,
                depth_img_callbacks: self.depth_img_callbacks,
                pipeline,
            }
        )
    }
}


pub enum RealSenseBuildError {
    ConfigurationError(ConfigurationError),
    ContextConstructionError(ContextConstructionError),
    PipelineError(Box<dyn std::error::Error + Send + Sync>),
    DeviceError(Box<dyn std::error::Error + Send + Sync>)
}

impl From<ConfigurationError> for RealSenseBuildError {
    fn from(e: ConfigurationError) -> Self {
        RealSenseBuildError::ConfigurationError(e)
    }
}

impl From<ContextConstructionError> for RealSenseBuildError {
    fn from(e: ContextConstructionError) -> Self {
        RealSenseBuildError::ContextConstructionError(e)
    }
}


pub struct RealSenseCamera {
    color_img_callbacks: ColorCallbacks,
    depth_img_callbacks: DepthCallbacks,
    pipeline: ActivePipeline,
}

impl RealSenseCamera {
    pub fn poll(&mut self, max_duration: Option<Duration>) -> Result<(), FrameWaitError> {
        let frames = self.pipeline.wait(max_duration)?;

        for frame in frames.frames_of_type::<ColorFrame>() {
            let Some(img) = ImageBuffer::<Rgb<u8>, _>::from_raw(
                frame.width() as u32,
                frame.height() as u32,
                frame
                    .iter()
                    .flat_map(|px| {
                        let PixelKind::Bgr8 { r, g, b } = px else {
                            unreachable!()
                        };
                        [*r, *g, *b]
                    })
                    .collect(),
            ) else {
                error!("Failed to copy realsense color image");
                continue;
            };
            let img = Arc::new(DynamicImage::from(img));
            self.color_img_callbacks.call(&img);
        }

        for frame in frames.frames_of_type::<DepthFrame>() {
            let Some(img) = ImageBuffer::<Luma<u16>, _>::from_raw(
                frame.width() as u32,
                frame.height() as u32,
                frame
                    .iter()
                    .flat_map(|px| {
                        let PixelKind::Z16 { depth } = px else {
                            unreachable!("Unexpected pixel kind: {px:?}")
                        };
                        [*depth]
                    })
                    .collect(),
            ) else {
                error!("Failed to copy realsense depth image");
                continue;
            };
            let img = Arc::new(DynamicImage::from(img));
            self.depth_img_callbacks.call(&img);
        }

        Ok(())
    }
    
    pub fn poll_until(&mut self, deadline: Instant) -> Result<(), FrameWaitError> {
        let now = Instant::now();
        loop {
            if now >= deadline {
                break Ok(());
            }
            self.poll(Some(deadline - now))?;
        }
    }

    pub fn get_path(&self) -> PathBuf {
        let path = self
            .pipeline
            .profile()
            .device()
            .info(Rs2CameraInfo::PhysicalPort)
            .expect("Failed to query camera port")
            .to_bytes();
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Path::new(OsStr::from_bytes(path)).to_owned()
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            Path::new(&OsString::from_wide(bytemuck::cast_slice(path))).to_owned()
        }
    }

    pub fn get_name(&self) -> OsString {
        let path = self
            .pipeline
            .profile()
            .device()
            .info(Rs2CameraInfo::Name)
            .expect("Failed to query camera name")
            .to_bytes();
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            OsStr::from_bytes(path).to_owned()
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(bytemuck::cast_slice(path))
        }
    }
}

impl SyncTask for RealSenseCamera {
    type Output = Result<!, FrameWaitError>;

    fn run(mut self) -> Self::Output {
        loop {
            self.poll(None)?;
        }
    }
}

/// Returns an iterator over all the RealSense cameras that were identified.
pub fn discover_all_realsense(product_mask: impl IntoIterator<Item=Rs2ProductLine>) -> Result<impl Iterator<Item = RealSenseCameraBuilder>, RealSenseBuildError> {
    let context = get_context()?.lock().unwrap();
    let devices = context.query_devices(product_mask.into_iter().collect());

    Ok(devices.into_iter().map(move |device| RealSenseCameraBuilder {
        source: CameraSource::Device(device),
        color_img_callbacks: ColorCallbacks::default(),
        depth_img_callbacks: DepthCallbacks::default(),
        color_image_width: 0,
        color_image_height: 0,
        color_fps: 0,
        depth_image_width: 0,
        depth_image_height: 0,
        depth_fps: 0,
    }))
}
