use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use axum::{
    extract::{ws::Message, ConnectInfo, WebSocketUpgrade}, routing::get, Router
};
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() {
    // build our application with a single route
    let app = Router::new()
        .route("/", get(|| async {
            "Hello, world!"
        }))
        .route("/ip", get(|ConnectInfo(addr): ConnectInfo<SocketAddr>| async move {
            format!("Your IP is {}", addr.ip())
        }))
        .route("/udp-ws", get(|ws: WebSocketUpgrade| async {
            ws.on_upgrade(|mut socket| async move {
                let mut send_to = None;
                let mut udp = None;
                
                for port in 30000..=30100 {
                    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
                    match UdpSocket::bind(addr).await {
                        Ok(x) => {
                            udp = Some(x);
                            break;
                        }
                        Err(e) => {
                            if port == 30100 {
                                eprintln!("Failed to bind UDP socket: {e}");
                                return;
                            }
                        },
                    };
                }

                let udp = udp.unwrap();

                let addr = match udp.local_addr() {
                    Ok(x) => x,
                    Err(e) => {
                        eprintln!("Failed to get local UDP address: {e}");
                        return;
                    },
                };
                if let Err(e) = socket.send(format!("Set lunabase_address in lunaserver app-config.toml to {}", addr).into()).await {
                    eprintln!("Failed to send lunabase_address to client: {e}");
                    return;
                }
                let mut buf = [0u8; 2048];
                loop {
                    tokio::select! {
                        option = socket.recv() => {
                            let Some(result) = option else {
                                break;
                            };
                            let msg = match result {
                                Ok(msg) => msg,
                                Err(e) => {
                                    eprintln!("Received error from peer: {e}");
                                    break;
                                },
                            };
                            let Message::Binary(data) = msg else {
                                continue;
                            };
                            if let Some(addr) = send_to {
                                // Peer could be down for many trivial reasons
                                let _ = udp.send_to(&data, &addr).await;
                            }
                        }
                        result = udp.recv_from(&mut buf) => {
                            let Ok((n, from)) = result else {
                                // Also could be down for many trivial reasons
                                continue;
                            };
                            send_to = Some(from);
                            if let Err(e) = socket.send(Message::Binary(buf[..n].to_vec())).await {
                                eprintln!("Failed to send UDP packet to client: {e}");
                                break;
                            }
                        }
                    }
                }
            })
        }));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:80").await.expect("Failed to bind TCP listener");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    ).await.unwrap();
}