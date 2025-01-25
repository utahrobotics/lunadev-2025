use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use crossbeam::atomic::AtomicCell;
use nalgebra::{Isometry3, UnitQuaternion, UnitVector3, Vector3};
use simple_motion::StaticNode;
use spin_sleep::SpinSleeper;
use tracing::error;

#[cfg(not(feature = "production"))]
use crate::apps::LunasimStdin;
use crate::
    utils::{lerp_value, swing_twist_decomposition}
;

const ACCELEROMETER_LERP_SPEED: f64 = 150.0;
const LOCALIZATION_DELTA: f64 = 1.0 / 60.0;
/// The threshold of speed in m/s for the robot to be considered in motion.
const IN_MOTION_THRESHOLD: f64 = 0.1;
const IN_MOTION_DURATION: f64 = 0.5;

#[derive(Default)]
struct LocalizerRefInner {
    acceleration: AtomicCell<Vector3<f64>>,
    angular_velocity: AtomicCell<UnitQuaternion<f64>>,
    april_tag_isometry: AtomicCell<Option<Isometry3<f64>>>,
    in_motion: AtomicBool,
}

#[derive(Clone)]
pub struct LocalizerRef {
    inner: Arc<LocalizerRefInner>,
}

impl LocalizerRef {
    pub fn set_acceleration(&self, acceleration: Vector3<f64>) {
        self.inner.acceleration.store(acceleration);
    }

    pub fn set_april_tag_isometry(&self, isometry: Isometry3<f64>) {
        self.inner.april_tag_isometry.store(Some(isometry));
    }

    pub fn set_angular_velocity(&self, angular_velocity: UnitQuaternion<f64>) {
        self.inner.angular_velocity.store(angular_velocity);
    }

    fn acceleration(&self) -> Vector3<f64> {
        self.inner.acceleration.load()
    }

    fn april_tag_isometry(&self) -> Option<Isometry3<f64>> {
        self.inner.april_tag_isometry.take()
    }

    fn angular_velocity(&self) -> UnitQuaternion<f64> {
        self.inner.angular_velocity.load()
    }

    // pub fn is_in_motion(&self) -> bool {
    //     self.inner.in_motion.load(Ordering::Relaxed)
    // }
}

pub struct Localizer {
    root_node: StaticNode,
    #[cfg(not(feature = "production"))]
    lunasim_stdin: Option<LunasimStdin>,
    localizer_ref: LocalizerRef,
}

impl Localizer {
    #[cfg(not(feature = "production"))]
    pub fn new(root_node: StaticNode, lunasim_stdin: Option<LunasimStdin>) -> Self {
        Self {
            root_node,
            lunasim_stdin,
            localizer_ref: LocalizerRef {
                inner: Default::default(),
            },
        }
    }
    #[cfg(feature = "production")]
    pub fn new(root_node: StaticNode) -> Self {
        Self {
            root_node,
            localizer_ref: LocalizerRef {
                inner: Default::default(),
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
        let mut is_in_motion = false;
        let mut is_in_motion_timer = 0.0;

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
                self.localizer_ref
                    .inner
                    .in_motion
                    .store(false, Ordering::Relaxed);
                self.root_node.set_isometry(Isometry3::identity());
            }

            let mut down_axis = UnitVector3::new_unchecked(Vector3::new(0.0, -1.0, 0.0));
            let acceleration =
                UnitVector3::new_normalize(isometry * self.localizer_ref.acceleration());
            if !acceleration.x.is_finite()
                || !acceleration.y.is_finite()
                || !acceleration.z.is_finite()
            {
                continue;
            }
            let angle = down_axis.angle(&acceleration)
                * lerp_value(LOCALIZATION_DELTA, ACCELEROMETER_LERP_SPEED);

            if angle > 0.001 {
                let cross = UnitVector3::new_normalize(down_axis.cross(&acceleration));
                isometry.append_rotation_wrt_center_mut(&UnitQuaternion::from_axis_angle(
                    &cross, -angle,
                ));
            }

            down_axis = isometry.rotation * down_axis;

            if let Some(tag_isometry) = self.localizer_ref.april_tag_isometry() {
                isometry.translation = tag_isometry.translation;

                let (_, new_twist) = swing_twist_decomposition(&tag_isometry.rotation, &down_axis);
                let (old_swing, _) = swing_twist_decomposition(&isometry.rotation, &down_axis);
                isometry.rotation = old_swing * new_twist;
            } else {
                let (_, twist) =
                    swing_twist_decomposition(&self.localizer_ref.angular_velocity(), &down_axis);
                isometry.append_rotation_wrt_center_mut(
                    &UnitQuaternion::default()
                        .try_slerp(&twist, LOCALIZATION_DELTA, 0.001)
                        .unwrap_or_default(),
                );
            }

            let currently_in_motion = (isometry.translation.vector
                - self.root_node.get_global_isometry().translation.vector)
                .magnitude()
                / LOCALIZATION_DELTA
                > IN_MOTION_THRESHOLD;
            if is_in_motion {
                if currently_in_motion {
                    is_in_motion_timer = IN_MOTION_DURATION;
                } else {
                    is_in_motion_timer -= LOCALIZATION_DELTA;
                    if is_in_motion_timer <= 0.0 {
                        is_in_motion = false;
                        self.localizer_ref
                            .inner
                            .in_motion
                            .store(false, Ordering::Relaxed);
                    }
                }
            } else if currently_in_motion {
                is_in_motion = true;
                is_in_motion_timer = IN_MOTION_DURATION;
                self.localizer_ref
                    .inner
                    .in_motion
                    .store(true, Ordering::Relaxed);
            }

            self.root_node.set_isometry(isometry);
            // let axis = isometry.rotation.axis().map(|x| x.into_inner()).unwrap_or_default();
            // println!(
            //     "pos: [{:.2}, {:.2}, {:.2}] angle: {}deg axis: [{:.2}, {:.2}, {:.2}]",
            //     isometry.translation.x,
            //     isometry.translation.y,
            //     isometry.translation.z,
            //     (isometry.rotation.angle() / std::f64::consts::PI * 180.0).round() as i32,
            //     axis.x,
            //     axis.y,
            //     axis.z,
            // );

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
