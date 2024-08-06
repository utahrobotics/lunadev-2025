use serde::Deserialize;
use urobotics::{app::Application, task::SyncTask};

use crate::simbot::{linear_maze::LinearMazeSensor, teleop::LinearMazeTeleop, Drive, REFRESH_RATE};

use super::DriveInstruction;


#[derive(Deserialize)]
pub struct LinearMazeTeleopSolution {}

impl Application for LinearMazeTeleopSolution {
    const APP_NAME: &'static str = "linear_maze_teleop_soln";
    const DESCRIPTION: &'static str = "A solution to the linear maze teleop problem";

    fn run(self) {
        let linear_maze = LinearMazeSensor::default();
        let teleop = LinearMazeTeleop::<(), _>::new(|_| true);
        let mut drive = Drive::default();

        let (distance_tx, distance_rx) = std::sync::mpsc::channel();
        let raycast_callback = teleop.raycast_callback();
        let mut first = true;
        linear_maze
            .raycast_callbacks_ref()
            .add_fn_mut(move |(_, distance)| {
                distance_tx.send(distance).unwrap();
                if first {
                    first = false;
                    raycast_callback(distance, ());
                }
            });

        let raycast_callback = teleop.raycast_callback();

        teleop.drive_callbacks_ref().add_fn_mut(move |instruction| {
            match instruction {
                DriveInstruction::Drive(distance) => drive.drive(distance),
                DriveInstruction::Turn(angle) => drive.set_direction(drive.get_direction() + angle),
            }
            while let Ok(_) = distance_rx.try_recv() {

            }
            std::thread::sleep(REFRESH_RATE);
            let Ok(distance) = distance_rx.recv() else { return; };
            raycast_callback(distance, ());
        });

        teleop.spawn();
        let _ = linear_maze.spawn().join();
    }
}
