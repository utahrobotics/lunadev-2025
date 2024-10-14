use std::{net::{Ipv4Addr, SocketAddr, SocketAddrV4}, ops::Deref, sync::Arc, time::{Duration, Instant}};

use cakap2::{packet::Action, Event, PeerStateMachine, RecommendedAction};
use common::{FromLunabot, LunabotStage};
use crossbeam::atomic::AtomicCell;
use urobotics::{get_tokio_handle, log::{error, warn}, tokio::{self, net::UdpSocket, sync::mpsc}};

pub struct PacketBuilder {
    builder: cakap2::packet::PacketBuilder,
    packet_tx: mpsc::UnboundedSender<Action>,
}

impl Deref for PacketBuilder {
    type Target = cakap2::packet::PacketBuilder;

    fn deref(&self) -> &Self::Target {
        &self.builder
    }
}

impl PacketBuilder {
    pub fn send_packet(&self, packet: Action) {
        let _ = self.packet_tx.send(packet);
    }
}

pub struct LunabaseConn<F> {
    pub lunabase_address: SocketAddr,
    pub on_msg: F,
    pub lunabot_stage: Arc<AtomicCell<LunabotStage>>
}

impl<F: FnMut(&[u8]) -> bool + Send + 'static> LunabaseConn<F> {
    /// Connect to the lunabase and return a [`PacketBuilder`] to send packets to the lunabase.
    /// 
    /// The `on_msg` closure is called whenever a message is received from the lunabase, and must
    /// return `true` if the message was successfully parsed, and `false` otherwise.
    pub fn connect_to_lunabase(mut self) -> PacketBuilder {
        let (cakap_sm, first_action) = PeerStateMachine::new(Duration::from_millis(150), 1024);
        let packet_builder = cakap_sm.get_packet_builder();
        let (packet_tx, mut packet_rx) = mpsc::unbounded_channel();

        get_tokio_handle().spawn(async move {
            let mut cakap_sm = cakap_sm;
            let mut action = first_action;

            let udp = loop {
                let udp = match UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await {
                    Ok(x) => x,
                    Err(e) => {
                        error!("Failed to bind to lunabase address: {e}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                if let Err(e) = udp.connect(self.lunabase_address).await {
                    error!("Failed to connect to lunabase: {e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
                break udp;
            };
        
            let mut wait_for: Option<Duration>;
        
            macro_rules! send {
                ($data: expr) => {{
                    loop {
                        if let Err(e) = udp.send($data).await {
                            error!("Failed to send data to lunabase: {e}");
                            continue;
                        }
                        action = cakap_sm.poll(Event::NoEvent, Instant::now());
                        break;
                    }
                }};
            }
        
            let mut buf= [0u8; 1408];
            macro_rules! handle {
                () => {
                    loop {
                        match action {
                            RecommendedAction::WaitForData => {
                                wait_for = None;
                                break;
                            }
                            RecommendedAction::WaitForDuration(duration) => {
                                wait_for = Some(duration);
                                break;
                            }
                            RecommendedAction::HandleError(cakap_error) => {
                                error!("{cakap_error}");
                                action = cakap_sm.poll(Event::NoEvent, Instant::now());
                            }
                            RecommendedAction::HandleData(received) => {(self.on_msg)(&received);}
                            RecommendedAction::HandleDataAndSend { received, to_send } =>  if (self.on_msg)(&received) {
                                send!(&to_send);
                            }
                            RecommendedAction::SendData(hot_packet) => {
                                send!(&hot_packet);
                            }
                        }
                    }
                }
            }
            handle!();
            let mut bitcode_buffer = bitcode::Buffer::new();
        
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(800)) => {
                        let bytes = bitcode_buffer.encode(&FromLunabot::Ping(self.lunabot_stage.load()));
                        if let Err(e) = udp.send(bytes).await {
                            error!("Failed to send ping to lunabase: {e}");
                        }
                        continue;
                    }
                    _ = async {
                        if let Some(duration) = wait_for {
                            tokio::time::sleep(duration).await;
                        } else {
                            std::future::pending::<()>().await;
                        }
                    } => {
                        action = cakap_sm.poll(Event::NoEvent, Instant::now());
                    }
                    packet = async {
                        if let Some(packet) = packet_rx.recv().await {
                            packet
                        } else {
                            warn!("Packet channel closed");
                            std::future::pending().await
                        }
                    } => {
                        action = cakap_sm.poll(Event::Action(packet), Instant::now());
                    }
                    result = udp.recv(&mut buf) => {
                        let n = match result {
                            Ok(n) => n,
                            Err(e) => {
                                error!("Failed to receive data from lunabase: {e}");
                                continue;
                            }
                        };
                        action = cakap_sm.poll(Event::IncomingData(&buf[..n]), Instant::now());
                    }
                }
                handle!();
            }
        });

        PacketBuilder {
            builder: packet_builder,
            packet_tx,
        }
    }
}
