use std::{sync::Arc, time::Duration};

use common::lunasim::FromLunasimbot;
use crossbeam::atomic::AtomicCell;
use k::{Chain, UnitQuaternion, Vector3};
use nalgebra::UnitVector3;
use spin_sleep::SpinSleeper;
use urobotics::task::SyncTask;

use crate::{utils::lerp_value, LunasimStdin};

const ACCELEROMETER_LERP_SPEED: f64 = 150.0;
const LOCALIZATION_DELTA: f64 = 1.0 / 60.0;

pub struct Localizer {
    pub robot_chain: Arc<Chain<f64>>,
    pub lunasim_stdin: Option<LunasimStdin>,
    // pub lunabase_sender: CakapSender,
    pub acceleration: Arc<AtomicCell<Vector3<f64>>>,
}


impl SyncTask for Localizer {
    type Output = !;

    fn run(self) -> Self::Output {
        let spin_sleeper = SpinSleeper::default();

        loop {
            spin_sleeper.sleep(Duration::from_secs_f64(LOCALIZATION_DELTA));
            let mut isometry = self.robot_chain.origin();
            let down_axis = isometry * Vector3::new(0.0, -1.0, 0.0);
            let acceleration = self.acceleration.load();
            let angle = down_axis.angle(&acceleration) * lerp_value(LOCALIZATION_DELTA, ACCELEROMETER_LERP_SPEED);

            if angle < 0.001 {
                continue;
            }

            let cross = UnitVector3::new_normalize(down_axis.cross(&acceleration));
            isometry.append_rotation_wrt_center_mut(&UnitQuaternion::from_axis_angle(&cross, angle));
            self.robot_chain.set_origin(isometry);

            if let Some(lunasim_stdin) = &self.lunasim_stdin {
                let quat = [
                    isometry.rotation.i as f32,
                    isometry.rotation.j as f32,
                    isometry.rotation.k as f32,
                    isometry.rotation.w as f32,
                ];

                let origin = [
                    isometry.translation.x as f32,
                    isometry.translation.y as f32,
                    isometry.translation.z as f32,
                ];
                
                FromLunasimbot::Isometry { quat, origin }.encode(|bytes| {
                    lunasim_stdin.write(bytes);
                });
            }
        }
    }
}