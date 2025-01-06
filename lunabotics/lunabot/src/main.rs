#![feature(result_flattening, array_chunks, iterator_try_collect)]

use fxhash::FxHashMap;
use std::net::SocketAddr;

use apps::{default_max_pong_delay_ms, Apriltag, LunabotApp, LunasimbotApp};
use lumpur::LumpurBuilder;
use tracing::Level;

mod apps;
mod localization;
mod motors;
// mod obstacles;
mod pathfinding;
mod pipelines;
mod teleop;
mod utils;

lumpur::define_configuration! {
    pub enum Commands {
        Main {
            lunabase_address: SocketAddr,
            max_pong_delay_ms: Option<u64>,
            lunabase_streaming_address: Option<SocketAddr>,
            cameras: FxHashMap<String, apps::CameraInfo>,
            depth_cameras: FxHashMap<String, apps::DepthCameraInfo>,
            apriltags: FxHashMap<String, Apriltag>
        },
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
        .add_ignore("k::urdf", Level::INFO, false)
        .add_ignore("wgpu_core::device::resource", Level::INFO, false)
        .add_ignore("wgpu_core::device::life", Level::INFO, false)
        .add_ignore("wgpu_core::device::global", Level::INFO, false)
        .add_ignore("wgpu_hal::gles::adapter", Level::INFO, false)
        .add_ignore("wgpu_hal::vulkan::instance", Level::INFO, false)
        .add_ignore("wgpu_hal::gles::egl", Level::INFO, false)
        .add_ignore("wgpu_core::storage", Level::INFO, false)
        .add_ignore("yaserde_derive", Level::INFO, false)
        .add_ignore("yaserde::de", Level::INFO, false)
        .add_ignore("wgpu_core::instance", Level::INFO, false)
        .init();

    match cmd {
        Commands::Sim {
            lunabase_address,
            max_pong_delay_ms,
        } => {
            LunasimbotApp {
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
            cameras,
            depth_cameras,
            apriltags,
        } => {
            LunabotApp {
                lunabase_address,
                lunabase_streaming_address,
                max_pong_delay_ms: max_pong_delay_ms.unwrap_or_else(default_max_pong_delay_ms),
                cameras,
                depth_cameras,
                apriltags,
            }
            .run();
        }
    }
}
