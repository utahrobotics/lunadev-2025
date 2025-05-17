use std::{
    io::Cursor,
    path::PathBuf,
    sync::mpsc::{Receiver, Sender, SyncSender},
};

use super::apriltag::{
    image::{self, ImageBuffer, ImageDecoder, Luma},
    AprilTagDetector, TagObservation, Apriltag,
};
use chrono::SubsecRound;
use fxhash::FxHashMap;
use rerun::Boxes3D;
use simple_motion::StaticImmutableNode;
use tracing::{error, info, warn};
use udev::{EventType, MonitorBuilder, Udev};
use v4l::{buffer::Type, io::traits::CaptureStream, prelude::MmapStream, video::Capture};

use crate::{apps::production::udev_poll, localization::LocalizerRef};

use super::{
    streaming::{CameraStream, DownscaleRgbImageReader},
};

pub struct CameraInfo {
    pub node: StaticImmutableNode,
    pub focal_length_x_px: f64,
    pub focal_length_y_px: f64,
    pub stream_index: Option<usize>,
}

pub fn enumerate_cameras(
    localizer_ref: &LocalizerRef,
    port_to_chain: impl IntoIterator<Item = (String, CameraInfo)>,
    apriltags: &'static [(usize, Apriltag)],
) {
    let mut threads: FxHashMap<String, SyncSender<PathBuf>> = port_to_chain
        .into_iter()
        .filter_map(
            |(
                port,
                CameraInfo {
                    node,
                    focal_length_x_px,
                    focal_length_y_px,
                    stream_index,
                },
            )| {
                let mut camera_stream = None;
                if let Some(stream_index) = stream_index {
                    let Some(tmp) = CameraStream::new(stream_index) else {
                        return None;
                    };
                    camera_stream = Some(tmp);
                }
                let port2 = port.clone();
                let localizer_ref = localizer_ref.clone();
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                std::thread::spawn(move || {
                    let mut camera_task = CameraTask {
                        path: rx,
                        port,
                        camera_stream,
                        focal_length_x_px,
                        focal_length_y_px,
                        apriltags,
                        localizer_ref,
                        node,
                    };
                    loop {
                        camera_task.camera_task();
                    }
                });
                Some((port2, tx))
            },
        )
        .collect();

    std::thread::spawn(move || {
        let mut monitor = match MonitorBuilder::new() {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to create udev monitor: {e}");
                return;
            }
        };
        monitor = match monitor.match_subsystem("video4linux") {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to set match-subsystem filter: {e}");
                return;
            }
        };
        let listener = match monitor.listen() {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to listen for udev events: {e}");
                return;
            }
        };

        let mut enumerator = {
            let udev = match Udev::new() {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to create udev context: {e}");
                    return;
                }
            };
            match udev::Enumerator::with_udev(udev) {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to create udev enumerator: {e}");
                    return;
                }
            }
        };
        if let Err(e) = enumerator.match_subsystem("video4linux") {
            error!("Failed to set match-subsystem filter: {e}");
        }
        let devices = match enumerator.scan_devices() {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to scan devices: {e}");
                return;
            }
        };
        devices
            .into_iter()
            .chain(
                udev_poll(listener)
                    .filter(|event| event.event_type() == EventType::Add)
                    .map(|event| event.device()),
            )
            .for_each(|device| {
                let Some(path) = device.devnode() else {
                    return;
                };
                // Valid camera paths are of the form /dev/videoN
                let Some(path_str) = path.to_str() else {
                    return;
                };
                if !path_str.starts_with("/dev/video") {
                    return;
                }
                let Some(udev_index) = device.attribute_value("index") else {
                    warn!("No udev_index for camera {path_str}");
                    return;
                };
                if udev_index.to_str() != Some("0") {
                    return;
                }
                if let Some(name) = device.attribute_value("name") {
                    if let Some(name) = name.to_str() {
                        if name.contains("RealSense") {
                            return;
                        }
                    }
                }
                let Some(port_raw) = device.property_value("ID_PATH") else {
                    warn!("No port for camera {path_str}");
                    return;
                };
                let Some(port) = port_raw.to_str() else {
                    warn!("Failed to parse port of camera {path_str}");
                    return;
                };
                if let Some(path_sender) = threads.get(port) {
                    if path_sender.send(path.to_path_buf()).is_err() {
                        threads.remove(port);
                    }
                } else {
                    warn!("Unexpected camera with port {}", port);
                }
            });
    });
}

struct CameraTask {
    path: Receiver<PathBuf>,
    port: String,
    camera_stream: Option<CameraStream>,
    focal_length_x_px: f64,
    focal_length_y_px: f64,
    apriltags: &'static [(usize, Apriltag)],
    localizer_ref: LocalizerRef,
    node: StaticImmutableNode,
}

impl CameraTask {
    fn camera_task(&mut self) {
        let path = match self.path.recv() {
            Ok(x) => x,
            Err(_) => loop {
                std::thread::park();
            },
        };
        let mut camera = match v4l::Device::with_path(&path) {
            Ok(x) => x,
            Err(e) => {
                warn!("Failed to open camera {}: {e}", self.port);
                return;
            }
        };
        let format = match camera.format() {
            Ok(x) => x,
            Err(e) => {
                warn!("Failed to get format for camera {}: {e}", self.port);
                return;
            }
        };
        
        // Create channels for image sharing with the AprilTag detector
        let (image_tx, image_rx) = std::sync::mpsc::channel();
        let (tag_tx, tag_rx) = std::sync::mpsc::channel();
        
        // Initialize detector
        let mut det = AprilTagDetector::new(
            self.focal_length_x_px,
            self.focal_length_y_px,
            format.width,
            format.height,
            image_rx,
            tag_tx,
        );
        
        for (tag_id, tag) in self.apriltags {
            det.add_tag(tag.tag_position, tag.get_quat(), tag.tag_width, *tag_id);
        }
        
        // Processing thread for apriltag detections
        let localizer_ref = self.localizer_ref.clone();
        let mut inverse_local = self.node.get_isometry_from_base();
        inverse_local.inverse_mut();
        
        let detection_thread = std::thread::spawn(move || {
            loop {
                match tag_rx.recv() {
                    Ok(observation) => {
                        info!("Tag {} detected", observation.tag_id);
                        if let Some(rec) = crate::apps::RECORDER.get() {
                            let location = (
                                observation.tag_global_isometry.translation.x as f32,
                                observation.tag_global_isometry.translation.y as f32,
                                observation.tag_global_isometry.translation.z as f32,
                            );
                            let seen_at = chrono::Local::now().time().trunc_subsecs(0);
                            let quaterion = observation
                                .tag_global_isometry
                                .rotation
                                .quaternion()
                                .as_vector()
                                .iter()
                                .map(|val| *val as f32)
                                .collect::<Vec<f32>>();
                            if let Err(e) = rec.recorder.log(
                                format!("apriltags/{}/location", observation.tag_id),
                                &Boxes3D::from_centers_and_half_sizes([(location)], [(0.1, 0.1, 0.01)])
                                    .with_quaternions([[
                                        quaterion[0],
                                        quaterion[1],
                                        quaterion[2],
                                        quaterion[3],
                                    ]])
                                    .with_labels([format!("{}", seen_at)]),
                            ) {
                                error!("Couldn't log april tag: {e}")
                            }
                        }
                        localizer_ref.set_april_tag_isometry(observation.get_isometry_of_observer() * inverse_local);
                    },
                    Err(_) => break,
                }
            }
        });
        
        // Start the detector in a separate thread
        std::thread::spawn(move || det.run());
        
        info!("Camera {} opened", self.port);
        
        // Create and initialize an empty image buffer
        let mut image_buffer = ImageBuffer::from_pixel(
            format.width,
            format.height,
            Luma([0]),
        );

        let mut stream = match MmapStream::with_buffers(&mut camera, Type::VideoCapture, 4) {
            Ok(x) => x,
            Err(e) => {
                warn!("Failed to create mmap stream for camera {}: {e}", self.port);
                return;
            }
        };

        let mut rgb_img = vec![0u8; format.width as usize * format.height as usize * 3];
        loop {
            let (jpg_img, _) = match stream.next() {
                Ok(x) => x,
                Err(e) => {
                    warn!("Failed to get next frame from camera {}: {e}", self.port);
                    break;
                }
            };

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

            if let Some(camera_stream) = &mut self.camera_stream {
                camera_stream
                    .write(DownscaleRgbImageReader::new(
                        &rgb_img,
                        format.width,
                        format.height,
                    ))
                    .unwrap();
            }

            // Convert RGB to grayscale and send to apriltag detector
            image_buffer
                .iter_mut()
                .zip(rgb_img.array_chunks::<3>().map(|[r, g, b]| {
                    (0.299 * *r as f64 + 0.587 * *g as f64 + 0.114 * *b as f64) as u8
                }))
                .for_each(|(dst, new)| {
                    *dst = new;
                });
                
            // Clone the image buffer and send it to the detector
            if let Err(e) = image_tx.send(image_buffer.clone()) {
                error!("Failed to send image to detector: {e}");
                break;
            }
        }
        
        // The end of the loop means camera failure, but we don't need to explicitly drop the channel
        // as it will be dropped when this function returns
        error!("Camera {} task exited", self.port);
    }
}
