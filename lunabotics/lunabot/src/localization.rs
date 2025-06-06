use std::{sync::Arc, time::Duration};

#[cfg(feature = "production")]
use cakap2::packet::PacketBody;
#[cfg(feature = "production")]
use common::FromLunabot;
use crossbeam::atomic::AtomicCell;
use nalgebra::{Isometry3, UnitQuaternion, UnitVector3, Vector3};
use simple_motion::StaticNode;
use spin_sleep::SpinSleeper;
use tracing::error;

#[cfg(not(feature = "production"))]
use crate::apps::LunasimStdin;
#[cfg(feature = "production")]
use crate::teleop::PacketBuilder;
use crate::utils::{lerp, lerp_value, swing_twist_decomposition};

use std::time::Instant;

#[cfg(feature = "production")]
use imu_calib::*;

const ACCELEROMETER_LERP_SPEED: f64 = 150.0;
const LOCALIZATION_DELTA: f64 = 1.0 / 60.0;

const APRILTAG_LERP_ALPHA: f64 = 0.01;

#[derive(Clone, Copy, Debug, Default)]
pub struct IMUReading {
    pub angular_velocity: Vector3<f64>,
    pub acceleration: Vector3<f64>,
}

#[derive(Default)]
struct LocalizerRefInner {
    april_tag_isometry: AtomicCell<Option<Isometry3<f64>>>,
    imu_readings: Box<[AtomicCell<Option<IMUReading>>]>,

    #[cfg(feature = "production")]
    imu_correction: AtomicCell<Option<CalibrationParameters>>,

}

#[derive(Clone)]
pub struct LocalizerRef {
    inner: Arc<LocalizerRefInner>,
}

impl LocalizerRef {
    #[cfg(feature = "production")]
    pub fn set_imu_correction_parameters(&self, correction: Option<CalibrationParameters>) {
        self.inner.imu_correction.store(correction);
    }

    pub fn set_april_tag_isometry(&self, isometry: Isometry3<f64>) {
        self.inner.april_tag_isometry.store(Some(isometry));
    }
    pub fn set_imu_reading(&self, index: usize, imu: IMUReading) {
        if let Some(cell) = self.inner.imu_readings.get(index) {
            cell.store(Some(imu));
        } else {
            error!("Tried to set an IMU reading at an invalid index: {}", index);
        }
    }

    fn take_imu_readings(&self) -> Option<IMUReading> {
        let mut out = IMUReading {
            angular_velocity: Vector3::zeros(),
            acceleration: Vector3::zeros(),
        };
        let mut count = 0usize;
        self.inner.imu_readings.iter().for_each(|reading| {
            let Some(reading) = reading.take() else {
                return;
            };

            out.angular_velocity += reading.angular_velocity;
            out.acceleration += reading.acceleration;
            count += 1;
        });
        if count > 0 {
            out.angular_velocity /= count as f64;
            out.acceleration /= count as f64;
            Some(out)
        } else {
            None
        }
    }

    fn april_tag_isometry(&self) -> Option<Isometry3<f64>> {
        self.inner.april_tag_isometry.load()
    }
}

pub struct Localizer {
    root_node: StaticNode,
    #[cfg(not(feature = "production"))]
    lunasim_stdin: Option<LunasimStdin>,
    #[cfg(feature = "production")]
    packet_builder: PacketBuilder,
    localizer_ref: LocalizerRef,
    #[cfg(feature = "production")]
    pub isometry_sync_delta: f64,
}

impl Localizer {
    #[cfg(not(feature = "production"))]
    pub fn new(
        root_node: StaticNode,
        lunasim_stdin: Option<LunasimStdin>,
        imu_count: usize,
    ) -> Self {
        Self {
            root_node,
            lunasim_stdin,
            localizer_ref: LocalizerRef {
                inner: Arc::new(LocalizerRefInner {
                    imu_readings: (0..imu_count).map(|_| AtomicCell::new(None)).collect(),
                    ..Default::default()
                }),
            },
        }
    }
    #[cfg(feature = "production")]
    pub fn new(root_node: StaticNode, imu_count: usize, packet_builder: PacketBuilder) -> Self {
        Self {
            root_node,
            localizer_ref: LocalizerRef {
                inner: Arc::new(LocalizerRefInner {
                    imu_readings: (0..imu_count).map(|_| AtomicCell::new(None)).collect(),
                    ..Default::default()
                }),
            },
            packet_builder,
            isometry_sync_delta: 0.1,
        }
    }

    pub fn get_ref(&self) -> LocalizerRef {
        self.localizer_ref.clone()
    }

    pub fn run(self) {
        #[cfg(feature = "production")]
        let lift_hinge_node = self.root_node.get_node_with_name("lift_hinge").unwrap();
        #[cfg(feature = "production")]
        let bucket_node = self.root_node.get_node_with_name("bucket").unwrap();
        let spin_sleeper = SpinSleeper::default();
        #[cfg(not(feature = "production"))]
        let mut bitcode_buffer = bitcode::Buffer::new();
        #[cfg(feature = "production")]
        let mut isometry_sync_timer = self.isometry_sync_delta;

        #[cfg(feature = "calibrate")]
        let start_time = std::time::Instant::now();

        #[cfg(feature = "calibrate")]
        let mut calibrator = Calibrator::new();

        // let mut sum = Vector3::zeros();
        // let mut sum_gyro = Vector3::zeros();
        // let mut iterations = 0;
        loop {
            #[cfg(not(feature = "calibrate"))]
            // allow the loop to speed up if calibrating to collect samples faster
            spin_sleeper.sleep(Duration::from_secs_f64(LOCALIZATION_DELTA));

            let mut isometry = self.root_node.get_global_isometry();

            'check: {
                if isometry.translation.x.is_nan()
                    || isometry.translation.y.is_nan()
                    || isometry.translation.z.is_nan()
                {
                    error!("Robot origin is NaN");
                } else if isometry.translation.x.is_infinite()
                    || isometry.translation.y.is_infinite()
                    || isometry.translation.z.is_infinite()
                {
                    error!("Robot origin is infinite");
                } else if isometry.rotation.w.is_nan()
                    || isometry.rotation.i.is_nan()
                    || isometry.rotation.j.is_nan()
                    || isometry.rotation.k.is_nan()
                {
                    error!("Robot rotation is NaN");
                } else if isometry.rotation.w.is_infinite()
                    || isometry.rotation.i.is_infinite()
                    || isometry.rotation.j.is_infinite()
                    || isometry.rotation.k.is_infinite()
                {
                    error!("Robot rotation is infinite");
                } else {
                    break 'check;
                }
                self.root_node.set_isometry(Isometry3::identity());
            }

            let mut down_axis = -Vector3::y_axis();
            let mut angular_velocity = None;

            if let Some(IMUReading {
                acceleration,
                angular_velocity: tmp_angular_velocity,
            }) = self.localizer_ref.take_imu_readings()
            {
                if tmp_angular_velocity.x.is_finite()
                    && tmp_angular_velocity.y.is_finite()
                    && tmp_angular_velocity.z.is_finite()
                {
                    #[cfg(feature = "calibrate")]
                    calibrator.add_static_sample(acceleration, tmp_angular_velocity);

                    #[cfg(feature = "production")]
                    if let Some(correction) = self.localizer_ref.inner.imu_correction.load() {
                        let corrected = correction.correct_gyroscope(&tmp_angular_velocity);
                        angular_velocity = Some(corrected);
                    } else {
                        angular_velocity = Some(tmp_angular_velocity);
                    }

                    #[cfg(not(feature = "production"))]
                    {
                        angular_velocity = Some(tmp_angular_velocity);
                    }
                }

                #[cfg(feature = "production")]
                let acceleration =
                    if let Some(correction) = self.localizer_ref.inner.imu_correction.load() {
                        let corrected = correction.correct_accelerometer(&acceleration);
                        corrected
                    } else {
                        acceleration
                    };

                #[cfg(feature = "calibrate")]
                if start_time.elapsed().as_secs() > 10 {
                    println!("Calibrating, this may take a while...");
                    println!("Number of Samples: {}", calibrator.sample_count());
                    // calibrate without trying to find scaling biases
                    match calibrator.calibrate(false) {
                        Ok(correction) => {
                            // expect is ok here because worst case the calibration fails.
                            println!(
                                "Correction parameters: {}",
                                correction
                                    .serialize_to_string()
                                    .expect("couldn't serialize correction params")
                            );
                            std::process::exit(0);
                        }
                        Err(e) => {
                            tracing::error!("Failed to calculate Correction parameters: {e}");
                            std::process::exit(0);
                        }
                    };
                }

                let acceleration =
                    UnitVector3::new_normalize(isometry.transform_vector(&acceleration));
                if acceleration.x.is_finite()
                    && acceleration.y.is_finite()
                    && acceleration.z.is_finite()
                {
                    let angle = down_axis.angle(&acceleration)
                        * lerp_value(LOCALIZATION_DELTA, ACCELEROMETER_LERP_SPEED);

                    if angle > 0.001 {
                        let cross = UnitVector3::new_normalize(down_axis.cross(&acceleration));
                        isometry.append_rotation_wrt_center_mut(&UnitQuaternion::from_axis_angle(
                            &cross, -angle,
                        ));
                    }
                }
            }

            down_axis = isometry.rotation * down_axis;

            if let Some(tag_isometry) = self.localizer_ref.april_tag_isometry() {
                // Lerp the translation
                isometry.translation.vector = lerp(
                    isometry.translation.vector,
                    tag_isometry.translation.vector,
                    LOCALIZATION_DELTA,
                    ACCELEROMETER_LERP_SPEED,
                );

                let (_, new_twist) = swing_twist_decomposition(&tag_isometry.rotation, &down_axis);
                let (old_swing, _) = swing_twist_decomposition(&isometry.rotation, &down_axis);
                let new_rotation = old_swing * new_twist;
                if new_rotation.w.is_finite()
                    && new_rotation.i.is_finite()
                    && new_rotation.j.is_finite()
                    && new_rotation.k.is_finite()
                {
                    let dot_product = isometry.rotation.coords.dot(&new_rotation.coords);
                    
                    let target_quat = if dot_product < 0.0 {
                        UnitQuaternion::new_normalize(-new_rotation.into_inner())
                    } else {
                        new_rotation
                    };
                    
                    // Use lerp for the quaternion interpolation with proper direction
                    isometry.rotation = UnitQuaternion::new_normalize(lerp(
                        isometry.rotation.into_inner(),
                        target_quat.into_inner(),
                        LOCALIZATION_DELTA,
                        ACCELEROMETER_LERP_SPEED,
                    ));
                }
            } else if let Some(angular_velocity) = angular_velocity {
                isometry.append_rotation_wrt_center_mut(&UnitQuaternion::from_axis_angle(
                    &down_axis,
                    -angular_velocity.y * LOCALIZATION_DELTA,
                ));
            }

            self.root_node.set_isometry(isometry);
            #[cfg(feature = "production")]
            {
                isometry_sync_timer -= LOCALIZATION_DELTA;
                if isometry_sync_timer <= 0.0 {
                    isometry_sync_timer = self.isometry_sync_delta;
                    let data = bitcode::encode(&FromLunabot::RobotIsometry {
                        origin: isometry.translation.vector.cast().data.0[0],
                        quat: isometry.rotation.as_vector().cast().data.0[0],
                    });
                    let packet = self
                        .packet_builder
                        .new_unreliable(PacketBody { data })
                        .unwrap();
                    self.packet_builder
                        .send_packet(cakap2::packet::Action::SendUnreliable(packet));
                    // if let Some((lift_hinge_node, bucket_node)) = lift_hinge_node {
                    let data = bitcode::encode(&FromLunabot::ArmAngles { hinge: lift_hinge_node.get_local_angle_one_axis().unwrap() as f32, bucket: bucket_node.get_local_angle_one_axis().unwrap() as f32 });
                    let packet = self
                        .packet_builder
                        .new_unreliable(PacketBody { data })
                        .unwrap();
                    self.packet_builder
                        .send_packet(cakap2::packet::Action::SendUnreliable(packet));
                    // }
                }

                crate::apps::RECORDER.get().map(|recorder| {
                    if let Err(e) = recorder.recorder.log(
                        crate::apps::ROBOT_STRUCTURE,
                        &rerun::Transform3D::from_translation_rotation(
                            isometry.translation.vector.cast::<f32>().data.0[0],
                            rerun::Quaternion::from_xyzw(
                                isometry.rotation.as_vector().cast::<f32>().data.0[0],
                            ),
                        ),
                    ) {
                        error!("Failed to log robot transform: {e}");
                    }
                });
            }

            #[cfg(not(feature = "production"))]
            if let Some(lunasim_stdin) = &self.lunasim_stdin {
                let (axis, angle) = isometry
                    .rotation
                    .axis_angle()
                    .unwrap_or((UnitVector3::new_normalize(Vector3::new(0.0, 0.0, 1.0)), 0.0));
                let axis = [axis.x as f32, axis.y as f32, axis.z as f32];

                let origin = [
                    isometry.translation.x as f32,
                    isometry.translation.y as f32,
                    isometry.translation.z as f32,
                ];

                let bytes = bitcode_buffer.encode(&common::lunasim::FromLunasimbot::Isometry {
                    axis,
                    angle: angle as f32,
                    origin,
                });

                lunasim_stdin.write(bytes);
            }
        }
    }
}
