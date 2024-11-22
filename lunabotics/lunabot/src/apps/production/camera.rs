use std::{io::Cursor, os::unix::ffi::OsStrExt};

use anyhow::Context;
use fxhash::{FxHashMap, FxHashSet};
use udev::Udev;
use urobotics::{
    log::{error, warn},
    shared::{OwnedData, SharedData},
};
use urobotics_apriltag::{
    image::{self, ImageBuffer, ImageDecoder, Luma},
    AprilTagDetector,
};
use v4l::{buffer::Type, format, io::traits::CaptureStream, prelude::MmapStream, video::Capture};

use crate::localization::LocalizerRef;

use super::{
    apriltag::Apriltag,
    streaming::{CameraStream, DownscaleRgbImageReader},
};

pub struct CameraInfo {
    pub k_node: k::Node<f64>,
    pub focal_length_x_px: f64,
    pub focal_length_y_px: f64,
    pub stream_index: usize,
}

pub fn enumerate_cameras(
    localizer_ref: LocalizerRef,
    port_to_chain: impl IntoIterator<Item = (String, CameraInfo)>,
    apriltags: &FxHashMap<usize, Apriltag>,
) -> anyhow::Result<()> {
    let mut port_to_chain: FxHashMap<String, Option<_>> = port_to_chain
        .into_iter()
        .map(|(port, chain)| (port, Some(chain)))
        .collect();
    {
        let udev = Udev::new()?;
        let mut enumerator = udev::Enumerator::with_udev(udev.clone())?;
        let mut seen = FxHashSet::default();

        for udev_device in enumerator.scan_devices()? {
            let Some(path) = udev_device.devnode() else {
                continue;
            };
            // Valid camera paths are of the form /dev/videoN
            let Some(path_str) = path.to_str() else {
                continue;
            };
            if !path_str.starts_with("/dev/video") {
                continue;
            }
            if let Some(name) = udev_device.attribute_value("name") {
                if let Some(name) = name.to_str() {
                    if name.contains("RealSense") {
                        continue;
                    }
                }
            }
            let Some(port_raw) = udev_device.property_value("ID_PATH") else {
                warn!("No port for camera {path_str}");
                continue;
            };
            let Some(port) = port_raw.to_str() else {
                warn!("Failed to parse port of camera {path_str}");
                continue;
            };
            if !seen.insert(port.to_string()) {
                continue;
            }
            let Some(cam_info) = port_to_chain.get_mut(port) else {
                warn!("Unexpected camera with port {}", port);
                continue;
            };
            let CameraInfo {
                k_node,
                focal_length_x_px,
                focal_length_y_px,
                stream_index,
            } = cam_info.take().unwrap();

            let mut camera = match v4l::Device::with_path(path) {
                Ok(x) => x,
                Err(e) => {
                    warn!("Failed to open camera {path_str}: {e}");
                    continue;
                }
            };

            let format = match camera.format() {
                Ok(x) => x,
                Err(e) => {
                    warn!("Failed to get format for camera {path_str}: {e}");
                    continue;
                }
            };
            // format.fourcc = v4l::FourCC::new(b"RGB3");
            // match camera.set_format(&format) {
            //     Ok(actual) => if actual.fourcc != format.fourcc {
            //         error!(
            //             "Failed to set format for camera {path_str}: {}", actual.fourcc
            //         );
            //         continue;
            //     },
            //     Err(e) => {
            //         warn!("Failed to get format for camera {path_str}: {e}");
            //         continue;
            //     }
            // }

            let Some(mut camera_stream) = CameraStream::new(stream_index) else {
                continue;
            };

            let image = OwnedData::from(ImageBuffer::from_pixel(
                format.width,
                format.height,
                Luma([0]),
            ));
            let mut image = image.pessimistic_share();
            let mut det = AprilTagDetector::new(
                focal_length_x_px,
                focal_length_y_px,
                format.width,
                format.height,
                image.create_lendee(),
            );
            for (&tag_id, tag) in apriltags {
                det.add_tag(tag.tag_position, tag.get_quat(), tag.tag_width, tag_id);
            }
            let localizer_ref = localizer_ref.clone();
            let mut inverse_local = k_node.origin();
            inverse_local.inverse_mut();
            det.detection_callbacks_ref().add_fn(move |observation| {
                // println!(
                //     "pos: [{:.2}, {:.2}, {:.2}] angle: {}deg axis: [{:.2}, {:.2}, {:.2}]",
                //     observation.tag_local_isometry.translation.x,
                //     observation.tag_local_isometry.translation.y,
                //     observation.tag_local_isometry.translation.z,
                //     (observation.tag_local_isometry.rotation.angle() / std::f64::consts::PI * 180.0).round() as i32,
                //     observation.tag_local_isometry.rotation.axis().unwrap().x,
                //     observation.tag_local_isometry.rotation.axis().unwrap().y,
                //     observation.tag_local_isometry.rotation.axis().unwrap().z,
                // );
                let pose = observation.get_isometry_of_observer();
                println!(
                    "pos: [{:.2}, {:.2}, {:.2}] angle: {}deg axis: [{:.2}, {:.2}, {:.2}]",
                    pose.translation.x,
                    pose.translation.y,
                    pose.translation.z,
                    (pose.rotation.angle() / std::f64::consts::PI * 180.0).round() as i32,
                    pose.rotation.axis().unwrap().x,
                    pose.rotation.axis().unwrap().y,
                    pose.rotation.axis().unwrap().z,
                );
                localizer_ref
                    .set_april_tag_isometry(inverse_local * observation.get_isometry_of_observer());
            });
            std::thread::spawn(move || det.run());

            std::thread::spawn(move || {
                let mut stream = MmapStream::with_buffers(&mut camera, Type::VideoCapture, 4)
                    .expect("Failed to create buffer stream");

                let mut rgb_img = vec![0u8; format.width as usize * format.height as usize * 3];
                loop {
                    let (jpg_img, _) = stream.next().unwrap();

                    match image::codecs::jpeg::JpegDecoder::new(Cursor::new(jpg_img)) {
                        Ok(decoder) => {
                            if let Err(e) = decoder.read_image(&mut rgb_img) {
                                error!("Failed to decode JPEG image: {e}");
                                continue;
                            }
                        }
                        Err(e) => {
                            error!("Failed to create JPEG decoder: {e}");
                            continue;
                        }
                    }

                    camera_stream.write(DownscaleRgbImageReader::new(
                        &rgb_img,
                        format.width,
                        format.height,
                    ));

                    match image.try_recall() {
                        Ok(img) => {
                            let (img, uninit) = img.uninit();
                            let mut vec = img.into_raw();
                            vec.clear();
                            vec.extend(rgb_img.array_chunks::<3>().map(|[r, g, b]| {
                                (0.299 * *r as f64 + 0.587 * *g as f64 + 0.114 * *b as f64) as u8
                            }));
                            let img = ImageBuffer::from_raw(format.width, format.height, vec)
                                .expect("Failed to create image buffer");
                            let img = uninit.init(img);
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

    for (port, cam_info) in port_to_chain {
        if cam_info.is_some() {
            error!("Camera with port {port} not found");
        }
    }

    Ok(())
}
