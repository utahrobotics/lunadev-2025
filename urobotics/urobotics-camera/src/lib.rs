//! This crate provides a node that can connect to any generic
//! color camera. This crate is cross-platform.
//!
//! Do note that this crate should not be expected to connect
//! to RealSense cameras.

use std::{borrow::Cow, sync::Arc};

use image::{imageops::FilterType, DynamicImage};
use nokhwa::{
    pixel_format::RgbFormat,
    query,
    utils::{
        CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
    },
};
use serde::Deserialize;
use urobotics_core::define_shared_callbacks;

define_shared_callbacks!(ImageCallbacks => FnMut(image: &Arc<DynamicImage>) + Send + Sync);

#[derive(Deserialize)]
pub enum CameraIdentifier {
    Index(u32),
    Name(Cow<'static, str>),
    Path(Cow<'static, str>),
}

/// A pending connection to a camera.
///
/// The connection is not created until this `Node` is ran.
#[derive(Deserialize)]
pub struct Camera {
    pub identifier: CameraIdentifier,
    #[serde(default)]
    pub fps: u32,
    #[serde(default)]
    pub image_width: u32,
    #[serde(default)]
    pub image_height: u32,
    #[serde(skip)]
    image_received: ImageCallbacks,
}

impl Camera {
    /// Creates a pending connection to the camera with the given index.
    pub fn new(identifier: CameraIdentifier) -> Self {
        Self {
            identifier,
            fps: 0,
            image_width: 0,
            image_height: 0,
            image_received: ImageCallbacks::default(),
        }
    }

    /// Gets a reference to the `Signal` that represents received images.
    pub fn image_received_ref(
        &self,
    ) -> SharedCallbacksRef<dyn FnMut(&Arc<DynamicImage>) + Send + Sync> {
        self.image_received.get_ref()
    }
}

impl Node for Camera {
    const DEFAULT_NAME: &'static str = "camera";

    fn get_intrinsics(&mut self) -> &mut NodeIntrinsics<Self> {
        &mut self.intrinsics
    }

    async fn run(mut self, context: RuntimeContext) -> anyhow::Result<()> {
        setup_logging!(context);

        let index = CameraIndex::Index(self.camera_index);

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

        let res_x = self.image_width;
        let res_y = self.image_height;

        let drop_check = DropCheck::default();
        let drop_obs = drop_check.get_observing();

        asyncify_run(move || {
            let mut camera =
                nokhwa::Camera::new(index, requested).context("Failed to initialize camera")?;
            camera.open_stream()?;
            loop {
                let frame = camera.frame()?;
                if drop_obs.has_dropped() {
                    break Ok(());
                }
                let decoded = frame.decode_image::<RgbFormat>().unwrap();
                let mut img = DynamicImage::from(decoded);
                if res_x != 0 && res_y != 0 {
                    img = img.resize(res_x, res_y, FilterType::CatmullRom);
                }
                self.image_received.set(Arc::new(img));
            }
        })
        .await
    }
}

/// Returns an iterator over all the cameras that were identified on this computer.
pub fn discover_all_cameras() -> anyhow::Result<impl Iterator<Item = Camera>> {
    Ok(query(nokhwa::utils::ApiBackend::Auto)?
        .into_iter()
        .filter_map(|info| {
            let CameraIndex::Index(n) = info.index() else {
                return None;
            };
            let Ok(cam) = Camera::new(*n) else {
                return None;
            };
            Some(cam)
        }))
}
