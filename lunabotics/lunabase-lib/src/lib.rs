#![feature(backtrace_frames)]

use std::{
    collections::VecDeque, net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket}, sync::{
        atomic::{AtomicBool, Ordering},
        Once,
    }, time::{Duration, Instant}
};

use bitcode::encode;
use cakap2::{
    packet::{Action, ReliableIndex},
    Event, PeerStateMachine, RecommendedAction,
};
use crossbeam::atomic::AtomicCell;
#[cfg(feature = "production")]
use nalgebra::{Isometry3, Quaternion, Vector3, UnitQuaternion};
use common::{FromLunabase, FromLunabot, LunabotStage, Steering, THALASSIC_CELL_SIZE, THALASSIC_HEIGHT, THALASSIC_WIDTH};
#[cfg(feature = "production")]
use common::lunabase_sync::ThalassicData;
use godot::{
    classes::{image::Format, Engine, Image, Os},
    prelude::*,
};
use tasker::shared::{OwnedData, SharedDataReceiver};

#[cfg(feature = "audio")]
mod audio;
#[cfg(feature = "production")]
mod stream;
#[cfg(feature = "production")]
mod config;

const STREAM_WIDTH: u32 = 1920;
const STREAM_HEIGHT: u32 = 720;

struct LunabaseLib;

#[gdextension]
unsafe impl ExtensionLibrary for LunabaseLib {}

static PANIC_INIT: Once = Once::new();

pub fn init_panic_hook() {
    PANIC_INIT.call_once(|| {
        // To enable backtrace, you will need the `backtrace` crate to be included in your cargo.toml, or
        // a version of Rust where backtrace is included in the standard library (e.g. Rust nightly as of the date of publishing)
        // use backtrace::Backtrace;
        use std::backtrace::Backtrace;
        let old_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let loc_string;
            if let Some(location) = panic_info.location() {
                loc_string = format!("file '{}' at line {}", location.file(), location.line());
            } else {
                loc_string = "unknown location".to_owned()
            }

            let error_message;
            if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
                error_message = format!("[RUST] {}: panic occurred: {:?}", loc_string, s);
            } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
                error_message = format!("[RUST] {}: panic occurred: {:?}", loc_string, s);
            } else {
                error_message = format!("[RUST] {}: unknown panic occurred", loc_string);
            }
            godot_error!("{}", error_message);
            // Uncomment the following line if backtrace crate is included as a dependency
            for frame in Backtrace::force_capture().frames() {
                godot_error!("{frame:?}");
            }
            (*(old_hook.as_ref()))(panic_info);

            // unsafe {
            // if let Some(gd_panic_hook) = godot::api::utils::autoload::<gdnative::api::Node>("rust_panic_hook") {
            //     gd_panic_hook.call("rust_panic_hook", &[GodotString::from_str(error_message).to_variant()]);
            // }
            // }
        }));
    });
}

struct LunabotConnInner {
    cakap_sm: PeerStateMachine,
    udp: UdpSocket,
    to_lunabot: VecDeque<Action>,
    bitcode_buffer: bitcode::Buffer,
    did_reconnection: bool,
    last_steering: Option<(Steering, ReliableIndex)>,
    send_to: Option<IpAddr>,
    stream_lendee: SharedDataReceiver<Vec<u8>>,
    stream_corrupted: &'static AtomicBool,
}

#[derive(GodotClass)]
#[class(base=Node)]
struct LunabotConn {
    inner: Option<LunabotConnInner>,
    base: Base<Node>,
    #[var]
    stream_image: Gd<Image>,
    #[var]
    stream_image_updated: bool,
    last_received_duration: f64,
    #[cfg(feature = "audio")]
    audio_streaming: Option<audio::AudioStreaming>,
    #[cfg(feature = "production")]
    app_config: Option<config::AppConfig>,
    #[cfg(feature = "production")]
    thalassic_data: &'static AtomicCell<Option<ThalassicData>>,
}

thread_local! {
    static PONG_MESSAGE: Box<[u8]> = {
        encode(&FromLunabase::Pong).into()
    };
}

#[godot_api]
impl INode for LunabotConn {
    fn init(base: Base<Node>) -> Self {
        let thalassic_data: &_ = Box::leak(Box::new(AtomicCell::new(None)));

        let stream_image = Image::create_empty(
            STREAM_WIDTH as i32,
            STREAM_HEIGHT as i32,
            false,
            Format::RGB8,
        )
        .unwrap();
        if Engine::singleton().is_editor_hint() {
            return Self {
                inner: None,
                base,
                stream_image,
                stream_image_updated: false,
                #[cfg(feature = "audio")]
                audio_streaming: None,
                last_received_duration: 0.0,
                #[cfg(feature = "production")]
                app_config: None,
                #[cfg(feature = "production")]
                thalassic_data,
            };
        }

        let lunabot_address_str = Os::singleton()
            .get_cmdline_user_args()
            .get(0)
            .map(|x| x.to_string())
            .unwrap_or_else(|| "192.168.0.102".into());
        let lunabot_address = {
            if let Ok(addr) = lunabot_address_str.parse::<IpAddr>() {
                godot_warn!("Connecting to: {lunabot_address_str}");
                Some(addr)
            } else {
                godot_error!("Failed to parse address: {lunabot_address_str}");
                None
            }
        };

        if let Some(lunabot_address) = lunabot_address {
            common::lunabase_sync::lunabase_task(lunabot_address,
                |thalassic| {
                    thalassic_data.store(Some(thalassic.clone()));
                    // godot_print!("Thalassic data received {:?}", thalassic.heightmap.iter().filter(|&&h| h as f32 != 0.0).count());
                },
                |_path| {

                },
                |e| {
                    if e.kind() == std::io::ErrorKind::ConnectionRefused {
                        return;
                    }
                    godot_error!("Error in lunabase sync: {e}");
                },
            );
        }

        init_panic_hook();

        let stream_corrupted: &_ = Box::leak(Box::new(AtomicBool::new(false)));
        let mut shared_rgb_img =
            OwnedData::from(vec![
                0u8;
                STREAM_WIDTH as usize * STREAM_HEIGHT as usize * 3
            ]);
        let stream_lendee = shared_rgb_img.create_lendee();
        #[cfg(feature = "production")]
        stream::camera_streaming(
            lunabot_address,
            shared_rgb_img.pessimistic_share(),
            stream_corrupted,
        );

        let udp = UdpSocket::bind(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            #[cfg(not(feature = "production"))]
            common::ports::LUNABASE_SIM_TELEOP,
            #[cfg(feature = "production")]
            common::ports::TELEOP,
        ))
        .expect("Failed to bind to teleop port");

        udp.set_nonblocking(true)
            .expect("Failed to set non-blocking");

        let cakap_sm = PeerStateMachine::new(Duration::from_millis(150), 1024, 1400);
        #[cfg(feature = "audio")]
        let audio_streaming = audio::AudioStreaming::new(lunabot_address);

        Self {
            inner: Some(LunabotConnInner {
                udp,
                cakap_sm,
                to_lunabot: VecDeque::new(),
                bitcode_buffer: bitcode::Buffer::new(),
                did_reconnection: false,
                last_steering: None,
                send_to: lunabot_address,
                stream_lendee,
                stream_corrupted,
            }),
            base,
            stream_image,
            stream_image_updated: false,
            #[cfg(feature = "audio")]
            audio_streaming: Some(audio_streaming),
            last_received_duration: 0.0,
            #[cfg(feature = "production")]
            app_config: config::load_config(),
            #[cfg(feature = "production")]
            thalassic_data,
        }
    }

    fn process(&mut self, delta: f64) {
        if let Some(mut inner) = self.inner.as_mut() {
            let mut received = false;

            if let Some(data) = inner.stream_lendee.try_get() {
                self.stream_image.set_data(
                    STREAM_WIDTH as i32,
                    STREAM_HEIGHT as i32,
                    false,
                    Format::RGB8,
                    &PackedByteArray::from(&**data),
                );
                self.stream_image_updated = true;
                received = true;
            }

            macro_rules! on_msg {
                ($msg: ident) => {{
                    received = true;
                    match $msg {
                        #[cfg(feature = "production")]
                        FromLunabot::RobotIsometry { origin: [x, y, z], quat: [i, j, k, w] } => {
                            if let Some(app_config) = &self.app_config {
                                app_config.robot_chain.set_isometry(
                                    Isometry3::from_parts(
                                        Vector3::new(x, y, z).cast().into(),
                                        UnitQuaternion::new_unchecked(Quaternion::new(w, i, j, k).cast()),
                                    ),
                                );
                            }
                            // let transform = Transform3D {
                            //     basis: Basis::from_quat(Quaternion { x: i, y: j, z: k, w }),
                            //     origin: Vector3 { x, y, z },
                            // };
                            // self.base_mut().emit_signal("robot_transform", &[transform.to_variant()]);
                            inner = self.inner.as_mut().unwrap();
                        }
                        #[cfg(not(feature = "production"))]
                        FromLunabot::RobotIsometry { .. } => {}
                        FromLunabot::Ping(stage) => {
                            match stage {
                                LunabotStage::TeleOp => {
                                    self.base_mut().emit_signal("entered_manual", &[])
                                }
                                LunabotStage::SoftStop => {
                                    self.base_mut().emit_signal("entered_soft_stop", &[])
                                }
                                LunabotStage::TraverseObstacles => self
                                    .base_mut()
                                    .emit_signal("entered_traverse_obstacles", &[]),
                                LunabotStage::Dig => {
                                    self.base_mut().emit_signal("entered_dig", &[])
                                }
                                LunabotStage::Dump => {
                                    self.base_mut().emit_signal("entered_dump", &[])
                                }
                            };
                            inner = self.inner.as_mut().unwrap();

                            PONG_MESSAGE.with(|pong| {
                                inner.to_lunabot.push_back(Action::SendUnreliable(
                                    inner
                                        .cakap_sm
                                        .get_packet_builder()
                                        .new_unreliable(pong.to_vec().into())
                                        .unwrap(),
                                ));
                            });
                        }
                    }
                }};
            }

            macro_rules! handle {
                ($action: ident) => {
                    match $action {
                        RecommendedAction::HandleError(cakap_error) => {
                            godot_error!("{cakap_error}")
                        }
                        RecommendedAction::HandleData(received) => {
                            match inner.bitcode_buffer.decode::<FromLunabot>(received) {
                                Ok(x) => {
                                    on_msg!(x);
                                }
                                Err(e) => {
                                    godot_error!("Failed to decode message: {e}")
                                }
                            }
                        }
                        RecommendedAction::HandleDataAndSend { received, to_send } => {
                            match inner.bitcode_buffer.decode::<FromLunabot>(received) {
                                Ok(x) => {
                                    if let Some(ip) = inner.send_to {
                                        if let Err(e) = inner.udp.send_to(
                                            &to_send,
                                            SocketAddr::new(ip, common::ports::TELEOP),
                                        ) {
                                            godot_error!("Failed to send ack: {e}");
                                        }
                                        on_msg!(x);
                                    }
                                }
                                Err(e) => {
                                    godot_error!("Failed to decode message: {e}")
                                }
                            }
                        }
                        RecommendedAction::SendData(hot_packet) => {
                            if let Some(ip) = inner.send_to {
                                if let Err(e) = inner.udp.send_to(
                                    &hot_packet,
                                    SocketAddr::new(ip, common::ports::TELEOP),
                                ) {
                                    godot_error!("Failed to send hot packet: {e}");
                                }
                            }
                        }
                        RecommendedAction::WaitForData | RecommendedAction::WaitForDuration(_) => {}
                    }
                };
            }

            let now = Instant::now();

            while let Some(to_lunabot) = inner.to_lunabot.pop_front() {
                let action = inner.cakap_sm.poll(Event::Action(to_lunabot), now);
                handle!(action);
            }

            let mut buf = [0u8; 1408];
            loop {
                match inner.udp.recv_from(&mut buf) {
                    Ok((n, addr)) => {
                        // if addr.port() != common::ports::TELEOP {
                        //     godot_warn!("Received data from unknown client: {addr}");
                        //     continue;
                        // }
                        inner.send_to = Some(addr.ip());
                        if !inner.did_reconnection {
                            let tmp_action = inner.cakap_sm.send_reconnection_msg(now).0;
                            handle!(tmp_action);
                            inner.did_reconnection = true;
                        }
                        let action = inner.cakap_sm.poll(Event::IncomingData(&buf[..n]), now);
                        handle!(action);
                    }
                    Err(e) => match e.kind() {
                        std::io::ErrorKind::WouldBlock => break,
                        _ => godot_error!("Failed to receive data: {e}"),
                    },
                }
            }

            loop {
                let action = inner.cakap_sm.poll(Event::NoEvent, now);
                match action {
                    RecommendedAction::HandleError(cakap_error) => godot_error!("{cakap_error}"),
                    RecommendedAction::SendData(hot_packet) => {
                        if let Some(ip) = inner.send_to {
                            if let Err(e) = inner
                                .udp
                                .send_to(&hot_packet, SocketAddr::new(ip, common::ports::TELEOP))
                            {
                                godot_error!("Failed to send hot packet: {e}");
                            }
                        }
                    }
                    RecommendedAction::WaitForData | RecommendedAction::WaitForDuration(_) => break,
                    _ => unreachable!(),
                }
            }

            if received {
                self.last_received_duration = 0.0;
                self.base_mut().emit_signal("something_received", &[]);
            } else {
                self.last_received_duration += delta;

                if self.last_received_duration >= 1.0 {
                    self.last_received_duration = 0.0;
                    if inner.send_to.is_some() {
                        PONG_MESSAGE.with(|pong| {
                            inner.to_lunabot.push_back(Action::SendUnreliable(
                                inner
                                    .cakap_sm
                                    .get_packet_builder()
                                    .new_unreliable(pong.to_vec().into())
                                    .unwrap(),
                            ));
                        });
                    }
                }
            }
        }
        #[cfg(feature = "audio")]
        if let Some(mut audio_streaming) = self.audio_streaming.take() {
            audio_streaming.poll(self.base_mut(), delta);
            self.audio_streaming = Some(audio_streaming);
        }

        #[cfg(feature = "production")]
        if let Some(thalassic) = self.thalassic_data.take() {
            let heightmap: PackedFloat32Array = thalassic.heightmap.into_iter().map(|x| x as f32).collect();
            self.base_mut().emit_signal("heightmap_received", &[heightmap.to_variant()]);
        }
    }
}

impl LunabotConn {
    fn send_reliable(&mut self, msg: &FromLunabase) {
        if let Some(inner) = &mut self.inner {
            match inner
                .cakap_sm
                .get_packet_builder()
                .new_reliable(encode(msg).into())
            {
                Ok(packet) => {
                    inner.to_lunabot.push_back(Action::SendReliable(packet));
                }
                Err(e) => {
                    godot_error!("Failed to build reliable packet: {e}");
                }
            }
        }
    }

    fn set_steering(&mut self, new_steering: Steering) {
        if let Some(inner) = &mut self.inner {
            let mut last_steering_reliable_idx = None;
            if let Some((old_steering, old_idx)) = inner.last_steering {
                last_steering_reliable_idx = Some(old_idx);
                if old_steering == new_steering {
                    return;
                }
            }
            let msg = FromLunabase::Steering(new_steering);
            match inner
                .cakap_sm
                .get_packet_builder()
                .new_reliable(encode(&msg).into())
            {
                Ok(packet) => {
                    if let Some(old_idx) = last_steering_reliable_idx {
                        inner.to_lunabot.push_back(Action::CancelReliable(old_idx));
                    }
                    inner.last_steering = Some((new_steering, packet.get_index()));
                    inner.to_lunabot.push_back(Action::SendReliable(packet));
                }
                Err(e) => {
                    godot_error!("Failed to build reliable packet: {e}");
                }
            }
        }
    }

    // fn send_unreliable(&mut self, msg: &FromLunabase) {
    //     if let Some(inner) = &mut self.inner {
    //         match inner.cakap_sm.get_packet_builder().new_unreliable(encode(msg).into()) {
    //             Ok(packet) => {
    //                 inner.to_lunabot.push_back(Action::SendUnreliable(packet));
    //             }
    //             Err(e) => {
    //                 godot_error!("Failed to build reliable packet: {e}");
    //             }
    //         }
    //     }
    // }
}

#[godot_api]
impl LunabotConn {
    #[signal]
    fn something_received(&self);
    #[signal]
    fn entered_manual(&self);
    #[signal]
    fn entered_soft_stop(&self);
    #[signal]
    fn entered_traverse_obstacles(&self);
    #[signal]
    fn entered_dig(&self);
    #[signal]
    fn entered_dump(&self);
    #[signal]
    fn heightmap_received(&self, heightmap: PackedFloat32Array);

    #[constant]
    const CAMERA_STREAMING: bool = cfg!(feature = "production");
    #[constant]
    const STREAM_WIDTH: i32 = STREAM_WIDTH as i32;
    #[constant]
    const STREAM_HEIGHT: i32 = STREAM_HEIGHT as i32;
    #[constant]
    const GRID_WIDTH: i32 = THALASSIC_WIDTH as i32;
    #[constant]
    const GRID_HEIGHT: i32 = THALASSIC_HEIGHT as i32;

    /// There is a bug that prevents const f64 from being used in the godot_api macro
    #[func]
    fn get_cell_size() -> f64 {
        THALASSIC_CELL_SIZE as f64
    }

    /// There is a bug that prevents const f64 from being used in the godot_api macro
    #[func]
    fn get_default_steering_weight() -> f64 {
        Steering::DEFAULT_WEIGHT
    }

    #[cfg(feature = "production")]
    #[func]
    fn get_camera_transform(&self, stream_index: i64) -> Transform3D {
        if stream_index < 0 {
            godot_error!("Invalid stream index: {stream_index}");
            return Transform3D::default();
        }
        let stream_index = stream_index as usize;
        let Some(app_config) = &self.app_config else {
            return Transform3D::default();
        };
        let Some(Some(data)) = app_config.camera.get(stream_index) else {
            godot_error!("Invalid or nonexistent stream index: {stream_index}");
            return Transform3D::default();
        };
        let isometry = data.node.get_global_isometry();
        let [x, y, z] = isometry.translation.vector.cast().data.0[0];
        let [i, j, k, w] = isometry.rotation.coords.cast().data.0[0];
        Transform3D {
            basis: Basis::from_quat(godot::prelude::Quaternion { x: i, y: j, z: k, w }),
            origin: godot::prelude::Vector3 { x, y, z },
        }
    }

    #[cfg(feature = "production")]
    #[func]
    fn get_camera_fov(&self, stream_index: i64) -> f64 {
        if stream_index < 0 {
            godot_error!("Invalid stream index: {stream_index}");
            return 75.0f64;
        }
        let stream_index = stream_index as usize;
        let Some(app_config) = &self.app_config else {
            return 75.0f64;
        };
        let Some(Some(data)) = app_config.camera.get(stream_index) else {
            godot_error!("Invalid or nonexistent stream index: {stream_index}");
            return 75.0f64;
        };
        data.fov
    }

    #[cfg(feature = "production")]
    #[func]
    fn does_camera_exist(&self, stream_index: i64) -> bool {
        if stream_index < 0 {
            godot_error!("Invalid stream index: {stream_index}");
            return false;
        }
        let Some(app_config) = &self.app_config else {
            return false;
        };
        let stream_index = stream_index as usize;
        let Some(Some(_)) = app_config.camera.get(stream_index) else {
            return false;
        };
        true
    }

    #[cfg(not(feature = "production"))]
    #[func]
    fn get_camera_transform(&self, _stream_index: i64) -> Transform3D {
        Transform3D::default()
    }

    #[cfg(not(feature = "production"))]
    #[func]
    fn get_camera_fov(&self, stream_index: i64) -> f64 {
        75.0f
    }

    #[cfg(not(feature = "production"))]
    #[func]
    fn does_camera_exist(&self, stream_index: i64) -> bool {
        false
    }

    #[func]
    fn is_stream_corrupted(&self) -> bool {
        self.inner.as_ref().map_or(false, |inner| {
            inner.stream_corrupted.load(Ordering::Relaxed)
        })
    }
    #[func]
    fn set_steering_left_right(&mut self, left: f64, right: f64, weight: f64) {
        self.set_steering(Steering::new(left, right, weight));
    }

    #[func]
    fn continue_mission(&mut self) {
        self.send_reliable(&FromLunabase::ContinueMission);
    }

    #[func]
    fn traverse_obstacles(&mut self) {
        self.send_reliable(&FromLunabase::TraverseObstacles);
    }

    #[func]
    fn soft_stop(&mut self) {
        self.send_reliable(&FromLunabase::SoftStop);
    }
}
