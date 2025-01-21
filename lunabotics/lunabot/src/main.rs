#![feature(result_flattening, array_chunks, iterator_try_collect)]

use std::net::SocketAddr;

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
            lunabase_address: SocketAddr,
            max_pong_delay_ms: Option<u64>,
            lunabase_streaming_address: Option<SocketAddr>,
            lunabase_audio_streaming_address: Option<SocketAddr>,
            #[serde(default)]
            cameras: fxhash::FxHashMap<String, apps::CameraInfo>,
            #[serde(default)]
            depth_cameras: fxhash::FxHashMap<String, apps::DepthCameraInfo>,
            #[serde(default)]
            apriltags: fxhash::FxHashMap<String, apps::Apriltag>,
            robot_layout: Option<String>
        },
        Dataviz {
            lunabase_address: SocketAddr,
            max_pong_delay_ms: Option<u64>,
            lunabase_data_address: Option<SocketAddr>,
            #[serde(default)]
            depth_cameras: fxhash::FxHashMap<String, apps::DepthCameraInfo>,
            robot_layout: Option<String>
        },
        Sim {
            lunabase_address: SocketAddr,
            max_pong_delay_ms: Option<u64>
        }
    }
}
#[cfg(not(feature = "production"))]
lumpur::define_configuration! {
    pub enum Commands {
        Main {},
        Sim {
            lunabase_address: SocketAddr,
            max_pong_delay_ms: Option<u64>
        }
    }
}

fn main() {
    let cmd: Commands = LumpurBuilder::default()
        .symlink_path("godot")
        .symlink_path("target")
        .symlink_path("robot-layout")
        .set_total_ignores([
            ("wgpu_core.*", Level::INFO),
            ("wgpu_hal.*", Level::INFO),
            ("yaserde.*", Level::INFO),
            ("mio.*", Level::INFO),
            ("naga.*", Level::INFO),
        ])
        .set_console_ignores([
            ("k::urdf", Level::INFO),
            ("wgpu_hal::gles::egl", Level::WARN),
            ("wgpu_hal::vulkan::instance", Level::WARN),
        ])
        .init();

    match cmd {
        Commands::Sim {
            lunabase_address,
            max_pong_delay_ms,
        } => {
            apps::LunasimbotApp {
                lunabase_address,
                max_pong_delay_ms: max_pong_delay_ms.unwrap_or_else(default_max_pong_delay_ms),
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
            lunabase_streaming_address,
            lunabase_audio_streaming_address,
            cameras,
            depth_cameras,
            apriltags,
            robot_layout,
        } => {
            apps::LunabotApp {
                lunabase_address,
                lunabase_streaming_address,
                max_pong_delay_ms: max_pong_delay_ms.unwrap_or_else(default_max_pong_delay_ms),
                #[cfg(feature = "experimental")]
                lunabase_audio_streaming_address,
                cameras,
                depth_cameras,
                apriltags,
                robot_layout: robot_layout
                    .unwrap_or_else(|| "robot-layout/lunabot.json".to_string()),
            }
            .run();
            #[cfg(not(feature = "experimental"))]
            let _ = lunabase_audio_streaming_address;
        }
        #[cfg(feature = "production")]
        Commands::Dataviz {
            lunabase_address,
            lunabase_data_address,
            max_pong_delay_ms,
            depth_cameras,
            robot_layout,
        } => {
            apps::dataviz::DatavizApp {
                lunabase_address,
                lunabase_data_address,
                max_pong_delay_ms: max_pong_delay_ms.unwrap_or_else(default_max_pong_delay_ms),
                depth_cameras,
                robot_layout: robot_layout
                    .unwrap_or_else(|| "robot-layout/lunabot.json".to_string()),
            }
            .run();
        }
    }
}
