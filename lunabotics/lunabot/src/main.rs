#![feature(result_flattening, deadline_api, never_type)]

use std::{fs::File, net::SocketAddr, path::Path, sync::Arc};

use common::{FromLunabase, FromLunabot};
use k::Chain;
use nalgebra::Vector4;
use serde::{Deserialize, Serialize};
use sim::LunasimbotApp;
use urobotics::{
    app::{adhoc_app, application, Application},
    camera, define_callbacks, fn_alias,
    log::{error, warn},
    python, serial,
    tokio::{self},
    video::info::list_media_input,
    BlockOn,
};

// mod interfaces;
mod localization;
mod sim;
mod utils;
mod teleop;
mod obstacles;

fn_alias! {
    type PointCloudCallbacksRef = CallbacksRef(&[Vector4<f32>]) + Send + Sync
}
define_callbacks!(PointCloudCallbacks => Fn(point_cloud: &[Vector4<f32>]) + Send + Sync);

fn wait_for_ctrl_c() {
    match tokio::signal::ctrl_c().block_on() {
        Ok(()) => {
            warn!("Ctrl-C Received");
        }
        Err(e) => {
            error!("Failed to await ctrl_c: {e}");
            loop {
                std::thread::park();
            }
        }
    }
}

fn log_teleop_messages() {
    if let Err(e) = File::create("from_lunabase.txt")
        .map(|f| FromLunabase::write_code_sheet(f))
        .flatten()
    {
        error!("Failed to write code sheet for FromLunabase: {e}");
    }
    if let Err(e) = File::create("from_lunabot.txt")
        .map(|f| FromLunabot::write_code_sheet(f))
        .flatten()
    {
        error!("Failed to write code sheet for FromLunabot: {e}");
    }
}

fn create_robot_chain() -> Arc<Chain<f64>> {
    Arc::new(Chain::<f64>::from_urdf_file("urdf/lunabot.urdf").expect("Failed to load urdf"))
}

#[derive(Serialize, Deserialize)]
struct LunabotApp {
    lunabase_address: SocketAddr,
}

impl Application for LunabotApp {
    const APP_NAME: &'static str = "main";

    const DESCRIPTION: &'static str = "The lunabot application";

    fn run(self) {
        log_teleop_messages();
        todo!();
        // wait_for_ctrl_c();
    }
}

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

adhoc_app!(InfoApp, "info", "Print diagnostics", info_app);

fn main() {
    let mut app = application!();
    if Path::new("urobotics-venv").exists() {
        app.cabinet_builder.create_symlink_for("urobotics-venv");
    }
    app.cabinet_builder.create_symlink_for("godot");
    app.cabinet_builder.create_symlink_for("target");
    app.cabinet_builder.create_symlink_for("urdf");

    app.add_app::<serial::SerialConnection>()
        .add_app::<python::PythonVenvBuilder>()
        .add_app::<camera::CameraConnection>()
        .add_app::<LunabotApp>()
        .add_app::<InfoApp>()
        .add_app::<LunasimbotApp>()
        .run();
}
