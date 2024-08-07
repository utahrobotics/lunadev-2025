use std::path::Path;

use urobotics::{app::application, camera, python, serial};

fn main() {
    let mut app = application!();
    if Path::new("urobotics-venv").exists() {
        app.cabinet_builder.create_symlink_for("urobotics-venv");
    }
    app.add_app::<serial::SerialConnection>()
        .add_app::<python::PythonVenvBuilder>()
        .add_app::<camera::CameraConnection>()
        .add_app::<urobotics_learning::multiples_of_two::solution::MultiplesOfTwoSolution>()
        .add_app::<urobotics_learning::simbot::linear_maze::solution::LinearMazeSolution>()
        .add_app::<urobotics_learning::simbot::teleop::solution::LinearMazeTeleopSolution>()
        .run();
}
