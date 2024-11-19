#![feature(iterator_try_collect)]
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    process::ExitCode,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    body::Body,
    extract::{ws::Message, ConnectInfo, Path, WebSocketUpgrade},
    response::{Html, Response},
    routing::{get, post},
    Router,
};
use crossbeam::queue::SegQueue;
use fxhash::{FxBuildHasher, FxHashSet};
use serde::Deserialize;
use tokio::{
    fs::File,
    io::AsyncWriteExt,
    net::UdpSocket,
    process::Command,
    sync::{Notify, RwLock},
    time::Instant,
};

#[derive(Deserialize)]
struct Config {
    robots: Vec<RobotConfig>,
    #[serde(with = "humantime_serde")]
    #[serde(default = "default_max_duration")]
    max_duration: Duration,
    #[serde(default = "default_go2rtc_path")]
    go2rtc_path: PathBuf,
}

const fn default_max_duration() -> Duration {
    Duration::from_secs(300)
}

fn default_go2rtc_path() -> PathBuf {
    PathBuf::from("go2rtc")
}

#[derive(Deserialize)]
struct RobotConfig {
    name: String,
    addr: SocketAddrV4,
}

struct RobotInstance {
    socket: UdpSocket,
    reserved: AtomicBool,
}

struct OnDrop<F: FnOnce()> {
    f: Option<F>,
}

impl<F> From<F> for OnDrop<F>
where
    F: FnOnce(),
{
    fn from(f: F) -> Self {
        Self { f: Some(f) }
    }
}

impl<F> Drop for OnDrop<F>
where
    F: FnOnce(),
{
    fn drop(&mut self) {
        (self.f.take().unwrap())();
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    if !std::path::Path::new("mouser-config.toml").exists() {
        eprintln!("mouser-config.toml file not found");
        return ExitCode::FAILURE;
    }
    let config = std::fs::read_to_string("mouser-config.toml").unwrap();
    let config: Config = toml::from_str(&config).unwrap();
    let mut robot_conns =
        HashMap::with_capacity_and_hasher(config.robots.len(), FxBuildHasher::default());

    for RobotConfig { name, addr } in config.robots {
        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        socket.connect(addr).await.unwrap();
        robot_conns.insert(
            name,
            RobotInstance {
                socket,
                reserved: AtomicBool::new(false),
            },
        );
    }

    let wait_queue: &SegQueue<Arc<Notify>> = Box::leak(Box::new(SegQueue::new()));
    let robot_conns: &_ = Box::leak(Box::new(robot_conns));

    if !config.go2rtc_path.exists() {
        eprintln!("{:#?} not found", config.go2rtc_path);
        return ExitCode::FAILURE;
    }

    let address_log: FxHashSet<IpAddr>;
    let address_log_file = if std::path::Path::new("address_log.txt").exists() {
        let text = match tokio::fs::read_to_string("address_log.txt").await {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Error opening address log file: {}", e);
                return ExitCode::FAILURE;
            }
        };

        address_log = match text.lines().map(|x| x.parse()).try_collect() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Error parsing address log file: {}", e);
                return ExitCode::FAILURE;
            }
        };

        match File::options().append(true).open("address_log.txt").await {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Error creating address log file: {}", e);
                return ExitCode::FAILURE;
            }
        }
    } else {
        address_log = FxHashSet::default();
        match File::create("address_log.txt").await {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Error creating address log file: {}", e);
                return ExitCode::FAILURE;
            }
        }
    };
    let addresses = RwLock::new((address_log, address_log_file));
    let addresses: &_ = Box::leak(Box::new(addresses));

    let canonicalized = match config.go2rtc_path.canonicalize() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error canonicalizing path {:#?}: {}", config.go2rtc_path, e);
            return ExitCode::FAILURE;
        }
    };
    let child = match Command::new(canonicalized)
        .current_dir("mouser/mouser-host")
        .spawn()
    {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Error starting go2rtc: {}", e);
            return ExitCode::FAILURE;
        }
    };

    tokio::spawn(async move {
        let result = child.wait_with_output().await;
        eprintln!("go2rtc exited with: {:#?}", result);
    });

    let app = Router::new()
        .route(
            "/log_ip",
            post(
                move |ConnectInfo(addr): ConnectInfo<SocketAddr>| async move {
                    let ip = addr.ip();
                    if ip == IpAddr::V4(Ipv4Addr::LOCALHOST)
                        || ip == IpAddr::V4(Ipv4Addr::LOCALHOST)
                    {
                        return Response::builder()
                            .header("Content-Type", "application/json")
                            .body(Body::from("{}"))
                            .unwrap();
                    }
                    let mut new_ip = { !addresses.read().await.0.contains(&ip) };
                    if new_ip {
                        let mut writer = addresses.write().await;
                        new_ip = writer.0.insert(ip);
                        if new_ip {
                            if let Err(e) = writer.1.write_all(ip.to_string().as_bytes()).await {
                                eprintln!("Error writing to address log file: {}", e);
                            } else {
                                let _ = writer.1.flush().await;
                            }
                        }
                    }
                    Response::builder()
                        .header("Content-Type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap()
                },
            ),
        )
        .route(
            "/ws/",
            get(move |ws: WebSocketUpgrade| async move {
                ws.on_upgrade(move |mut ws| async move {
                    let (name, instance) = 'main: loop {
                        for (name, instance) in robot_conns {
                            let was_reserved = instance.reserved.swap(true, Ordering::Relaxed);
                            if !was_reserved {
                                break 'main (name, instance);
                            }
                        }
                        if let Err(e) = ws.send("Queued".into()).await {
                            eprintln!("Error sending message: {}", e);
                            return;
                        }
                        let notify = Arc::new(Notify::new());
                        let notified = notify.notified();
                        wait_queue.push(notify.clone());
                        notified.await;
                    };
                    let socket = &instance.socket;
                    let _on_drop = OnDrop::from(|| {
                        instance.reserved.store(false, Ordering::Relaxed);
                        if let Some(waiting) = wait_queue.pop() {
                            waiting.notify_one();
                        }
                    });
                    if let Err(e) = ws.send(name.as_str().into()).await {
                        eprintln!("Error sending message: {}", e);
                        return;
                    }
                    let mut deadline = Instant::now() + config.max_duration;
                    loop {
                        let result = tokio::select! {
                            option = ws.recv() => {
                                let Some(result) = option else { break; };
                                result
                            },
                            _ = tokio::time::sleep_until(deadline) => {
                                if let Some(waiting) = wait_queue.pop() {
                                    std::mem::forget(_on_drop);
                                    instance.reserved.store(false, Ordering::Relaxed);
                                    waiting.notify_one();
                                    break;
                                }
                                deadline += config.max_duration;
                                continue;
                            }
                        };
                        let msg = match result {
                            Ok(x) => x,
                            Err(e) => {
                                eprintln!("Error receiving message: {}", e);
                                continue;
                            }
                        };
                        match msg {
                            Message::Text(text) => match text.as_str() {
                                "W" | "A" | "S" | "D" => {
                                    if let Err(e) = socket.send(text.as_bytes()).await {
                                        eprintln!(
                                            "Error sending message to {}: {}",
                                            socket.peer_addr().unwrap(),
                                            e
                                        );
                                    }
                                }
                                _ => {
                                    eprintln!("Ignoring unknown message: {}", text);
                                }
                            },
                            Message::Binary(_) => {
                                eprintln!("Ignoring binary message");
                            }
                            _ => {}
                        }
                    }
                })
            }),
        )
        .route(
            "/",
            get(|| async { Html(include_str!("../../mouser-web/build/index.html")) }),
        )
        .route(
            "/favicon.png",
            get(|| async {
                let bytes: &[u8] = include_bytes!("../../mouser-web/build/favicon.png");
                Response::builder()
                    .header("Content-Type", "image/png")
                    .body(Body::from(bytes))
                    .unwrap()
            }),
        )
        .route(
            "/_app/version.json",
            get(|| async {
                Response::builder()
                    .header("Content-Type", "application/json")
                    .body(Body::from(include_str!(
                        "../../mouser-web/build/_app/version.json"
                    )))
                    .unwrap()
            }),
        )
        .route(
            "/_app/immutable/chunks/:path",
            get(|Path(path): Path<String>| async move {
                let bytes = tokio::fs::read(format!(
                    "mouser/mouser-web/build/_app/immutable/chunks/{path}"
                ))
                .await
                .unwrap();
                Response::builder()
                    .header("Content-Type", "application/javascript")
                    .body(Body::from(bytes))
                    .unwrap()
            }),
        )
        .route(
            "/_app/immutable/nodes/:path",
            get(|Path(path): Path<String>| async move {
                let bytes = tokio::fs::read(format!(
                    "mouser/mouser-web/build/_app/immutable/nodes/{path}"
                ))
                .await
                .unwrap();
                Response::builder()
                    .header("Content-Type", "application/javascript")
                    .body(Body::from(bytes))
                    .unwrap()
            }),
        )
        .route(
            "/_app/immutable/assets/:path",
            get(|Path(path): Path<String>| async move {
                let bytes = tokio::fs::read(format!(
                    "mouser/mouser-web/build/_app/immutable/assets/{path}"
                ))
                .await
                .unwrap();
                Response::builder()
                    // CSS
                    .header("Content-Type", "text/css")
                    .body(Body::from(bytes))
                    .unwrap()
            }),
        )
        .route(
            "/_app/immutable/entry/:path",
            get(|Path(path): Path<String>| async move {
                let bytes = tokio::fs::read(format!(
                    "mouser/mouser-web/build/_app/immutable/entry/{path}"
                ))
                .await
                .unwrap();
                Response::builder()
                    .header("Content-Type", "application/javascript")
                    .body(Body::from(bytes))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:80").await.unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
    unreachable!()
}
