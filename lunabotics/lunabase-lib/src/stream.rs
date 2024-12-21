use std::{
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{extract::{ws, WebSocketUpgrade}, response::Html, routing::get, Router};
use crossbeam::{atomic::AtomicCell, utils::Backoff};
use godot::global::{godot_error, godot_print};
use openh264::{decoder::Decoder, nal_units};
use tasker::shared::{LoanedData, MaybeOwned, SharedDataReceiver};
use tokio::{sync::Notify, task::block_in_place};
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_H264},
        APIBuilder,
    },
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit}, ice_connection_state::RTCIceConnectionState,
        ice_server::RTCIceServer,
    },
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState, sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{track_local_static_sample::TrackLocalStaticSample, TrackLocal},
};

#[derive(Default)]
struct BroadcastingBuffer {
    bytes: Vec<u8>,
    packet_sizes: Vec<usize>,
}

pub fn camera_streaming(
    mut shared_rgb_img: LoanedData<Vec<u8>>,
    stream_corrupted: &'static AtomicBool,
) {
    let mut broadcasting_buffer = MaybeOwned::from(BroadcastingBuffer::default());
    let lendee_storage: Arc<AtomicCell<Option<SharedDataReceiver<BroadcastingBuffer>>>> =
        Arc::new(AtomicCell::new(None));
    let lendee_storage2 = lendee_storage.clone();

    std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(3)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let app = Router::new()
                    .route("/", get(|| async { Html(include_str!("index.html")) }))
                    .route(
                        "/rtc",
                        get(|ws: WebSocketUpgrade| async {
                            ws.on_upgrade(|mut ws| async move {
                                let mut m = MediaEngine::default();
                                m.register_default_codecs()
                                    .expect("Failed to register default codecs");
                                let mut registry = Registry::new();
                                registry = register_default_interceptors(registry, &mut m)
                                    .expect("Failed to register default interceptors");

                                let api = APIBuilder::new()
                                    .with_media_engine(m)
                                    .with_interceptor_registry(registry)
                                    .build();

                                let config = RTCConfiguration {
                                    ice_servers: vec![RTCIceServer {
                                        urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                                        ..Default::default()
                                    }],
                                    ..Default::default()
                                };
                                let peer_connection = Arc::new(
                                    api.new_peer_connection(config)
                                        .await
                                        .expect("Failed to create peer connection"),
                                );

                                let video_track = Arc::new(TrackLocalStaticSample::new(
                                    RTCRtpCodecCapability {
                                        mime_type: MIME_TYPE_H264.to_owned(),
                                        ..Default::default()
                                    },
                                    "video".to_owned(),
                                    "webrtc-rs".to_owned(),
                                ));

                                // Add this newly created track to the PeerConnection
                                let rtp_sender = peer_connection
                                    .add_track(Arc::clone(&video_track)
                                        as Arc<dyn TrackLocal + Send + Sync>)
                                    .await
                                    .expect("Failed to add video track to peer connection");

                                // Read incoming RTCP packets
                                // Before these packets are returned they are processed by interceptors. For things
                                // like NACK this needs to be called.
                                tokio::spawn(async move {
                                    let mut rtcp_buf = vec![0u8; 1500];
                                    while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
                                });

                                let connected_notify = Arc::new(Notify::new());
                                let connected_notify2 = connected_notify.clone();
                                peer_connection.on_ice_connection_state_change(Box::new(
                                    move |connection_state: RTCIceConnectionState| {
                                        if connection_state == RTCIceConnectionState::Connected {
                                            connected_notify2.notify_waiters();
                                        }
                                        Box::pin(async {})
                                    },
                                ));

                                let disconnected_notify = Arc::new(Notify::new());
                                let disconnected_notify2 = disconnected_notify.clone();
                                peer_connection.on_peer_connection_state_change(Box::new(
                                    move |s: RTCPeerConnectionState| {
                                        println!("Peer Connection State has changed: {s}");

                                        if s == RTCPeerConnectionState::Failed {
                                            disconnected_notify2.notify_waiters();
                                        }

                                        Box::pin(async {})
                                    },
                                ));

                                let (to_send_tx, mut to_send_rx) = tokio::sync::mpsc::channel(1);

                                let to_send_tx2 = to_send_tx.clone();
                                peer_connection.on_ice_candidate(Box::new(
                                    move |ice: Option<RTCIceCandidate>| {
                                        let ice = ice.map(|ice| ice.to_json().unwrap());
                                        let to_send_tx2 = to_send_tx2.clone();
                                        Box::pin(async move {
                                            let _ = to_send_tx2
                                                .send(serde_json::to_string(&ice).unwrap())
                                                .await;
                                        })
                                    },
                                ));

                                loop {
                                    tokio::select! {
                                        _ = connected_notify.notified() => break,
                                        _ = disconnected_notify.notified() => {
                                            let _ = ws.close().await;
                                            return;
                                        }
                                        opt = to_send_rx.recv() => {
                                            let Some(to_send) = opt else {
                                                let _ = ws.close().await;
                                                return;
                                            };
                                            if ws.send(to_send.into()).await.is_err() {
                                                break;
                                            }
                                        }
                                        opt = ws.recv() => {
                                            let Some(Ok(ws::Message::Text(msg))) = opt else {
                                                let _ = ws.close().await;
                                                return;
                                            };
                                            if let Ok(offer) = serde_json::from_str::<RTCSessionDescription>(&msg) {
                                                peer_connection.set_remote_description(offer).await.expect("Failed to set remote description");
                                                let answer = peer_connection.create_answer(None).await.expect("Failed to create answer");
                                                if ws.send(serde_json::to_string(&answer).unwrap().into()).await.is_err() {
                                                    break;
                                                }
                                                peer_connection.set_local_description(answer).await.expect("Failed to set local description");
                                            } else {
                                                let ice = serde_json::from_str::<Option<RTCIceCandidateInit>>(&msg).expect("Failed to parse ice candidate");
                                                peer_connection.add_ice_candidate(ice.unwrap_or(RTCIceCandidateInit {
                                                    candidate: "".into(),
                                                    ..Default::default()
                                                })).await.expect("Failed to add ice candidate");
                                            }
                                        }
                                    }
                                }

                                let _ = ws.close().await;

                                let backoff = Backoff::new();
                                let receiver = loop {
                                    let tmp = lendee_storage.take();
                                    if let Some(receiver) = tmp {
                                        break receiver;
                                    }
                                    backoff.snooze();
                                };
                                tokio::select! {
                                    _ = async {
                                        'main: loop {
                                            let buffer = block_in_place(|| receiver.get());
                                            let mut start_i = 0usize;
        
                                            for &len in &buffer.packet_sizes {
                                                if video_track
                                                    .write_sample(&Sample {
                                                        data: buffer.bytes
                                                            [start_i..(start_i + len)]
                                                            .to_vec()
                                                            .into(),
                                                        // TODO: Calculate duration
                                                        duration: Duration::from_secs(0),
                                                        ..Default::default()
                                                    })
                                                    .await
                                                    .is_err()
                                                {
                                                    break 'main;
                                                }
                                                start_i += len;
                                            }
                                        }
                                    } => {}
                                    _ = disconnected_notify.notified() => {}
                                }

                                let _ = peer_connection.close().await;
                            })
                        }),
                    );

                let listener = tokio::net::TcpListener::bind("0.0.0.0:80")
                    .await
                    .expect("Failed to bind TCP listener");
                godot_print!("HTTP Server started");
                axum::serve(listener, app.into_make_service())
                    .await
                    .unwrap();
            });
    });

    std::thread::spawn(move || {
        let stream_udp = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 10601))
            .expect("Failed to bind to 10601");

        if let Err(e) = Decoder::new() {
            godot_error!("Failed to initialize decoder: {e}");
            return;
        }
        let mut dec = Decoder::new().expect("Failed to initialize decoder");
        let mut buf = [0u8; 1400];
        let mut stream = vec![];

        godot_print!("Stream server started");
        let mut nals = vec![];

        loop {
            match stream_udp.recv(&mut buf) {
                Ok(n) => {
                    stream.extend_from_slice(&buf[..n]);
                }
                Err(e) => {
                    godot_error!("Failed to receive stream data: {e}");
                    break;
                }
            }

            let mut last_stream_i = 0usize;
            let start_i = stream.as_ptr() as usize;
            nals.extend(
                nal_units(&stream)
                    .into_iter()
                    .map(|nal| (nal.as_ptr() as usize - start_i, nal.len())),
            );
            let mut read_frame = false;
            // The last packet is usually incomplete
            nals.pop();

            for &(stream_index, len) in nals.iter() {
                last_stream_i = stream_index + len;
                match dec.decode(&stream[stream_index..last_stream_i]) {
                    Ok(Some(frame)) => {
                        if !read_frame {
                            read_frame = true;
                            stream_corrupted.store(false, Ordering::Relaxed);
                            match shared_rgb_img.try_recall() {
                                Ok(mut owned) => {
                                    frame.write_rgb8(&mut owned);
                                    shared_rgb_img = owned.pessimistic_share();
                                }
                                Err(shared) => {
                                    shared_rgb_img = shared;
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        stream_corrupted.store(false, Ordering::Relaxed);
                    }
                    Err(_) => {
                        stream_corrupted.store(true, Ordering::Relaxed);
                    }
                }
            }

            if let Some(&(first_nal_i, _)) = nals.first() {
                if broadcasting_buffer.try_recall() {
                    let buffer = broadcasting_buffer.get_mut().unwrap();

                    buffer.bytes.clear();
                    buffer.packet_sizes.clear();
                    buffer
                        .bytes
                        .extend_from_slice(&stream[first_nal_i..last_stream_i]);
                    for (_, len) in nals.drain(..) {
                        buffer.packet_sizes.push(len);
                    }

                    broadcasting_buffer.share();
                } else {
                    nals.clear();
                }
            }

            if let Some(storage) = lendee_storage2.take() {
                lendee_storage2.store(Some(storage));
            } else {
                lendee_storage2.store(Some(broadcasting_buffer.create_lendee()));
            }

            stream.drain(..last_stream_i);
        }
    });
}
