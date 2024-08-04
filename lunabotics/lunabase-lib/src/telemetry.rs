use std::{
    net::{Ipv4Addr, SocketAddrV4},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crossbeam::{atomic::AtomicCell, queue::SegQueue};
use godot::{engine::notify::NodeNotification, obj::BaseMut, prelude::*};
use lunabot_lib::{
    make_negotiation, ArmAction, ArmParameters, Audio, AutonomyAction, CameraMessage,
    ControlsPacket, ExecutiveArmAction, ImportantMessage,
};
use networking::new_server;
use unros::{
    anyhow,
    logging::rate::RateLogger,
    node::SyncNode,
    pubsub::{MonoPublisher, Publisher, Subscriber},
    runtime::{start_unros_runtime, MainRuntimeContext},
    setup_logging, tokio,
};

use crate::audio::{init_audio, MIC_PLAYBACK};

struct LunabotShared {
    base_mut_queue: SegQueue<Box<dyn FnOnce(BaseMut<LunabotConn>) + Send>>,
    controls_data: AtomicCell<ControlsPacket>,
    echo_controls: AtomicBool,
    connected: AtomicBool,
    enable_camera: AtomicBool,
    audio_pub: Mutex<Publisher<Audio>>,
    camera_pub: Mutex<Publisher<CameraMessage>>,
    important_pub: Mutex<Publisher<ImportantMessage>>,
}

#[derive(GodotClass)]
#[class(base=Node)]
struct LunabotConn {
    shared: Option<Arc<LunabotShared>>,
    base: Base<Node>,
    thr: Option<JoinHandle<()>>,
}

#[godot_api]
impl INode for LunabotConn {
    fn init(base: Base<Node>) -> Self {
        let shared = LunabotShared {
            base_mut_queue: SegQueue::default(),
            controls_data: AtomicCell::default(),
            echo_controls: AtomicBool::default(),
            connected: AtomicBool::default(),
            enable_camera: AtomicBool::new(true),
            audio_pub: Mutex::new(Publisher::default()),
            camera_pub: Mutex::new(Publisher::default()),
            important_pub: Mutex::new(Publisher::default()),
        };
        let shared = Arc::new(shared);

        Self {
            shared: Some(shared),
            base,
            thr: None,
        }
    }

    fn process(&mut self, _delta: f64) {
        if let Some(shared) = self.shared.as_ref().cloned() {
            while let Some(func) = shared.base_mut_queue.pop() {
                func(self.base_mut());
            }
        }
    }

    fn ready(&mut self) {
        init_audio();
        self.base().get_tree().unwrap().set_auto_accept_quit(false);
        std::fs::File::create("camera-ffmpeg.log").expect("camera-ffmpeg.log should be writable");
        let shared = self.shared.clone().unwrap();
        // let mut audio_player = AudioStreamPlayer::new_alloc();
        // let mut audio_gen = AudioStreamGenerator::new_gd();
        // let playback = audio_gen.instantiate_playback().unwrap();
        // let mut playback: Gd<AudioStreamGeneratorPlayback> = playback.cast();
        // audio_player.set_stream(audio_gen.upcast());
        // self.base_mut().add_child(audio_player.upcast());

        let main = |context: MainRuntimeContext| async move {
            let peer_sub = Subscriber::new(1);
            let (network_node, _) = new_server::<u8, _>(
                SocketAddrV4::new(Ipv4Addr::from_bits(0), LunabotConn::RECV_FROM),
                peer_sub.create_subscription(),
            )?;
            network_node.spawn(context.make_context("conn-sender"));

            setup_logging!(context);
            let negotiation = make_negotiation();

            loop {
                let peer;
                tokio::select! {
                    result = peer_sub.recv_or_closed() => {
                        let Some(result) = result else {
                            error!("Server closed itself");
                            godot_error!("Server closed itself");
                            break Ok(());
                        };

                        peer = match result {
                            Ok((_, x)) => x,
                            Err(e) => {
                                error!("Invalid init data: {e}");
                                godot_error!("Invalid init data: {e}");
                                continue;
                            }
                        };
                    }
                    _ = async {
                        loop {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            if Arc::strong_count(&shared) == 1 {
                                break;
                            }
                        }
                    } => {
                        break Ok(());
                    }
                }

                let (important, camera, odometry, controls, audio, audio_controls) =
                    match peer.negotiate(&negotiation).await {
                        Ok(x) => x,
                        Err(e) => {
                            error!("Failed to negotiate with lunabot! {e:?}");
                            godot_error!("Failed to negotiate with lunabot! {e:?}");
                            continue;
                        }
                    };

                shared.base_mut_queue.push(Box::new(|mut base| {
                    base.emit_signal("connected".into(), &[]);
                }));
                shared.echo_controls.store(false, Ordering::Relaxed);
                shared.connected.store(true, Ordering::Relaxed);
                let mut last_receive_time = Instant::now();
                let mut ffplay: Option<std::process::Child> = None;
                let mut ffplay_stderr: Option<JoinHandle<std::io::Result<u64>>> = None;

                let camera_sub = Subscriber::new(1);
                camera.accept_subscription(camera_sub.create_subscription());
                shared
                    .camera_pub
                    .lock()
                    .unwrap()
                    .accept_subscription(camera.create_reliable_subscription());

                let audio_sub = Subscriber::new(32);
                audio.accept_subscription(audio_sub.create_subscription());

                let mut controls_pub =
                    MonoPublisher::from(controls.create_unreliable_subscription());
                let controls_sub = Subscriber::new(1);
                controls.accept_subscription(controls_sub.create_subscription());

                shared
                    .important_pub
                    .lock()
                    .unwrap()
                    .accept_subscription(important.create_reliable_subscription());
                let important_sub = Subscriber::new(8);
                important.accept_subscription(important_sub.create_subscription());

                let odometry_sub = Subscriber::new(8);
                odometry.accept_subscription(odometry_sub.create_subscription());

                shared
                    .audio_pub
                    .lock()
                    .unwrap()
                    .accept_subscription(audio_controls.create_reliable_subscription());

                let mut last_enable_camera = shared.enable_camera.load(Ordering::Relaxed);

                macro_rules! kill_ffplay {
                    () => {
                        if let Some(mut ffplay) = ffplay {
                            match ffplay.kill() {
                                Ok(()) => match ffplay.wait() {
                                    Ok(x) => info!("ffplay exited with {x}"),
                                    Err(e) => error!("ffplay failed to exit {e}"),
                                },
                                Err(e) => error!("ffplay failed to be killed {e}"),
                            }
                        }
                    };
                }

                let mut rate = RateLogger::default();
                loop {
                    macro_rules! received {
                        () => {{
                            let tmp = last_receive_time.elapsed();
                            last_receive_time += tmp;
                            shared.base_mut_queue.push(Box::new(|mut base| {
                                base.emit_signal("something_received".into(), &[]);
                            }));
                        }};
                    }
                    macro_rules! make_ffplay {
                        () => {{
                            // let mut child = std::process::Command::new("ffplay")
                            // .args([
                            //     "-protocol_whitelist",
                            //     "file,rtp,udp",
                            //     "-i",
                            //     "camera.sdp",
                            //     "-flags",
                            //     "low_delay",
                            //     "-avioflags",
                            //     "direct",
                            //     "-probesize",
                            //     "32",
                            //     "-analyzeduration",
                            //     "0",
                            //     "-sync",
                            //     "ext",
                            //     "-framedrop",
                            // ])
                            let mut child = std::process::Command::new("mplayer")
                                .args(["-monitorpixelaspect", "1", "-nocache", "sdp://camera.sdp"])
                                // .args(["rtp://192.168.0.102:43722"])
                                .stderr(Stdio::piped())
                                .spawn()
                                .expect("Failed to init ffplay process");

                            if let Some(mut ffplay) = ffplay.take() {
                                let _ = ffplay.kill();
                            }

                            let mut stderr = child.stderr.take().unwrap();
                            ffplay = Some(child);

                            ffplay_stderr = Some(std::thread::spawn(move || {
                                std::io::copy(
                                    &mut stderr,
                                    &mut std::fs::OpenOptions::new()
                                        .append(true)
                                        .create(true)
                                        .open("camera-ffmpeg.log")
                                        .expect("camera-ffmpeg.log should be writable"),
                                )
                            }));
                        }};
                    }
                    tokio::select! {
                        result = important_sub.recv_or_closed() => {
                            let Some(result) = result else {
                                godot_error!("important_sub closed");
                                error!("important_sub closed");
                                break;
                            };
                            received!();

                            let msg = match result {
                                Ok(x) => x,
                                Err(e) => {
                                    godot_error!("Failed to parse incoming log: {e}");
                                    continue;
                                }
                            };

                            match msg {
                                _ => {}
                            }
                        }
                        result = camera_sub.recv_or_closed() => {
                            let Some(result) = result else {
                                godot_error!("camera_sub closed");
                                error!("camera_sub closed");
                                break;
                            };
                            received!();

                            let msg = match result {
                                Ok(x) => x,
                                Err(e) => {
                                    godot_error!("Failed to parse incoming sdp: {e}");
                                    error!("Failed to parse incoming sdp: {e}");
                                    continue;
                                }
                            };

                            let sdp = match msg {
                                lunabot_lib::CameraMessage::Sdp(x) => x,
                                lunabot_lib::CameraMessage::Swap(_, _) => continue,
                                // msg => {
                                //     godot_error!("Unexpected camera message: {msg:?}");
                                //     error!("Unexpected camera message: {msg:?}");
                                //     continue;
                                // }
                            };

                            std::fs::write("camera.sdp", sdp.as_bytes())
                                        .expect("camera.sdp should be writable");

                            make_ffplay!();
                        }
                        result = controls_sub.recv_or_closed() => {
                            let Some(result) = result else {
                                godot_error!("controls_sub closed");
                                error!("controls_sub closed");
                                break;
                            };
                            received!();

                            let controls = match result {
                                Ok(x) => x,
                                Err(e) => {
                                    godot_error!("Failed to parse incoming controls: {e}");
                                    continue;
                                }
                            };

                            shared.echo_controls.store(controls != shared.controls_data.load(), Ordering::Relaxed);
                        }
                        result = odometry_sub.recv_or_closed() => {
                            let Some(result) = result else {
                                godot_error!("odometry_sub closed");
                                error!("odometry_sub closed");
                                break;
                            };
                            received!();

                            match result {
                                Ok(odom) => {
                                    shared.base_mut_queue.push(Box::new(move |mut base| {
                                        base.emit_signal("odometry_received".into(), &[
                                            odom.arm_angle.to_variant(),
                                            Vector3::new(odom.acceleration[0], odom.acceleration[1], odom.acceleration[2]).to_variant(),
                                            odom.front_elevation.to_variant(),
                                            odom.back_elevation.to_variant(),
                                        ]);
                                    }));
                                }
                                Err(e) => {
                                    godot_error!("Failed to parse incoming odometry: {e}");
                                    continue;
                                }
                            }
                        }
                        result = audio_sub.recv_or_closed() => {
                            let Some(result) = result else {
                                godot_error!("audio_sub closed");
                                error!("audio_sub closed");
                                break;
                            };
                            if let Ok(frame) = result {
                                // godot_print!("{frame}");
                                rate.increment();
                                MIC_PLAYBACK.cell.store(frame * 4.0);
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(50)) => {}
                    }

                    if Arc::strong_count(&shared) == 1 {
                        kill_ffplay!();
                        return Ok(());
                    }

                    if shared.echo_controls.load(Ordering::Relaxed) {
                        controls_pub.set(shared.controls_data.load());
                    }

                    if let Some(inner) = &mut ffplay_stderr {
                        if inner.is_finished() {
                            if let Err(e) = ffplay_stderr.take().unwrap().join().unwrap() {
                                error!("{e}");
                                godot_error!("{e}");
                            }
                        }
                    }

                    let current_enable_camera = shared.enable_camera.load(Ordering::Relaxed);
                    if current_enable_camera != last_enable_camera {
                        last_enable_camera = current_enable_camera;
                        if current_enable_camera {
                            shared
                                .important_pub
                                .lock()
                                .unwrap()
                                .set(ImportantMessage::EnableCamera);

                            if ffplay_stderr.is_none() {
                                make_ffplay!();
                            }
                        } else {
                            shared
                                .important_pub
                                .lock()
                                .unwrap()
                                .set(ImportantMessage::DisableCamera);
                        }
                    }
                }

                shared.connected.store(false, Ordering::Relaxed);
                shared.base_mut_queue.push(Box::new(|mut base| {
                    base.emit_signal("disconnected".into(), &[]);
                }));
                kill_ffplay!();
            }
        };
        self.thr = Some(std::thread::spawn(|| {
            if let Some(Err(e)) = start_unros_runtime::<anyhow::Result<()>, _>(main, |_| {}) {
                godot_error!("{e}");
                unros::log::error!("{e}");
            }
        }));
    }

    fn on_notification(&mut self, notif: NodeNotification) {
        if notif == NodeNotification::WmCloseRequest {
            self.shared = None;
            if let Some(thr) = self.thr.take() {
                let _ = thr.join();
            }

            self.base()
                .get_tree()
                .unwrap()
                .call_deferred("quit".into(), &[]);
        }
    }
}

#[godot_api]
impl LunabotConn {
    #[constant]
    const RECV_FROM: u16 = 43721;

    #[signal]
    fn connected(&self);

    #[signal]
    fn disconnected(&self);

    #[signal]
    fn something_received(&self);

    #[signal]
    fn odometry_received(
        &self,
        arm_angle: f32,
        acceleration: Vector3,
        front_elevation: f32,
        back_elevation: f32,
    );

    #[func]
    fn is_lunabot_connected(&mut self) -> bool {
        self.shared
            .as_ref()
            .unwrap()
            .connected
            .load(Ordering::Relaxed)
    }

    #[func]
    fn send_steering(&self, mut drive: f32, mut steering: f32) {
        if drive > 1.0 {
            drive = 1.0;
            godot_warn!("Drive greater than 1!")
        }
        if drive < -1.0 {
            drive = -1.0;
            godot_warn!("Drive lesser than -1!");
        }
        if steering > 1.0 {
            steering = 1.0;
            godot_warn!("Steering greater than 1!")
        }
        if steering < -1.0 {
            steering = -1.0;
            godot_warn!("Steering lesser than -1!")
        }

        let shared = self.shared.as_ref().unwrap();
        let mut controls_packet = shared.controls_data.load();
        let new_drive = (drive * 127.0).round() as i8;
        let new_steering = (steering * 127.0).round() as i8;
        if controls_packet.drive == new_drive && controls_packet.steering == new_steering {
            return;
        }
        controls_packet.drive = new_drive;
        controls_packet.steering = new_steering;
        shared.controls_data.store(controls_packet);
        shared.echo_controls.store(true, Ordering::Relaxed);
    }

    #[func]
    fn lift_arm(&self) {
        let shared = self.shared.as_ref().unwrap();
        let mut controls_packet = shared.controls_data.load();
        controls_packet.arm_params.lift = ArmAction::Extend;
        shared.controls_data.store(controls_packet);
        shared.echo_controls.store(true, Ordering::Relaxed);
    }

    #[func]
    fn lower_arm(&self) {
        let shared = self.shared.as_ref().unwrap();
        let mut controls_packet = shared.controls_data.load();
        controls_packet.arm_params.lift = ArmAction::Retract;
        shared.controls_data.store(controls_packet);
        shared.echo_controls.store(true, Ordering::Relaxed);
    }

    #[func]
    fn stop_lift_arm(&self) {
        let shared = self.shared.as_ref().unwrap();
        let mut controls_packet = shared.controls_data.load();
        controls_packet.arm_params.lift = ArmAction::Stop;
        shared.controls_data.store(controls_packet);
        shared.echo_controls.store(true, Ordering::Relaxed);
    }

    #[func]
    fn tilt_bucket_up(&self) {
        let shared = self.shared.as_ref().unwrap();
        let mut controls_packet = shared.controls_data.load();
        controls_packet.arm_params.tilt = ArmAction::Retract;
        shared.controls_data.store(controls_packet);
        shared.echo_controls.store(true, Ordering::Relaxed);
    }

    #[func]
    fn tilt_bucket_down(&self) {
        let shared = self.shared.as_ref().unwrap();
        let mut controls_packet = shared.controls_data.load();
        controls_packet.arm_params.tilt = ArmAction::Extend;
        shared.controls_data.store(controls_packet);
        shared.echo_controls.store(true, Ordering::Relaxed);
    }

    #[func]
    fn stop_tilt_bucket(&self) {
        let shared = self.shared.as_ref().unwrap();
        let mut controls_packet = shared.controls_data.load();
        controls_packet.arm_params.tilt = ArmAction::Stop;
        shared.controls_data.store(controls_packet);
        shared.echo_controls.store(true, Ordering::Relaxed);
    }

    #[func]
    fn stop_arm(&self) {
        let shared = self.shared.as_ref().unwrap();
        let mut controls_packet = shared.controls_data.load();
        controls_packet.arm_params = ArmParameters::default();
        shared.controls_data.store(controls_packet);
        shared.echo_controls.store(true, Ordering::Relaxed);
    }

    #[func]
    fn enable_camera(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared.enable_camera.store(true, Ordering::Relaxed);
    }

    #[func]
    fn disable_camera(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared.enable_camera.store(false, Ordering::Relaxed);
    }

    #[func]
    fn play_buzz(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared.audio_pub.lock().unwrap().set(Audio::PlayBuzz);
    }

    #[func]
    fn pause_buzz(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared.audio_pub.lock().unwrap().set(Audio::PauseBuzz);
    }

    #[func]
    fn play_music(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared.audio_pub.lock().unwrap().set(Audio::PlayMusic);
    }

    #[func]
    fn pause_music(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared.audio_pub.lock().unwrap().set(Audio::PauseMusic);
    }

    #[func]
    fn swap_camera(&self, first: i64, second: i64) {
        if first < 0 {
            godot_error!("first must be non-negative");
            return;
        }
        if second < 0 {
            godot_error!("second must be non-negative");
            return;
        }
        let shared = self.shared.as_ref().unwrap();
        shared
            .camera_pub
            .lock()
            .unwrap()
            .set(CameraMessage::Swap(first as usize, second as usize));
    }

    #[func]
    fn dig_autonomy(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared
            .important_pub
            .lock()
            .unwrap()
            .set(ImportantMessage::Autonomy(AutonomyAction::Dig));
    }

    #[func]
    fn dump_autonomy(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared
            .important_pub
            .lock()
            .unwrap()
            .set(ImportantMessage::Autonomy(AutonomyAction::Dump));
    }

    #[func]
    fn stop_autonomy(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared
            .important_pub
            .lock()
            .unwrap()
            .set(ImportantMessage::Autonomy(AutonomyAction::Stop));
    }

    #[func]
    fn home_tilt(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared
            .important_pub
            .lock()
            .unwrap()
            .set(ImportantMessage::ExecutiveArmAction(ArmParameters {
                lift: ExecutiveArmAction::None,
                tilt: ExecutiveArmAction::Home,
            }));
    }

    #[func]
    fn home_lift(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared
            .important_pub
            .lock()
            .unwrap()
            .set(ImportantMessage::ExecutiveArmAction(ArmParameters {
                tilt: ExecutiveArmAction::None,
                lift: ExecutiveArmAction::Home,
            }));
    }

    #[func]
    fn reset_tilt(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared
            .important_pub
            .lock()
            .unwrap()
            .set(ImportantMessage::ExecutiveArmAction(ArmParameters {
                lift: ExecutiveArmAction::None,
                tilt: ExecutiveArmAction::SoftReset,
            }));
    }

    #[func]
    fn reset_lift(&self) {
        let shared = self.shared.as_ref().unwrap();
        shared
            .important_pub
            .lock()
            .unwrap()
            .set(ImportantMessage::ExecutiveArmAction(ArmParameters {
                tilt: ExecutiveArmAction::None,
                lift: ExecutiveArmAction::SoftReset,
            }));
    }
}
