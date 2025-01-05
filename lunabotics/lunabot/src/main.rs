#![feature(result_flattening, array_chunks, iterator_try_collect)]

use std::net::SocketAddr;

use apps::{default_max_pong_delay_ms, LunasimbotApp};
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
        Production {
            lunabase_address: SocketAddr,
            max_pong_delay_ms: Option<u64>
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
        .add_ignore("k::urdf", Level::INFO)
        .add_ignore("wgpu_core::device::resource", Level::INFO)
        .add_ignore("wgpu_hal::vulkan::instance", Level::INFO)
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
        Commands::Production { .. } => {
            tracing::error!("Production mode is not enabled");
        }
        #[cfg(feature = "production")]
        Commands::Production {
            lunabase_address,
            max_pong_delay_ms,
        } => {}
    }
}
