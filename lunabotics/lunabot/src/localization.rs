use std::{sync::Arc, time::Duration};

use byteable::IntoBytesSlice;
use common::lunasim::FromLunasimbot;
use crossbeam::atomic::AtomicCell;
use k::{Chain, Isometry3, UnitQuaternion, Vector3};
use nalgebra::UnitVector3;
use spin_sleep::SpinSleeper;
use urobotics::{log::error, task::SyncTask};

use crate::{
    sim::LunasimStdin,
    utils::{lerp_value, swing_twist_decomposition},
};

const ACCELEROMETER_LERP_SPEED: f64 = 150.0;
const LOCALIZATION_DELTA: f64 = 1.0 / 60.0;

#[derive(Default)]
struct LocalizerRefInner {
    acceleration: AtomicCell<Vector3<f64>>,
    angular_velocity: AtomicCell<UnitQuaternion<f64>>,
    april_tag_isometry: AtomicCell<Option<Isometry3<f64>>>,
}

#[derive(Default, Clone)]
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
}

pub struct Localizer {
    pub robot_chain: Arc<Chain<f64>>,
    pub lunasim_stdin: Option<LunasimStdin>,
    // pub lunabase_sender: CakapSender,
    pub localizer_ref: LocalizerRef,
}

impl SyncTask for Localizer {
    type Output = !;

    fn run(self) -> Self::Output {
        let spin_sleeper = SpinSleeper::default();

        loop {
            spin_sleeper.sleep(Duration::from_secs_f64(LOCALIZATION_DELTA));
            let mut isometry = self.robot_chain.origin();

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
                self.robot_chain.set_origin(Isometry3::identity());
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

            self.robot_chain.set_origin(isometry);
            self.robot_chain.update_transforms();

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

                FromLunasimbot::Isometry {
                    axis,
                    angle: angle as f32,
                    origin,
                }
                .into_bytes_slice(|bytes| {
                    lunasim_stdin.write(bytes);
                });
            }
        }
    }
}
