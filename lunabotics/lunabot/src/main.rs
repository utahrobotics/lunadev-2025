#![feature(result_flattening, array_chunks, iterator_try_collect)]

use fxhash::FxHashMap;
use std::net::SocketAddr;

use apps::default_max_pong_delay_ms;
use lumpur::LumpurBuilder;
use tracing::Level;

mod apps;
mod localization;
mod motors;
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
            cameras: FxHashMap<String, apps::CameraInfo>,
            #[serde(default)]
            depth_cameras: FxHashMap<String, apps::DepthCameraInfo>,
            #[serde(default)]
            apriltags: FxHashMap<String, apps::Apriltag>
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
        .symlink_path("urdf")
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
            }
            .run();
            #[cfg(not(feature = "experimental"))]
            let _ = lunabase_audio_streaming_address;
        }
    }
}
