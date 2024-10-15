mod production;
mod sim;

use std::{fs::File, net::SocketAddr, sync::Arc};

use common::{FromLunabase, FromLunabot, LunabotStage};
use crossbeam::atomic::AtomicCell;
use k::Chain;
use nalgebra::Vector4;
pub use production::LunabotApp;
pub use sim::{LunasimStdin, LunasimbotApp};
use urobotics::{
    define_callbacks, fn_alias,
    log::{error, warn},
    tokio, BlockOn,
};

use crate::teleop::{LunabaseConn, PacketBuilder};

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

fn_alias! {
    pub type PointCloudCallbacksRef = CallbacksRef(&[Vector4<f32>]) + Send + Sync
}
define_callbacks!(PointCloudCallbacks => Fn(point_cloud: &[Vector4<f32>]) + Send + Sync);

fn create_packet_builder(
    lunabase_address: SocketAddr,
    lunabot_stage: Arc<AtomicCell<LunabotStage>>,
) -> (PacketBuilder, std::sync::mpsc::Receiver<FromLunabase>) {
    let (from_lunabase_tx, from_lunabase_rx) = std::sync::mpsc::channel();
    let mut bitcode_buffer = bitcode::Buffer::new();

    let packet_builder = LunabaseConn {
        lunabase_address,
        on_msg: move |bytes: &[u8]| match bitcode_buffer.decode(bytes) {
            Ok(msg) => {
                let _ = from_lunabase_tx.send(msg);
                true
            }
            Err(e) => {
                error!("Failed to decode from lunabase: {e}");
                false
            }
        },
        lunabot_stage,
    }
    .connect_to_lunabase();

    (packet_builder, from_lunabase_rx)
}
