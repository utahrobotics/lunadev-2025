//! This crate provides a node that can connect to any generic
//! color camera. This crate is cross-platform.
//!
//! Do note that this crate should not be expected to connect
//! to RealSense cameras.

use std::sync::Arc;

use image::{imageops::FilterType, DynamicImage};
use nokhwa::{
    pixel_format::RgbFormat,
    query,
    utils::{
        CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
    },
};
use unros::{
    anyhow::{self, Context},
    async_trait, asyncify_run, log,
    pubsub::{Publisher, PublisherRef},
    setup_logging, DropCheck, Node, NodeIntrinsics, RuntimeContext,
};

/// A pending connection to a camera.
///
/// The connection is not created until this `Node` is ran.
pub struct Camera {
    pub fps: u32,
    pub camera_index: u32,
    pub res_x: u32,
    pub res_y: u32,
    camera_name: String,
    image_received: Publisher<Arc<DynamicImage>>,
    intrinsics: NodeIntrinsics<Self>,
}

impl Camera {
    /// Creates a pending connection to the camera with the given index.
    pub fn new(camera_index: u32) -> anyhow::Result<Self> {
        let tmp_camera = nokhwa::Camera::new(
            CameraIndex::Index(camera_index),
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate),
        )
        .context("Failed to initialize camera")?;
        let camera_name = tmp_camera.info().human_name();
        log::info!("Pinged {} with index {}", camera_name, camera_index);
        Ok(Self {
            fps: 0,
            res_x: 0,
            res_y: 0,
            camera_index,
            camera_name,
            image_received: Default::default(),
            intrinsics: Default::default(),
        })
    }

    pub fn get_camera_name(&self) -> &str {
        &self.camera_name
    }

    /// Gets a reference to the `Signal` that represents received images.
    pub fn image_received_pub(&self) -> PublisherRef<Arc<DynamicImage>> {
        self.image_received.get_ref()
    }
}

#[async_trait]
impl Node for Camera {
    const DEFAULT_NAME: &'static str = "camera";

    fn get_intrinsics(&mut self) -> &mut NodeIntrinsics<Self> {
        &mut self.intrinsics
    }

    async fn run(mut self, context: RuntimeContext) -> anyhow::Result<()> {
        setup_logging!(context);

        let index = CameraIndex::Index(self.camera_index);

        let requested = if self.fps > 0 {
            if self.res_x > 0 && self.res_y > 0 {
                RequestedFormat::new::<RgbFormat>(RequestedFormatType::Exact(CameraFormat::new(
                    Resolution::new(self.res_x, self.res_y),
                    FrameFormat::RAWRGB,
                    self.fps,
                )))
            } else {
                RequestedFormat::new::<RgbFormat>(RequestedFormatType::HighestFrameRate(self.fps))
            }
        } else if self.res_x > 0 && self.res_y > 0 {
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::HighestResolution(
                Resolution::new(self.res_x, self.res_y),
            ))
        } else {
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate)
        };

        let res_x = self.res_x;
        let res_y = self.res_y;

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