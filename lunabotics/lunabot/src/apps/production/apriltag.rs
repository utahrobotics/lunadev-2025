use std::f64::consts::PI;

use k::{UnitQuaternion, Vector3};
use nalgebra::Point3;
use serde::Deserialize;

#[derive(Deserialize, Clone, Copy)]
pub struct Apriltag {
    pub tag_position: Point3<f64>,
    forward_axis: Vector3<f64>,
    #[serde(default)]
    roll: f64,
    pub tag_width: f64,
}

impl Apriltag {
    pub fn get_quat(self) -> UnitQuaternion<f64> {
        // First rotation to face along the forward axis
        let rotation1 =
            UnitQuaternion::rotation_between(&Vector3::new(0.0, 0.0, -1.0), &self.forward_axis)
                .unwrap_or(UnitQuaternion::from_scaled_axis(Vector3::new(0.0, PI, 0.0)));

        let cross_axis = self.forward_axis.cross(&Vector3::new(0.0, 1.0, 0.0));
        let true_up = cross_axis.cross(&self.forward_axis);

        // Second rotation to rotate the up axis to face directly up
        let actual_up = rotation1 * Vector3::new(0.0, 1.0, 0.0);
        let rotation2 = UnitQuaternion::rotation_between(&actual_up, &true_up).unwrap();

        // Third rotation to roll the tag
        let rotation3 = UnitQuaternion::from_scaled_axis(self.forward_axis.normalize() * self.roll);

        rotation3 * rotation2 * rotation1
    }
}
