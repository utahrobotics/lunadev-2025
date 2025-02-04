use std::{net::SocketAddr, path::Path, process::Stdio};

use axum::{
    extract::{ConnectInfo, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::{any, get},
    Router,
};
use tokio::process::Command;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(|| async { "Hello, world!" }))
        .route(
            "/init-lunabot",
            any(|ws: WebSocketUpgrade| async {
                if !Path::new("/home/manglemix/lunadev-2025/.allow-ws").exists() {
                    return (StatusCode::FORBIDDEN, "").into_response();
                }
                ws.on_upgrade(|mut ws| async move {
                    let mut child = Command::new("/home/manglemix/.cargo/bin/lunabot")
                        .arg("main")
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .env("HEADLESS", "1")
                        .current_dir("/home/manglemix/lunadev-2025")
                        .spawn()
                        .unwrap();
                    let pid = child.id().unwrap();
                    tokio::select! {
                        _ = ws.recv() => {
                            let _ = Command::new("kill")
                                .args(["-s", "INT", pid.to_string().as_str()])
                                .status()
                                .await;
                        }
                        _ = child.wait() => {}
                    }
                })
            }),
        )
        .route(
            "/ip",
            get(|ConnectInfo(addr): ConnectInfo<SocketAddr>| async move {
                format!("Your IP is {}", addr.ip())
            }),
        );

    loop {
        std::thread::sleep(std::time::Duration::from_secs(3));
        let listener = match tokio::net::TcpListener::bind((
            option_env!("SERVER_IP").unwrap_or("0.0.0.0"),
            80,
        ))
        .await
        {
            Ok(x) => x,
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        };
        if let Err(e) = axum::serve(
            listener,
            app.clone()
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
            eprintln!("{e}");
        }
    }
}
