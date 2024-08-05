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
        let drive = Drive::default();
        linear_maze.raycast_callbacks_ref().add_fn(move |metric| {
            // TODO
        });

        let _ = linear_maze.spawn().join();
    }
}
