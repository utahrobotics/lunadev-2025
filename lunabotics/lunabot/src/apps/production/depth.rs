use std::{
    cell::OnceCell,
    f64::consts::PI,
    num::NonZeroU32,
    sync::mpsc::{Receiver, Sender, SyncSender},
    time::Duration,
};

use super::{
    apriltag::{
        image::{ImageBuffer, Luma},
        AprilTagDetector, Apriltag,
    },
    streaming::CameraStream,
    RECORDER, ROBOT, ROBOT_STRUCTURE,
};
use chrono::SubsecRound;
use fxhash::FxHashMap;
use gputter::types::{AlignedMatrix4, AlignedVec4};
use nalgebra::{UnitQuaternion, Vector2, Vector3, Vector4};
pub use realsense_rust;
use realsense_rust::{
    config::Config,
    device::Device,
    frame::{ColorFrame, DepthFrame, PixelKind, AccelFrame, GyroFrame},
    kind::{Rs2CameraInfo, Rs2Format, Rs2StreamKind},
    pipeline::{ActivePipeline, FrameWaitError, InactivePipeline},
};
use rerun::ImageFormat;
use simple_motion::StaticImmutableNode;
use tasker::shared::{MaybeOwned, OwnedData};
use thalassic::{DepthProjector, DepthProjectorBuilder, ThalassicPipelineRef};
use tracing::{error, info, warn};

use crate::{
    apps::production::streaming::DownscaleRgbImageReader,
    localization::LocalizerRef,
    pipelines::thalassic::{get_observe_depth, spawn_thalassic_pipeline, ThalassicData},
};

pub struct DepthCameraInfo {
    pub node: StaticImmutableNode,
    pub ignore_apriltags: bool,
    pub stream_index: usize,
}

const ESTIMATED_MAX_POINT_COUNT: u32 = 1024 * 812;

pub fn enumerate_realsense_devices(
    thalassic_buffer: OwnedData<ThalassicData>,
    localizer_ref: &LocalizerRef,
    serial_to_chain: impl IntoIterator<Item = (String, DepthCameraInfo)>,
    apriltags: &'static [(usize, Apriltag)],
) {
    let thalassic_ref = spawn_thalassic_pipeline(thalassic_buffer, ESTIMATED_MAX_POINT_COUNT);
    let (init_tx, init_rx) = std::sync::mpsc::channel::<&'static str>();
    let mut threads: FxHashMap<&str, SyncSender<(Device, ActivePipeline)>> = serial_to_chain
        .into_iter()
        .enumerate()
        .filter_map(
            |(index, (
                serial,
                DepthCameraInfo {
                    node,
                    ignore_apriltags,
                    stream_index,
                },
            ))| {
                let Some(camera_stream) = CameraStream::new(stream_index) else {
                    return None;
                };
                let serial: &_ = Box::leak(serial.into_boxed_str());
                let localizer_ref = localizer_ref.clone();
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                let init_tx = init_tx.clone();
                let isometry = node.get_local_isometry();

                if let Some(recorder) = RECORDER.get() {
                    let local_x = isometry.rotation * Vector3::x_axis();
                    let corrected_rotation =
                        UnitQuaternion::from_axis_angle(&local_x, PI) * isometry.rotation;
                    if let Err(e) = recorder.recorder.log_static(
                        format!("{ROBOT_STRUCTURE}/cameras/depth/{serial}"),
                        &rerun::Transform3D::from_translation_rotation(
                            isometry.translation.vector.cast::<f32>().data.0[0],
                            rerun::Quaternion::from_xyzw(
                                corrected_rotation.as_vector().cast::<f32>().data.0[0],
                            ),
                        ),
                    ) {
                        error!("Failed to log depth camera transform: {e}");
                    }
                }

                let thalassic_ref = thalassic_ref.clone();

                std::thread::spawn(move || {
                    let mut camera_task = DepthCameraTask {
                        pipeline: rx,
                        serial,
                        camera_stream,
                        state: OnceCell::new(),
                        apriltags,
                        localizer_ref,
                        node,
                        ignore_apriltags,
                        thalassic_ref,
                        init_tx,
                        index,
                    };
                    loop {
                        camera_task.depth_camera_task();
                    }
                });
                Some((serial, tx))
            },
        )
        .collect();

    let context = match realsense_rust::context::Context::new() {
        Ok(x) => x,
        Err(e) => {
            error!("Failed to get RealSense Context: {e}");
            return;
        }
    };
    let device_hub = match context.create_device_hub() {
        Ok(x) => x,
        Err(e) => {
            error!("Failed to create RealSense DeviceHub: {e}");
            return;
        }
    };

    std::thread::spawn(move || {
        loop {
            let Ok(target_serial) = init_rx.recv() else {
                break;
            };
            loop {
                let device = match device_hub.wait_for_device() {
                    Ok(x) => x,
                    Err(e) => {
                        error!("Failed to wait for RealSense device: {e}");
                        break;
                    }
                };
                // let Some(product_line_cstr) = device.info(Rs2CameraInfo::ProductLine) else {
                //     // Pseudo devices representing a RealSense Camera don't have a product line
                //     continue;
                // };
                let Some(current_serial_cstr) = device.info(Rs2CameraInfo::SerialNumber) else {
                    error!("Failed to get serial number for RealSense Camera");
                    continue;
                };
                let Ok(current_serial) = current_serial_cstr.to_str() else {
                    error!("Failed to parse serial number {:?}", current_serial_cstr);
                    continue;
                };
                if target_serial != current_serial {
                    continue;
                }
                // let Ok(product_line) = product_line_cstr.to_str() else {
                //     error!("Failed to parse product line {:?} for RealSense Camera {current_serial}", product_line_cstr);
                //     continue;
                // };
                // if product_line != "D400" {
                //     continue;
                // }
                let Some(pipeline_sender) = threads.get(current_serial) else {
                    warn!("Unexpected RealSense camera with serial {}", current_serial);
                    continue;
                };

                let Some(usb_cstr) = device.info(Rs2CameraInfo::UsbTypeDescriptor) else {
                    error!(
                        "Failed to read USB type descriptor for RealSense Camera {}",
                        current_serial
                    );
                    continue;
                };
                let Ok(usb_str) = usb_cstr.to_str() else {
                    error!(
                        "USB type descriptor for RealSense Camera {} is not utf-8",
                        current_serial
                    );
                    continue;
                };
                let Ok(usb_val) = usb_str.parse::<f32>() else {
                    error!(
                        "USB type descriptor for RealSense Camera {} is not f32",
                        current_serial
                    );
                    continue;
                };

                let mut config = Config::new();
                if let Err(e) = config.enable_device_from_serial(current_serial_cstr) {
                    error!("Failed to enable RealSense Camera {}: {e}", current_serial);
                    continue;
                }

                if let Err(e) = config.disable_all_streams() {
                    error!(
                        "Failed to disable all streams in RealSense Camera {}: {e}",
                        current_serial
                    );
                    continue;
                }

                if let Err(e) =
                    config.enable_stream(Rs2StreamKind::Depth, None, 0, 0, Rs2Format::Z16, 0)
                {
                    error!(
                        "Failed to enable depth stream in RealSense Camera {}: {e}",
                        current_serial
                    );
                    continue;
                }

                if let Err(e) = config.enable_stream(Rs2StreamKind::Accel,None, 0, 0, Rs2Format::MotionXyz32F, 0) {
                    error!(
                        "Failed to enable accel stream in RealSense Camera {}: {e}",
                        current_serial
                    );
                    continue;
                }

                if let Err(e) = config.enable_stream(Rs2StreamKind::Gyro, None, 0, 0, Rs2Format::MotionXyz32F, 0) {
                    error!(
                        "Failed to enable gyro stream in RealSense Camera {}: {e}",
                        current_serial
                    );
                    continue;
                }

                if let Err(e) =
                    config.enable_stream(Rs2StreamKind::Color, None, 0, 0, Rs2Format::Rgb8, 0)
                {
                    error!(
                        "Failed to enable color stream in RealSense Camera {}: {e}",
                        current_serial
                    );
                    continue;
                }

                if usb_val < 3.0 {
                    error!(
                        "Depth camera {} is connected to USB {usb_val}",
                        current_serial
                    );
                    continue;
                }

                let pipeline = match InactivePipeline::try_from(&context) {
                    Ok(x) => x,
                    Err(e) => {
                        warn!(
                            "Failed to open pipeline for RealSense Camera {}: {e}",
                            current_serial
                        );
                        continue;
                    }
                };
                let pipeline = match pipeline.start(Some(config)) {
                    Ok(x) => x,
                    Err(e) => {
                        error!(
                            "Failed to start pipeline for RealSense Camera {}: {e}",
                            current_serial
                        );
                        continue;
                    }
                };

                let current_serial = current_serial.to_string();
                if let Err(error) = pipeline_sender.send((device, pipeline)) {
                    error.0 .1.stop();
                    threads.remove(current_serial.as_str());
                }
                break;
            }
        }
    });

    std::thread::spawn(move || {
        info!("Thalassic pipeline spawned");
    });
}

struct DepthCameraState {
    image: MaybeOwned<ImageBuffer<Luma<u8>, Vec<u8>>>,
    depth_projector: DepthProjector,
    point_cloud: Box<[AlignedVec4<f32>]>,
}

struct DepthCameraTask {
    pipeline: Receiver<(Device, ActivePipeline)>,
    serial: &'static str,
    camera_stream: CameraStream,
    state: OnceCell<DepthCameraState>,
    apriltags: &'static [(usize, Apriltag)],
    localizer_ref: LocalizerRef,
    node: StaticImmutableNode,
    ignore_apriltags: bool,
    thalassic_ref: ThalassicPipelineRef,
    init_tx: Sender<&'static str>,
    index: usize,
}

enum StreamType {
    Depth,
    Img,
    Accel,
}

impl StreamType {
    fn is_depth(&self) -> bool {
        match self {
            Self::Depth => {
                true
            }
            _ => {
                false
            }
        }
    }
    fn is_img(&self) -> bool {
        match self {
            Self::Img => {
                true
            }
            _ => {
                false
            }
        }
    }
}

impl DepthCameraTask {

    fn depth_camera_task(&mut self) {
        let _ = self.init_tx.send(self.serial);
        let (device, mut pipeline) = match self.pipeline.recv() {
            Ok(x) => x,
            Err(_) => loop {
                std::thread::park();
            },
        };

        let mut depth_format = None;
        let mut color_format = None;

        for stream in pipeline.profile().streams() {
            let stream_type = match stream.format() {
                Rs2Format::Rgb8 => StreamType::Img,
                Rs2Format::Z16 => StreamType::Depth,
                Rs2Format::MotionXyz32F => {
                    StreamType::Accel                 
                },
                format => {
                    error!("Unexpected format {format:?} for {}", self.serial);
                    continue;
                }
            };
            let intrinsics = match stream.intrinsics() {
                Ok(x) => x,
                Err(e) => {
                    if stream_type.is_depth() {
                        error!(
                            "Failed to get depth intrinsics for RealSense camera {}: {e}",
                            self.serial
                        );
                    } else if stream_type.is_img() {
                        error!(
                            "Failed to get color intrinsics for RealSense camera {}: {e}",
                            self.serial
                        );
                    }
                    continue;
                }
            };
            if stream_type.is_depth() {
                depth_format = Some(intrinsics);
            } else if stream_type.is_img() {
                color_format = Some(intrinsics);
            }
        }

        let Some(depth_format) = depth_format else {
            error!(
                "Depth stream missing after initialization of {}",
                self.serial
            );
            return;
        };
        let Some(color_format) = color_format else {
            error!(
                "Color stream missing after initialization of {}",
                self.serial
            );
            return;
        };

        let DepthCameraState {
            image,
            depth_projector,
            point_cloud,
        } = if let Some(state) = self.state.get_mut() {
            if state.image.width() as usize != color_format.width()
                || state.image.height() as usize != color_format.height()
            {
                warn!("RealSense Color Camera {} format changed", self.serial);
                return;
            }
            state
        } else {
            let mut image = OwnedData::from(ImageBuffer::from_pixel(
                color_format.width() as u32,
                color_format.height() as u32,
                Luma([0]),
            ));
            if !self.ignore_apriltags {
                let mut det = AprilTagDetector::new(
                    color_format.fx() as f64,
                    color_format.fy() as f64,
                    color_format.width() as u32,
                    color_format.height() as u32,
                    image.create_lendee(),
                );
                for (tag_id, tag) in self.apriltags {
                    det.add_tag(tag.tag_position, tag.get_quat(), tag.tag_width, *tag_id);
                }
                let localizer_ref = self.localizer_ref.clone();
                let mut inverse_local = self.node.get_local_isometry();
                inverse_local.inverse_mut();
                det.detection_callbacks_ref().add_fn(move |observation| {
                    if let Some(rec) = crate::apps::RECORDER.get() {
                        let location = (
                            observation.tag_global_isometry.translation.x as f32, 
                            observation.tag_global_isometry.translation.y as f32, 
                            observation.tag_global_isometry.translation.z as f32);
                        let seen_at = chrono::Local::now().time().trunc_subsecs(0);
                        let quaterion = observation.tag_global_isometry.rotation.quaternion().as_vector().iter().map(
                            |val| *val as f32
                        ).collect::<Vec<f32>>();
                        if let Err(e) = rec.recorder.log(
                            format!("apriltags/{}/location",observation.tag_id), 
                            &rerun::Boxes3D::from_centers_and_half_sizes(
                                [(location)],
                                [(0.1, 0.1, 0.01)]
                            ).with_quaternions(
                                [
                                    [quaterion[0], quaterion[1], quaterion[2], quaterion[3]]
                                ]
                            ).with_labels(
                                [format!("{}", seen_at)]
                            )
                        ) {
                            error!("Couldn't log april tag: {e}")
                        }
                    }
                    localizer_ref.set_april_tag_isometry(
                        inverse_local * observation.get_isometry_of_observer(),
                    );
                });
                std::thread::spawn(move || det.run());
            }

            let focal_length_px;

            if depth_format.fx() != depth_format.fy() {
                warn!("Depth camera {} has unequal fx and fy", self.serial);
                focal_length_px = (depth_format.fx() + depth_format.fy()) / 2.0;
            } else {
                focal_length_px = depth_format.fx();
            }

            if let Some(recorder) = RECORDER.get() {
                if let Err(e) = recorder.recorder.log(
                    format!(
                        "{ROBOT_STRUCTURE}/cameras/depth/{}/depth_image",
                        self.serial
                    ),
                    &rerun::Pinhole::from_focal_length_and_resolution(
                        [depth_format.fx(), depth_format.fy()],
                        [depth_format.width() as f32, depth_format.height() as f32],
                    ),
                ) {
                    error!(
                        "Failed to log depth camera intrinsics for {}: {e}",
                        self.serial
                    );
                }
            }

            let depth_projecter_builder = DepthProjectorBuilder {
                image_size: Vector2::new(
                    NonZeroU32::new(depth_format.width() as u32).unwrap(),
                    NonZeroU32::new(depth_format.height() as u32).unwrap(),
                ),
                focal_length_px,
                principal_point_px: Vector2::new(depth_format.ppx(), depth_format.ppy()),
                max_depth: 2.0,
            };

            let depth_projector = depth_projecter_builder.build(self.thalassic_ref.clone());

            let _ = self.state.set(DepthCameraState {
                image: image.into(),
                point_cloud: std::iter::repeat_n(
                    AlignedVec4::from(Vector4::default()),
                    depth_projector.get_pixel_count().get() as usize,
                )
                .collect(),
                depth_projector,
            });
            self.state.get_mut().unwrap()
        };

        info!("RealSense Camera {} opened", self.serial);

        loop {
            let frames = match pipeline.wait(Some(Duration::from_millis(1000))) {
                Ok(x) => x,
                Err(e) => {
                    error!(
                        "Failed to get frame from RealSense Camera {}: {e}",
                        self.serial
                    );
                    if matches!(e, FrameWaitError::DidTimeoutBeforeFrameArrival) {
                        device.hardware_reset();
                    }
                    break;
                }
            };
            for frame in frames.frames_of_type::<AccelFrame>() {
                let [x,y,z] = frame.acceleration();
                self.localizer_ref.set_realsense_imu_accel(
                    self.index,
                    crate::localization::RsIMUAccel {
                        acceleration: Vector3::new(*x, *y, *z).cast()
                    }
                );
            }
            for frame in frames.frames_of_type::<GyroFrame>() {
                let [x,y,z] = frame.rotational_velocity();
                self.localizer_ref.set_realsense_imu_angular(
                    self.index,
                    crate::localization::RsIMUAngular {
                        angular_velocity: Vector3::new(*x, *y, *z).cast()
                    }
                );
            }

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

                if image.try_recall() {
                    let owned_image: &mut ImageBuffer<Luma<u8>, Vec<u8>> = image.get_mut().unwrap();
                    owned_image
                        .iter_mut()
                        .zip(bytes.array_chunks::<3>().map(|[r, g, b]| {
                            (0.299 * *r as f64 + 0.587 * *g as f64 + 0.114 * *b as f64) as u8
                        }))
                        .for_each(|(dst, new)| {
                            *dst = new;
                        });
                    image.share();
                }

                self.camera_stream
                    .write(DownscaleRgbImageReader::new(
                        &bytes,
                        frame.width() as u32,
                        frame.height() as u32,
                    ))
                    .unwrap();
            }

            let observe_depth = get_observe_depth();
            for frame in frames.frames_of_type::<DepthFrame>() {
                if !observe_depth && RECORDER.get().is_none() {
                    continue;
                }
                if !matches!(frame.get(0, 0), Some(PixelKind::Z16 { .. })) {
                    error!("Unexpected depth pixel kind: {:?}", frame.get(0, 0));
                }
                debug_assert_eq!(frame.bits_per_pixel(), 16);
                debug_assert_eq!(frame.width() * frame.height() * 2, frame.get_data_size());
                let slice;
                let bytes_slice;
                unsafe {
                    let data: *const _ = frame.get_data();
                    slice = std::slice::from_raw_parts(
                        data.cast::<u16>(),
                        frame.width() * frame.height(),
                    );
                    bytes_slice = std::slice::from_raw_parts(
                        data.cast::<u8>(),
                        frame.width() * frame.height() * 2,
                    );
                }

                let depth_scale = match frame.depth_units() {
                    Ok(x) => x,
                    Err(e) => {
                        error!(
                            "Failed to get depth scale from RealSense Camera {}: {e}",
                            self.serial
                        );
                        continue;
                    }
                };
                let camera_transform = self.node.get_global_isometry();
                let camera_transform: AlignedMatrix4<f32> =
                    camera_transform.to_homogeneous().cast::<f32>().into();
                depth_projector.project(slice, &camera_transform, depth_scale, Some(point_cloud));

                if let Some(recorder) = RECORDER.get() {
                    let result: rerun::RecordingStreamResult<()> = try {
                        recorder.recorder.log(
                            format!(
                                "{ROBOT_STRUCTURE}/cameras/depth/{}/depth_image",
                                self.serial
                            ),
                            &rerun::DepthImage::new(
                                bytes_slice,
                                ImageFormat::depth(
                                    [frame.width() as u32, frame.height() as u32],
                                    rerun::ChannelDatatype::U16,
                                ),
                            )
                            .with_meter(1.0 / depth_scale)
                            .with_depth_range([0.0, 2.0 / depth_scale as f64]),
                        )?;
                        recorder.recorder.log(
                            format!("{ROBOT}/point_clouds/{}", self.serial),
                            &rerun::Points3D::new(
                                point_cloud
                                    .iter()
                                    .filter(|point| point.w == 1.0)
                                    .map(|point| [point.x, point.y, point.z]),
                            )
                            .with_radii(std::iter::repeat_n(0.003, point_cloud.len())),
                        )?;
                    };
                    if let Err(e) = result {
                        error!("Failed to log depth for {}: {e}", self.serial);
                    }
                }
            }
        }

        error!("RealSense Camera {} closed", self.serial);
    }
}
