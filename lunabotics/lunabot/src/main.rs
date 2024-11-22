#![feature(result_flattening, never_type, array_chunks, sync_unsafe_cell, iterator_try_collect)]

use std::path::Path;

use apps::Sim;
use urobotics::{
    app::{adhoc_app, application},
    python, serial,
    video::info::list_media_input,
    BlockOn,
};

mod apps;
mod localization;
mod motors;
// mod obstacles;
mod pipelines;
mod teleop;
mod utils;

fn info_app() {
    match list_media_input().block_on() {
        Ok(list) => {
            if list.is_empty() {
                println!("No media input found");
            } else {
                println!("Media inputs:");
                for info in list {
                    println!("\t{} ({})", info.name, info.media_type);
                }
            }
        }
        Err(e) => eprintln!("Failed to list media input: {e}"),
    }
    println!();
}

adhoc_app!(InfoApp(info_app): "Print diagnostics");

fn main() {
    let mut app = application!();
    if Path::new("urobotics-venv").exists() {
        app.cabinet_builder.create_symlink_for("urobotics-venv");
    }
    app.cabinet_builder.create_symlink_for("godot");
    app.cabinet_builder.create_symlink_for("target");
    app.cabinet_builder.create_symlink_for("urdf");

    app = app
        .add_app::<serial::app::Serial>()
        .add_app::<python::app::Python>()
        .add_app::<InfoApp>()
        .add_app::<Sim>();
    #[cfg(feature = "production")]
    {
        app = app.add_app::<apps::Main>();
    }
    app.run();
}
