#![feature(backtrace_frames)]
use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::Once,
    time::{Duration, Instant},
};

use bitcode::encode;
use cakap2::{
    packet::{Action, ReliableIndex},
    Event, PeerStateMachine, RecommendedAction,
};
use common::{FromLunabase, FromLunabot, LunabotStage, Steering};
use godot::{classes::Engine, prelude::*};
use image::{DynamicImage, ImageBuffer};
use openh264::decoder::Decoder;
use pollster::FutureExt;
use urobotics_video::VideoDataDump;

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
    send_to: Option<SocketAddr>,
}

#[derive(GodotClass)]
#[class(base=Node)]
struct LunabotConn {
    inner: Option<LunabotConnInner>,
    base: Base<Node>,
}

thread_local! {
    static PONG_MESSAGE: Box<[u8]> = {
        encode(&FromLunabase::Pong).into()
    };
}

#[godot_api]
impl INode for LunabotConn {
    fn init(base: Base<Node>) -> Self {
        if Engine::singleton().is_editor_hint() {
            return Self { inner: None, base };
        }
        init_panic_hook();

        let udp = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 10600))
            .expect("Failed to bind to 10600");

        udp.set_nonblocking(true)
            .expect("Failed to set non-blocking");
        
        std::thread::spawn(move || {
            let stream_udp = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 10601))
                .expect("Failed to bind to 10601");
    
            stream_udp.set_nonblocking(true)
                .expect("Failed to set stream udp non-blocking");

            let mut rgb_img = vec![0u8; 2560 * 1440 * 3];
            let mut display = VideoDataDump::new_display("Stream", 2560, 1440, false).expect("Failed to open display");
            let mut dec = Decoder::new().expect("Failed to initialize decoder");
            let mut buf = [0u8; 1400];

            godot_print!("Stream server started");

            loop {
                match stream_udp.recv(&mut buf) {
                    Ok(n) => {
                        match dec.decode(&buf[..n]) {
                            Ok(Some(frame)) => {
                                frame.write_rgb8(&mut rgb_img);
                                let dyn_img = DynamicImage::ImageRgb8(ImageBuffer::from_raw(2560, 1440, rgb_img).unwrap());
                                if let Err(e) = display.write_frame(&dyn_img).block_on() {
                                    godot_error!("Failed to write frame to display: {e}");
                                    break;
                                }
                                let DynamicImage::ImageRgb8(img) = dyn_img else { unreachable!(); };
                                rgb_img = img.into_raw();
                            }
                            Ok(None) => {}
                            Err(e) => {
                                godot_error!("Failed to decode frame: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        godot_error!("Failed to receive stream data: {e}");
                        break;
                    }
                }
            }
        });

        let cakap_sm = PeerStateMachine::new(Duration::from_millis(150), 1024, 1400);
        // godot_warn!("LunabotConn initialized");

        Self {
            inner: Some(LunabotConnInner {
                udp,
                cakap_sm,
                to_lunabot: VecDeque::new(),
                bitcode_buffer: bitcode::Buffer::new(),
                did_reconnection: false,
                last_steering: None,
                send_to: None,
            }),
            base,
        }
    }

    fn process(&mut self, _delta: f64) {
        if let Some(mut inner) = self.inner.as_mut() {
            let mut received = false;

            macro_rules! on_msg {
                ($msg: ident) => {{
                    received = true;
                    match $msg {
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
                                    if let Some(addr) = inner.send_to {
                                        if let Err(e) = inner.udp.send_to(&to_send, addr) {
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
                            if let Some(addr) = inner.send_to {
                                if let Err(e) = inner.udp.send_to(&hot_packet, addr) {
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
                        // godot_warn!("{:?}", &buf[..n]);
                        inner.send_to = Some(addr);
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
                        if let Some(addr) = inner.send_to {
                            if let Err(e) = inner.udp.send_to(&hot_packet, addr) {
                                godot_error!("Failed to send hot packet: {e}");
                            }
                        }
                    }
                    RecommendedAction::WaitForData | RecommendedAction::WaitForDuration(_) => break,
                    _ => unreachable!(),
                }
            }

            if received {
                self.base_mut().emit_signal("something_received", &[]);
            }
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

    #[func]
    fn set_steering_drive_steering(&mut self, drive: f64, steering: f64) {
        self.set_steering(Steering::new(drive, steering));
    }

    #[func]
    fn set_steering_left_right(&mut self, left: f64, right: f64) {
        self.set_steering(Steering::new_left_right(left, right));
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
