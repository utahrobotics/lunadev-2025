use std::{
    ffi::OsString,
    num::NonZeroU32,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use anyhow::Context;
use fxhash::FxHashMap;
use gputter::types::{AlignedMatrix4, AlignedVec4};
use nalgebra::{Vector2, Vector4};
pub use realsense_rust;
use realsense_rust::{
    config::{Config, ConfigurationError},
    context::ContextConstructionError,
    device::Device,
    frame::{ColorFrame, DepthFrame, PixelKind},
    kind::{Rs2CameraInfo, Rs2Format, Rs2ProductLine, Rs2StreamKind},
    pipeline::{ActivePipeline, FrameWaitError, InactivePipeline},
};
use thalassic::DepthProjectorBuilder;
use urobotics::{
    log::{error, warn},
    shared::OwnedData,
};
use urobotics_apriltag::{
    image::{ImageBuffer, Luma, Rgb},
    AprilTagDetector,
};

use crate::{
    apps::production::streaming::DownscaleRgbImageReader,
    localization::LocalizerRef,
    pipelines::thalassic::{spawn_thalassic_pipeline, HeightMapCallbacksRef, PointsStorageChannel},
};

use super::{apriltag::Apriltag, streaming::CameraStream};

pub struct DepthCameraInfo {
    pub k_node: k::Node<f64>,
    pub ignore_apriltags: bool,
    pub stream_index: usize,
}

/// Returns an iterator over all the RealSense cameras that were identified.
pub fn enumerate_depth_cameras(
    localizer_ref: LocalizerRef,
    serial_to_chain: impl IntoIterator<Item = (String, DepthCameraInfo)>,
    apriltags: &FxHashMap<usize, Apriltag>,
) -> (HeightMapCallbacksRef, anyhow::Result<()>) {
    let context =
        match realsense_rust::context::Context::new().context("Failed to get RealSense Context") {
            Ok(x) => x,
            Err(e) => {
                return (spawn_thalassic_pipeline(Box::new([])).0, Err(e).into());
            }
        };
    let devices = context.query_devices(Some(Rs2ProductLine::Depth).into_iter().collect());
    let mut pcl_storage_channels = vec![];

    let mut serial_to_chain: FxHashMap<String, Option<_>> = serial_to_chain
        .into_iter()
        .map(|(serial, chain)| (serial, Some(chain)))
        .collect();

    for device in devices {
        let port = device.info(Rs2CameraInfo::PhysicalPort);
        let Some(serial_cstr) = device.info(Rs2CameraInfo::SerialNumber) else {
            if let Some(port) = port {
                error!("Failed to get serial number for {:?}", port);
            } else {
                error!("Failed to get serial number and port for depth camera");
            }
            continue;
        };
        let Ok(serial) = serial_cstr.to_str() else {
            if let Some(port) = port {
                error!(
                    "Failed to parse serial number {:?} for {:?}",
                    serial_cstr, port
                );
            } else {
                error!("Failed to parse serial number {:?}", serial_cstr);
            }
            continue;
        };
        let serial = serial.to_string();

        let Some(cam_info) = serial_to_chain.get_mut(&serial) else {
            warn!("Unexpected depth camera with serial number {}", serial);
            continue;
        };
        let Some(DepthCameraInfo {
            k_node,
            ignore_apriltags,
            stream_index,
        }) = cam_info.take()
        else {
            error!("Depth Camera {} already enumerated", serial);
            continue;
        };

        let Some(mut camera_stream) = CameraStream::new(stream_index) else {
            continue;
        };

        let mut config = Config::new();

        let Some(usb_cstr) = device.info(Rs2CameraInfo::UsbTypeDescriptor) else {
            error!("Failed to read USB type descriptor for depth camera {serial}");
            continue;
        };
        let Ok(usb_str) = usb_cstr.to_str() else {
            error!("USB type descriptor for depth camera {serial} is not utf-8");
            continue;
        };
        let Ok(usb_val) = usb_str.parse::<f32>() else {
            error!("USB type descriptor for depth camera {serial} is not f32");
            continue;
        };

        if let Err(e) = config.enable_device_from_serial(serial_cstr) {
            error!("Failed to enable depth camera {serial}: {e}");
            continue;
        }

        if let Err(e) = config.disable_all_streams() {
            error!("Failed to disable all streams in depth camera {serial}: {e}");
            continue;
        }

        if let Err(e) = config.enable_stream(Rs2StreamKind::Depth, None, 0, 0, Rs2Format::Z16, 0) {
            error!("Failed to enable depth stream in depth camera {serial}: {e}");
            continue;
        }

        let mut expecting_color = false;
        if usb_val >= 3.0 {
            if let Err(e) =
                config.enable_stream(Rs2StreamKind::Color, None, 0, 0, Rs2Format::Rgb8, 0)
            {
                error!("Failed to enable color stream in depth camera {serial}: {e}");
            } else {
                expecting_color = true;
            }
        } else {
            warn!("Depth camera {serial} is not connected to USB {usb_val}");
        }

        let pipeline = match InactivePipeline::try_from(&context) {
            Ok(x) => x,
            Err(e) => {
                warn!("Failed to open pipeline for depth camera {serial}: {e}");
                continue;
            }
        };
        let mut pipeline = match pipeline.start(Some(config)) {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to start pipeline for depth camera {serial}: {e}");
                continue;
            }
        };
        let mut depth_projecter = None;
        let mut shared_luma_img = None;

        for stream in pipeline.profile().streams() {
            let is_depth = match stream.format() {
                Rs2Format::Rgb8 => false,
                Rs2Format::Z16 => {
                    if depth_projecter.is_some() {
                        error!("Already handled depth stream for depth camera {serial}");
                        continue;
                    }
                    true
                }
                format => {
                    error!("Unexpected format {format:?} for {serial}");
                    continue;
                }
            };
            let intrinsics = match stream.intrinsics() {
                Ok(x) => x,
                Err(e) => {
                    if is_depth {
                        error!("Failed to get depth intrinsics for depth camera {serial}: {e}");
                    } else {
                        error!("Failed to get color intrinsics for depth camera {serial}: {e}");
                    }
                    continue;
                }
            };

            if is_depth {
                let focal_length_px;

                if intrinsics.fx() != intrinsics.fy() {
                    warn!("Depth camera {serial} has unequal fx and fy");
                    focal_length_px = (intrinsics.fx() + intrinsics.fy()) / 2.0;
                } else {
                    focal_length_px = intrinsics.fx();
                }
                let depth_projecter_builder = DepthProjectorBuilder {
                    image_size: Vector2::new(
                        NonZeroU32::new(intrinsics.width() as u32).unwrap(),
                        NonZeroU32::new(intrinsics.height() as u32).unwrap(),
                    ),
                    focal_length_px,
                    principal_point_px: Vector2::new(intrinsics.ppx(), intrinsics.ppy()),
                };
                let pcl_storage = depth_projecter_builder.make_points_storage();
                let pcl_storage_channel = Arc::new(PointsStorageChannel::new_for(&pcl_storage));
                pcl_storage_channel.set_projected(pcl_storage);
                pcl_storage_channels.push(pcl_storage_channel.clone());
                depth_projecter = Some((depth_projecter_builder.build(), pcl_storage_channel));
            } else if !expecting_color {
                error!("Received color stream for depth camera {serial} even though it was not expected");
            } else if !ignore_apriltags {
                if shared_luma_img.is_some() {
                    error!("Already handled color stream for depth camera {serial}");
                    continue;
                }
                let mut img_shared = OwnedData::from(
                    ImageBuffer::<Luma<u8>, _>::from_raw(
                        intrinsics.width() as u32,
                        intrinsics.height() as u32,
                        vec![0u8; intrinsics.width() as usize * intrinsics.height() as usize],
                    )
                    .unwrap(),
                );
                let mut det = AprilTagDetector::new(
                    intrinsics.fx() as f64,
                    intrinsics.fy() as f64,
                    intrinsics.width() as u32,
                    intrinsics.height() as u32,
                    img_shared.create_lendee(),
                );
                for (&tag_id, tag) in apriltags {
                    det.add_tag(tag.tag_position, tag.get_quat(), tag.tag_width, tag_id);
                }
                let localizer_ref = localizer_ref.clone();
                let mut inverse_local = k_node.origin();
                inverse_local.inverse_mut();
                det.detection_callbacks_ref().add_fn(move |observation| {
                    localizer_ref.set_april_tag_isometry(
                        inverse_local * observation.get_isometry_of_observer(),
                    );
                });
                std::thread::spawn(move || det.run());
                shared_luma_img = Some(img_shared.pessimistic_share());
            }
        }
        let Some((mut depth_projecter, pcl_storage_channel)) = depth_projecter else {
            error!("Depth stream missing after initialization of {serial}");
            continue;
        };
        let mut point_cloud: Box<[_]> = std::iter::repeat_n(
            AlignedVec4::from(Vector4::default()),
            depth_projecter.get_pixel_count().get() as usize,
        )
        .collect();

        std::thread::spawn(move || loop {
            let frames = match pipeline.wait(None) {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to get frame from depth camera {serial}: {e}");
                    break;
                }
            };

            if let Some(mut luma_img) = shared_luma_img {
                for frame in frames.frames_of_type::<ColorFrame>() {
                    // This is a bug in RealSense. It will say the pixel kind is BGR8 when it is actually RGB8.
                    if !matches!(frame.get(0, 0), Some(PixelKind::Bgr8 { .. })) {
                        error!("Unexpected color pixel kind: {:?}", frame.get(0, 0));
                    }
                    debug_assert_eq!(frame.bits_per_pixel(), 24);
                    debug_assert_eq!(frame.width() * frame.height() * 3, frame.get_data_size());
                    let bytes = unsafe {
                        let data: *const _ = frame.get_data();
                        std::slice::from_raw_parts(data.cast::<u8>(), frame.get_data_size())
                    };

                    match luma_img.try_recall() {
                        Ok(mut owned_img) => {
                            let (img, uninit) = owned_img.uninit();
                            let mut buffer = img.into_raw();
                            buffer.clear();
                            buffer.extend(bytes.array_chunks::<3>().map(|[b, g, r]| {
                                (0.299 * *r as f64 + 0.587 * *g as f64 + 0.114 * *b as f64) as u8
                            }));
                            owned_img = uninit.init(
                                ImageBuffer::from_raw(
                                    frame.width() as u32,
                                    frame.height() as u32,
                                    buffer,
                                )
                                .unwrap(),
                            );
                            luma_img = owned_img.pessimistic_share();
                        }
                        Err(x) => {
                            luma_img = x;
                        }
                    }

                    camera_stream
                        .write(DownscaleRgbImageReader::new(
                            &bytes,
                            frame.width() as u32,
                            frame.height() as u32,
                        ))
                        .unwrap();
                }
                shared_luma_img = Some(luma_img);
            }

            for frame in frames.frames_of_type::<DepthFrame>() {
                if !matches!(frame.get(0, 0), Some(PixelKind::Z16 { .. })) {
                    error!("Unexpected depth pixel kind: {:?}", frame.get(0, 0));
                }
                debug_assert_eq!(frame.bits_per_pixel(), 16);
                debug_assert_eq!(frame.width() * frame.height() * 2, frame.get_data_size());
                unsafe {
                    let data: *const _ = frame.get_data();
                    let slice = std::slice::from_raw_parts(
                        data.cast::<u16>(),
                        frame.width() * frame.height(),
                    );

                    let Some(camera_transform) = k_node.world_transform() else {
                        continue;
                    };
                    let camera_transform: AlignedMatrix4<f32> =
                        camera_transform.to_homogeneous().cast::<f32>().into();
                    let Some(mut pcl_storage) = pcl_storage_channel.get_finished() else {
                        break;
                    };
                    let depth_scale = match frame.depth_units() {
                        Ok(x) => x,
                        Err(e) => {
                            error!("Failed to get depth scale from depth camera {serial}: {e}");
                            continue;
                        }
                    };
                    pcl_storage =
                        depth_projecter.project(slice, &camera_transform, pcl_storage, depth_scale);
                    pcl_storage.read(&mut point_cloud);
                    pcl_storage_channel.set_projected(pcl_storage);
                }
            }
        });
    }

    for (serial_num, cam_info) in serial_to_chain {
        if cam_info.is_some() {
            error!("Depth camera with serial number {serial_num} not found");
        }
    }

    let (heightmap_callbacks,) = spawn_thalassic_pipeline(pcl_storage_channels.into_boxed_slice());

    (heightmap_callbacks, Ok(()))
}
