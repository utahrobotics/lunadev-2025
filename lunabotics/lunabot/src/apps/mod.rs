#[cfg(feature = "production")]
mod production;
#[cfg(not(feature = "production"))]
mod sim;

use std::{fs::File, net::SocketAddr, process::Stdio, sync::Arc, time::Duration};

use common::{FromLunabase, FromLunabot, LunabotStage};
use crossbeam::atomic::AtomicCell;
use embedded_common::ActuatorReading;
use lunabot_ai_common::{FromAI, FromHost, ParseError};
#[cfg(feature = "production")]
pub use production::{
    Apriltag, CameraInfo, DepthCameraInfo, LunabotApp, V3PicoInfo, Vesc, ROBOT,
    ROBOT_STRUCTURE, get_recorder, get_obstacle_map_throttle,
};
#[cfg(not(feature = "production"))]
pub use sim::{LunasimStdin, LunasimbotApp};
use simple_motion::StaticImmutableNode;
use tasker::{shared::SharedDataReceiver, tokio::{self, io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter}, sync::{mpsc::{self, UnboundedReceiver}, watch}, time::Instant}};
use tracing::error;

use crate::{pipelines::thalassic::{set_observe_depth, ThalassicData}, teleop::{LunabaseConn, PacketBuilder}};

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

async fn new_ai(mut from_ai: impl FnMut(FromAI), mut from_lunabase_rx: UnboundedReceiver<FromLunabase>, robot_chain: StaticImmutableNode, shared_thalassic_data: SharedDataReceiver<ThalassicData>, readings: &'static AtomicCell<Option<ActuatorReading>>) -> ! {
    loop {
        let mut child = tokio::process::Command::new("cargo")
            .args(["run", "-p", "lunabot-ai2"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();

        let mut stdin = BufWriter::new(child.stdin.take().unwrap());
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let (from_ai_tx, mut from_ai_rx) = tokio::sync::mpsc::channel(32);

        tokio::spawn(async move {
            let mut bytes = vec![];
            let mut buf = [0u8; 256];
            let mut necessary_bytes = 1usize;

            'main: loop {
                match stdout.read(&mut buf).await {
                    Ok(n) => {
                        bytes.extend_from_slice(&buf[0..n]);
                        loop {
                            if bytes.len() < necessary_bytes {
                                break;
                            }
                            match FromAI::parse(&bytes) {
                                Ok((msg, n)) => {
                                    bytes.drain(0..n);
                                    necessary_bytes = 1;
                                    if let Err(e) = from_ai_tx.try_send(msg) {
                                        if from_ai_tx.send(e.into_inner()).await.is_err() {
                                            break 'main;
                                        }
                                    }
                                }
                                Err(ParseError::InvalidData) => {
                                    tracing::error!("AI passed invalid data");
                                    break 'main;
                                }
                                Err(ParseError::NotEnoughBytes { bytes_needed }) => {
                                    necessary_bytes = bytes_needed;
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("AI terminated with {e}");
                        break;
                    }
                }
            }
        });

        let mut last_heartbeat = Instant::now() + std::time::Duration::from_millis(3000);

        let read_fut = async {
            loop {
                tokio::select! {
                    option = from_ai_rx.recv() => {
                        let Some(msg) = option else {
                            break;
                        };
                        last_heartbeat = Instant::now();
                        from_ai(msg);
                    },
                    _ = tokio::time::sleep_until(last_heartbeat + lunabot_ai_common::HOST_HEARTBEAT_LISTEN_RATE) => {
                        tracing::error!("AI timed out");
                        break;
                    }
                };
                
            }
        };

        let write_fut = async {
            let mut bytes = vec![];
            let mut instant = Instant::now();
            loop {
                tokio::select! {
                    option = from_lunabase_rx.recv() => {
                        let Some(msg) = option else {
                            std::future::pending::<()>().await;
                            unreachable!();
                        };
                        FromHost::FromLunabase { msg }.write_into(&mut bytes).unwrap();
                    },
                    _ = tokio::time::sleep_until(instant + Duration::from_millis(50)) => {
                        let isometry = robot_chain.get_global_isometry();
                        FromHost::BaseIsometry { isometry }.write_into(&mut bytes).unwrap();
                        instant = Instant::now();

                        if let Some(reading) = readings.take() {
                            FromHost::ActuatorReadings { lift: reading.m1_reading, bucket: reading.m2_reading }.write_into(&mut bytes).unwrap();
                        }
                    }
                    data = async {
                        loop {
                            let Some(data) = shared_thalassic_data.try_get() else {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                                continue;
                            };
                            break data;
                        }
                    } => {
                        set_observe_depth(false);
                        FromHost::ThalassicData { obstacle_map: Box::new(data.expanded_obstacle_map) }.write_into(&mut bytes).unwrap();
                    }
                };
                if let Err(e) = stdin.write_all(&bytes).await {
                    tracing::error!("AI terminated with {e}");
                    break;
                }
                if let Err(e) = stdin.flush().await {
                    tracing::error!("AI terminated with {e}");
                    break;
                }
                bytes.clear();
            }
        };

        tokio::select! {
            result = child.wait() => {
                match result {
                    Ok(status) => if !status.success() {
                        tracing::error!("AI terminated with {status}");
                    }
                    Err(e) => tracing::error!("AI terminated with {e}"),
                }
                continue;
            }
            _ = read_fut => {}
            _ = write_fut => {}
        }
        if let Err(e) = child.kill().await {
            tracing::error!("Failed to kill AI: {e}")
        }
    }
}