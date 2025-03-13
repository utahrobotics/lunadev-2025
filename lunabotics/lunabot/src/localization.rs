use std::{sync::Arc, time::Duration};

use crossbeam::atomic::AtomicCell;
use nalgebra::{Isometry3, UnitQuaternion, UnitVector3, Vector3};
use simple_motion::StaticNode;
use spin_sleep::SpinSleeper;
use tracing::error;

#[cfg(not(feature = "production"))]
use crate::apps::LunasimStdin;
use crate::utils::{lerp_value, swing_twist_decomposition};

const ACCELEROMETER_LERP_SPEED: f64 = 150.0;
const LOCALIZATION_DELTA: f64 = 1.0 / 60.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct IMUReading {
    pub angular_velocity: Vector3<f64>,
    pub acceleration: Vector3<f64>,
}

#[derive(Default)]
struct LocalizerRefInner {
    april_tag_isometry: AtomicCell<Option<Isometry3<f64>>>,
    imu_readings: Box<[AtomicCell<Option<IMUReading>>]>,
}

#[derive(Clone)]
pub struct LocalizerRef {
    inner: Arc<LocalizerRefInner>,
}

impl LocalizerRef {
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

    fn take_imu_readings(&self) -> IMUReading {
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
        }
        out
    }

    fn april_tag_isometry(&self) -> Option<Isometry3<f64>> {
        self.inner.april_tag_isometry.load()
    }
}

pub struct Localizer {
    root_node: StaticNode,
    #[cfg(not(feature = "production"))]
    lunasim_stdin: Option<LunasimStdin>,
    localizer_ref: LocalizerRef,
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
    pub fn new(root_node: StaticNode, imu_count: usize) -> Self {
        Self {
            root_node,
            localizer_ref: LocalizerRef {
                inner: Arc::new(LocalizerRefInner {
                    imu_readings: (0..imu_count).map(|_| AtomicCell::new(None)).collect(),
                    ..Default::default()
                }),
            },
        }
    }

    pub fn get_ref(&self) -> LocalizerRef {
        self.localizer_ref.clone()
    }

    pub fn run(self) {
        let spin_sleeper = SpinSleeper::default();
        #[cfg(not(feature = "production"))]
        let mut bitcode_buffer = bitcode::Buffer::new();
        let mut last_acceleration = - Vector3::y_axis();
        let mut last_angular_velocity = Vector3::zeros();

        loop {
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

            let IMUReading {
                acceleration,
                angular_velocity,
            } = self.localizer_ref.take_imu_readings();
            let mut down_axis = - Vector3::y_axis();
            let acceleration = UnitVector3::new_normalize(isometry * acceleration);
            if acceleration.x.is_finite()
                && acceleration.y.is_finite()
                && acceleration.z.is_finite()
            {
                last_acceleration = acceleration;
            }
            if angular_velocity.x.is_finite()
                && angular_velocity.y.is_finite()
                && angular_velocity.z.is_finite()
            {
                last_angular_velocity = angular_velocity;
            }
            // tracing::info!("{:.2} {:.2} {:.2}", last_acceleration.x, last_acceleration.y, last_acceleration.z);
            let angle = down_axis.angle(&last_acceleration)
                * lerp_value(LOCALIZATION_DELTA, ACCELEROMETER_LERP_SPEED);

            if angle > 1.0f64.to_radians() {
                let cross = UnitVector3::new_normalize(down_axis.cross(&last_acceleration));
                isometry.append_rotation_wrt_center_mut(&UnitQuaternion::from_axis_angle(
                    &cross, -angle,
                ));
            }

            down_axis = isometry.rotation * down_axis;

            if let Some(tag_isometry) = self.localizer_ref.april_tag_isometry() {
                tracing::info!("Tag");
                isometry.translation = tag_isometry.translation;

                let (_, new_twist) = swing_twist_decomposition(&tag_isometry.rotation, &down_axis);
                let (old_swing, _) = swing_twist_decomposition(&isometry.rotation, &down_axis);
                let new_rotation = old_swing * new_twist;
                if new_rotation.w.is_finite()
                    && new_rotation.i.is_finite()
                    && new_rotation.j.is_finite()
                    && new_rotation.k.is_finite()
                {
                    isometry.rotation = new_rotation;
                }
            } else {
                isometry.append_rotation_wrt_center_mut(&UnitQuaternion::from_axis_angle(
                    &down_axis,
                    -last_angular_velocity.y * LOCALIZATION_DELTA,
                ));
            }

            self.root_node.set_isometry(isometry);
            #[cfg(feature = "production")]
            {
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
