#![feature(
    result_flattening,
    array_chunks,
    iterator_try_collect,
    mpmc_channel,
    try_blocks,
    f16
)]

use std::net::IpAddr;

use apps::default_max_pong_delay_ms;
use lumpur::LumpurBuilder;
use tracing::Level;

mod apps;
mod localization;
mod pathfinding;
mod pipelines;
mod teleop;
mod utils;

#[cfg(feature = "production")]
lumpur::define_configuration! {
    pub enum Commands {
        Main {
            lunabase_address: Option<IpAddr>,
            max_pong_delay_ms: Option<u64>,
            #[serde(default)]
            cameras: fxhash::FxHashMap<String, apps::CameraInfo>,
            #[serde(default)]
            depth_cameras: fxhash::FxHashMap<String, apps::DepthCameraInfo>,
            #[serde(default)]
            apriltags: fxhash::FxHashMap<String, apps::Apriltag>,
            robot_layout: Option<String>,
            #[serde(default)]
            vesc: apps::Vesc,
            #[serde(default)]
            rerun_viz: apps::RerunViz,
            imu_correction: Option<imu_calib::CalibrationParameters>,
            v3pico: apps::V3PicoInfo,
            #[serde(default)]
            new_ai: bool
        },
    }
}
#[cfg(not(feature = "production"))]
lumpur::define_configuration! {
    pub enum Commands {
        Main {},
        Sim {
            lunabase_address: Option<IpAddr>,
            max_pong_delay_ms: Option<u64>,
            #[serde(default)]
            new_ai: bool
        }
    }
}

fn main() {
    let cmd: Commands = LumpurBuilder::default()
        .symlink_path("godot")
        .symlink_path("target")
        .symlink_path("robot-layout")
        .symlink_path("3d-models")
        .set_total_ignores([
            ("wgpu_core.*", Level::INFO),
            ("wgpu_hal.*", Level::INFO),
            ("yaserde.*", Level::INFO),
            ("mio.*", Level::INFO),
            ("naga.*", Level::INFO),
            ("re_sdk.*", Level::INFO),
            ("re_chunk.*", Level::INFO),
        ])
        .set_console_ignores([
            ("wgpu_hal::gles::egl", Level::WARN),
            ("wgpu_hal::vulkan::instance", Level::WARN),
        ])
        .init();

    match cmd {
        #[cfg(not(feature = "production"))]
        Commands::Sim {
            lunabase_address,
            max_pong_delay_ms,
            new_ai
        } => {
            apps::LunasimbotApp {
                lunabase_address,
                max_pong_delay_ms: max_pong_delay_ms.unwrap_or_else(default_max_pong_delay_ms),
                new_ai
            }
            .run();
        }
        #[cfg(not(feature = "production"))]
        Commands::Main { .. } => {
            tracing::error!("Production mode is not enabled");
        }
        #[cfg(feature = "production")]
        Commands::Main {
            lunabase_address,
            max_pong_delay_ms,
            cameras,
            depth_cameras,
            apriltags,
            robot_layout,
            vesc,
            rerun_viz,
            imu_correction,
            v3pico,
            new_ai
        } => {
            apps::LunabotApp {
                lunabase_address,
                max_pong_delay_ms: max_pong_delay_ms.unwrap_or_else(default_max_pong_delay_ms),
                cameras,
                depth_cameras,
                apriltags,
                vesc,
                robot_layout: robot_layout
                    .unwrap_or_else(|| "robot-layout/lunabot.json".to_string()),
                rerun_viz,
                imu_correction,
                v3pico,
                new_ai
            }
            .run();
        }
    }
}
