use std::f64::consts::{FRAC_PI_2, PI};

use serde::Deserialize;
use urobotics::{app::Application, task::SyncTask};

use crate::simbot::Drive;

use super::LinearMazeSensor;


#[derive(Deserialize)]
pub struct LinearMazeSolution {}

impl Application for LinearMazeSolution {
    const APP_NAME: &'static str = "linear_maze_soln";
    const DESCRIPTION: &'static str = "A solution to the linear maze problem";

    fn run(self) {
        let linear_maze = LinearMazeSensor::default();
        let mut drive = Drive::default();
        let mut turned_left = false;
        linear_maze.raycast_callbacks_ref().add_fn_mut(move |(_, distance)| {
            if (0.5 - distance).abs() < 0.01 {
                if turned_left {
                    turned_left = false;
                    drive.set_direction(drive.get_direction() - PI)
                } else {
                    turned_left = true;
                    drive.set_direction(drive.get_direction() + FRAC_PI_2)
                }
            } else {
                turned_left = false;
                drive.drive(distance - 0.5);
            }
        });

        let _ = linear_maze.spawn().join();
    }
}
