use std::{
    net::{SocketAddr, SocketAddrV4},
    sync::Arc,
};

use byteable::IntoBytes;
use cakap::{CakapSender, CakapSocket};
use common::{FromLunabase, FromLunabot, LunabotStage};
use crossbeam::atomic::AtomicCell;
use lunabot_ai::TeleOpComponent;
use urobotics::{
    callbacks::caller::try_drop_this_callback,
    get_tokio_handle,
    log::{error, info},
    tokio::{self, sync::mpsc},
    BlockOn,
};

const PING_DELAY: f64 = 1.0;

pub struct LunabaseConn {
    stage: Arc<AtomicCell<LunabotStage>>,
    sender: CakapSender,
    receiver: mpsc::UnboundedReceiver<FromLunabase>,
}

impl LunabaseConn {
    pub fn new(lunabase_address: SocketAddrV4) -> std::io::Result<Self> {
        let socket = CakapSocket::bind(0).block_on()?;
        let sender = socket.get_stream();
        sender.set_send_addr(SocketAddr::V4(lunabase_address));
        match socket.local_addr() {
            Ok(addr) => info!("Bound to {addr}"),
            Err(e) => error!("Failed to get local address: {e}"),
        }
        let (from_lunabase_tx, receiver) = mpsc::unbounded_channel();
        socket
            .get_bytes_callback_ref()
            .add_dyn_fn(Box::new(move |bytes| {
                let msg: FromLunabase = match TryFrom::try_from(bytes) {
                    Ok(x) => x,
                    Err(e) => {
                        error!("Failed to parse message from lunabase: {e}");
                        return;
                    }
                };
                if from_lunabase_tx.send(msg).is_err() {
                    try_drop_this_callback();
                }
            }));
        socket.spawn_looping();

        let sender2 = sender.clone();
        let stage = Arc::new(AtomicCell::new(LunabotStage::SoftStop));
        let stage2 = stage.clone();
        get_tokio_handle().spawn(async move {
            let ping_bytes = FromLunabot::Ping(stage2.load())
                .to_bytes_vec()
                .into_boxed_slice();
            loop {
                sender2.send_unreliable(&ping_bytes).await;
                tokio::time::sleep(std::time::Duration::from_secs_f64(PING_DELAY)).await;
            }
        });

        Ok(Self {
            sender,
            receiver,
            stage,
        })
    }
}

impl TeleOpComponent for LunabaseConn {
    async fn from_lunabase(&mut self) -> FromLunabase {
        self.receiver.recv().await.expect("Cakap thread closed")
    }

    async fn to_lunabase_unreliable(&mut self, to_lunabase: FromLunabot) {
        self.sender.send_unreliable(&to_lunabase.to_bytes()).await;
    }

    async fn to_lunabase_reliable(&mut self, to_lunabase: FromLunabot) {
        self.sender.send_reliable(to_lunabase.to_bytes_vec());
    }

    fn set_lunabot_stage(&mut self, stage: LunabotStage) {
        self.stage.store(stage);
    }
}
