use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use axum::{
    extract::{ws::Message, ConnectInfo, Request, State, WebSocketUpgrade},
    http::{
        uri::{Authority, Scheme},
        Uri,
    },
    routing::{any, get},
    Router,
};
use reqwest::{Client, Url};
use tokio::net::UdpSocket;

#[axum::debug_handler]
async fn lunabot_redirect(
    client: State<Client>,
    mut request: Request,
) -> Result<axum::response::Response, String> {
    let mut uri_parts = axum::http::uri::Parts::default();
    uri_parts.scheme = Some(Scheme::HTTP);
    uri_parts.authority = Some(Authority::from_static("127.0.0.1:21141"));
    uri_parts.path_and_query = request.uri().path_and_query().cloned();

    *request.uri_mut() = Uri::from_parts(uri_parts).unwrap();
    let mut new_request = reqwest::Request::new(
        request.method().clone(),
        Url::parse(&request.uri().to_string()).unwrap(),
    );
    new_request
        .headers_mut()
        .extend(std::mem::take(request.headers_mut()));
    *new_request.body_mut() = Some(reqwest::Body::wrap_stream(
        request.into_body().into_data_stream(),
    ));

    let mut resp = client
        .execute(new_request)
        .await
        .map_err(|e| e.to_string())?;
    let mut builder = axum::response::Response::builder().status(resp.status());

    for (k, v) in std::mem::take(resp.headers_mut()) {
        let Some(k) = k else {
            continue;
        };
        builder = builder.header(k, v);
    }

    Ok(builder
        .body(axum::body::Body::from(
            resp.bytes().await.map_err(|e| e.to_string())?,
        ))
        .unwrap())
}

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
        .route("/lunabot", any(lunabot_redirect))
        .with_state(Client::new())
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

    let listener = tokio::net::TcpListener::bind("0.0.0.0:80")
        .await
        .expect("Failed to bind TCP listener");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
