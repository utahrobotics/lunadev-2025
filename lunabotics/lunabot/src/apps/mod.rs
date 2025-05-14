#[cfg(feature = "production")]
mod production;
#[cfg(not(feature = "production"))]
mod sim;

use std::{fs::File, net::SocketAddr, sync::Arc, time::Duration};

use common::{FromLunabase, FromLunabot, LunabotStage};
use crossbeam::atomic::AtomicCell;
#[cfg(feature = "production")]
pub use production::{
    Apriltag, CameraInfo, DepthCameraInfo, LunabotApp, RerunViz, V3PicoInfo, Vesc, RECORDER, ROBOT,
    ROBOT_STRUCTURE,
};
#[cfg(not(feature = "production"))]
pub use sim::{LunasimStdin, LunasimbotApp};
use tasker::tokio::sync::{mpsc, watch};
use tracing::error;

use crate::teleop::{LunabaseConn, PacketBuilder};

pub fn default_max_pong_delay_ms() -> u64 {
    1500
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

#[derive(Clone)]
struct LunabotConnected {
    connected: watch::Receiver<bool>,
}

impl LunabotConnected {
    // fn is_connected(&self) -> bool {
    //     *self.connected.borrow()
    // }

    async fn wait_disconnect(&mut self) {
        let _ = self.connected.wait_for(|&x| !x).await;
    }
}

fn create_packet_builder(
    lunabase_address: Option<SocketAddr>,
    lunabot_stage: Arc<AtomicCell<LunabotStage>>,
    max_pong_delay_ms: u64,
) -> (
    PacketBuilder,
    mpsc::UnboundedReceiver<FromLunabase>,
    LunabotConnected,
) {
    let (from_lunabase_tx, from_lunabase_rx) = mpsc::unbounded_channel();
    let mut bitcode_buffer = bitcode::Buffer::new();
    let (pinged_tx, pinged_rx) = std::sync::mpsc::channel::<()>();

    let packet_builder = LunabaseConn {
        lunabase_address,
        on_msg: move |bytes: &[u8]| match bitcode_buffer.decode(bytes) {
            Ok(msg) => {
                if msg == FromLunabase::Pong {
                    let _ = pinged_tx.send(());
                } else {
                    let _ = from_lunabase_tx.send(msg);
                }
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

    let (connected_tx, connected_rx) = watch::channel(false);

    std::thread::spawn(move || loop {
        match pinged_rx.recv_timeout(Duration::from_millis(max_pong_delay_ms)) {
            Ok(()) => {
                let _ = connected_tx.send(true);
            }
            Err(_) => {
                let _ = connected_tx.send(false);
            }
        }
    });

    let connected = LunabotConnected {
        connected: connected_rx,
    };

    (packet_builder, from_lunabase_rx, connected)
}
