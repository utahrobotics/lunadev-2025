use std::path::Path;

use urobotics::{
    app::{adhoc_app, application},
    camera,
    log::OwoColorize,
    python, serial,
};

fn delete_cabinet() {
    if let Err(e) = std::fs::remove_dir_all("../../") {
        eprintln!(
            "{}",
            format!("Failed to delete cabinet folder: {}", e).red()
        );
    } else {
        eprintln!("{}", "Deleted cabinet folder".green());
    }
}

adhoc_app!(
    DeleteCabinet(delete_cabinet):
    "Delete cabinet folder"
);

fn main() {
    let mut app = application!();
    if Path::new("urobotics-venv").exists() {
        app.cabinet_builder.create_symlink_for("urobotics-venv");
    }
    app.add_app::<serial::app::Serial>()
        .add_app::<python::app::Python>()
        .add_app::<camera::app::Camera>()
        .add_app::<DeleteCabinet>()
        // .add_app::<urobotics_learning::multiples_of_two::solution::MultiplesOfTwoSolution>()
        // .add_app::<urobotics_learning::simbot::linear_maze::solution::LinearMazeSolution>()
        // .add_app::<urobotics_learning::simbot::teleop::solution::LinearMazeTeleopSolution>()
        .run();
}
