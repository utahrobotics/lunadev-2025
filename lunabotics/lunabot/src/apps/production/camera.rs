use anyhow::Context;
use fxhash::FxHashMap;
use udev::Udev;
use urobotics::{
    log::warn,
    shared::{OwnedData, SharedData},
};
use urobotics_apriltag::{
    image::{self, ImageBuffer, Luma},
    AprilTagDetector,
};
use v4l::{buffer::Type, format, io::traits::CaptureStream, prelude::MmapStream, video::Capture};

use crate::localization::LocalizerRef;

pub struct CameraInfo {
    pub k_node: k::Node<f64>,
    pub focal_length_px: f64,
}

pub fn enumerate_cameras(
    localizer_ref: LocalizerRef,
    serial_to_chain: impl IntoIterator<Item = (String, CameraInfo)>,
) -> anyhow::Result<()> {
    let mut serial_to_chain: FxHashMap<String, Option<_>> = serial_to_chain
        .into_iter()
        .map(|(serial, chain)| (serial, Some(chain)))
        .collect();
    {
        let udev = Udev::new()?;
        for node in v4l::context::enum_devices() {
            let udev_device =
                match udev::Device::from_syspath_with_context(udev.clone(), node.path()) {
                    Ok(x) => x,
                    Err(e) => {
                        warn!(
                            "Failed to get udev device for camera {:?}: {e}",
                            node.path()
                        );
                        continue;
                    }
                };
            let Some(serial_num) = udev_device.attribute_value("serial") else {
                warn!("No serial number for camera {:?}", node.path());
                continue;
            };
            let Some(serial_num) = serial_num.to_str() else {
                warn!("Failed to parse serial number for camera {:?}", node.path());
                continue;
            };
            let Some(cam_info) = serial_to_chain.get_mut(serial_num) else {
                warn!("Unexpected camera with serial number {:?}", serial_num);
                continue;
            };
            let Some(CameraInfo {
                k_node,
                focal_length_px,
            }) = cam_info.take()
            else {
                warn!(
                    "Camera with serial number {:?} already enumerated",
                    serial_num
                );
                continue;
            };

            let mut camera = match v4l::Device::with_path(node.path()) {
                Ok(x) => x,
                Err(e) => {
                    warn!("Failed to open camera {:?}: {e}", node.path());
                    continue;
                }
            };

            let format = match camera.format() {
                Ok(x) => x,
                Err(e) => {
                    warn!("Failed to get format for camera {:?}: {e}", node.path());
                    continue;
                }
            };
            let image = OwnedData::from(ImageBuffer::from_pixel(
                format.width,
                format.height,
                Luma([0]),
            ));
            let mut image = image.pessimistic_share();
            let det = AprilTagDetector::new(
                focal_length_px,
                format.width,
                format.height,
                image.create_lendee(),
            );
            let localizer_ref = localizer_ref.clone();
            let mut local_transform = k_node.origin();
            local_transform.inverse_mut();
            det.detection_callbacks_ref().add_fn(move |observation| {
                localizer_ref.set_april_tag_isometry(
                    local_transform * observation.get_isometry_of_observer(),
                );
            });
            det.run();

            std::thread::spawn(move || {
                let mut stream = MmapStream::with_buffers(&mut camera, Type::VideoCapture, 4)
                    .expect("Failed to create buffer stream");

                loop {
                    let (buf, _) = stream.next().unwrap();
                    match image.try_recall() {
                        Ok(mut img) => {
                            img.copy_from_slice(buf);
                            image = img.pessimistic_share();
                        }
                        Err(img) => {
                            image = img;
                        }
                    }
                }
            });
        }
    }

    for (serial_num, cam_info) in serial_to_chain {
        if cam_info.is_some() {
            warn!("Camera with serial number {serial_num} not found");
        }
    }

    Ok(())
}
