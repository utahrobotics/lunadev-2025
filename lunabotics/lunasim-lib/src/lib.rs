use std::{
    io::{stdin, stdout, BufReader, Read, Write},
    sync::{mpsc::Sender, Arc},
};

use common::lunasim::{FromLunasim, FromLunasimbot};
use crossbeam::queue::SegQueue;
use godot::{
    classes::Engine,
    global::{randf_range, randfn},
    prelude::*,
};

struct LunasimLib;

#[gdextension]
unsafe impl ExtensionLibrary for LunasimLib {}

struct LunasimShared {
    from_lunasimbot: SegQueue<FromLunasimbot>,
    to_lunasimbot: Sender<FromLunasim>,
}

#[derive(GodotClass)]
#[class(base=Node)]
struct Lunasim {
    #[var]
    accelerometer_deviation: f64,
    #[var]
    gyroscope_deviation: f64,
    #[var]
    depth_deviation: f64,
    shared: Arc<LunasimShared>,
    base: Base<Node>,
}

#[godot_api]
impl INode for Lunasim {
    fn init(base: Base<Node>) -> Self {
        let (to_lunasimbot, to_lunasimbot_rx) = std::sync::mpsc::channel();
        let shared = Arc::new(LunasimShared {
            from_lunasimbot: SegQueue::default(),
            to_lunasimbot,
        });

        if !Engine::singleton().is_editor_hint() {
            let shared2 = shared.clone();
            std::thread::spawn(move || {
                let mut stdin = BufReader::new(stdin().lock());
                let mut size_buf = [0u8; 4];
                let mut bytes = Vec::with_capacity(1024);
                macro_rules! handle_err {
                    ($err: ident) => {{
                        match $err.kind() {
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof => {}
                            _ => {
                                godot_error!(
                                    "Faced the following error while reading stdin: {:?}",
                                    $err
                                );
                            }
                        }
                        break;
                    }};
                }

                loop {
                    let size = match stdin.read_exact(&mut size_buf) {
                        Ok(_) => u32::from_ne_bytes(size_buf),
                        Err(e) => handle_err!(e),
                    };
                    bytes.resize(size as usize, 0u8);
                    match stdin.read_exact(&mut bytes) {
                        Ok(_) => {}
                        Err(e) => handle_err!(e),
                    }
                    match FromLunasimbot::try_from(bytes.as_slice()) {
                        Ok(msg) => shared2.from_lunasimbot.push(msg),
                        Err(e) => {
                            godot_error!("Failed to deserialize from lunasim: {e}");
                            continue;
                        }
                    }
                }
            });

            std::thread::spawn(move || {
                let mut stdout = stdout().lock();

                loop {
                    let Ok(msg) = to_lunasimbot_rx.recv() else {
                        break;
                    };
                    if let Err(e) = msg.encode(|bytes| {
                        if bytes.len() > u32::MAX as usize {
                            godot_error!("Message is too large");
                            return Ok(());
                        }
                        let len = bytes.len() as u32;
                        stdout.write_all(&len.to_ne_bytes())?;
                        stdout.write_all(bytes)
                    }) {
                        godot_error!("Faced the following error while writing to stdout: {e}");
                    }
                }
            });
        }

        Self {
            accelerometer_deviation: 0.0,
            gyroscope_deviation: 0.0,
            depth_deviation: 0.0,
            shared,
            base,
        }
    }

    fn process(&mut self, _delta: f64) {
        while let Some(msg) = self.shared.from_lunasimbot.pop() {
            match msg {
                FromLunasimbot::FittedPoints(points) => {
                    let points: Vec<_> = Box::into_iter(points)
                        .map(|[x, y, z]| Vector3 { x, y, z })
                        .collect();

                    self.base_mut()
                        .emit_signal("fitted_points".into(), &[points.to_variant()]);
                }
                FromLunasimbot::Isometry { quat, origin } => {
                    let [x, y, z, w] = quat;
                    let quat = Quaternion { x, y, z, w };
                    let basis = Basis::from_quat(quat);
                    let [x, y, z] = origin;
                    let origin = Vector3 { x, y, z };

                    self.base_mut().emit_signal(
                        "transform".into(),
                        &[Transform3D { basis, origin }.to_variant()],
                    );
                }
                FromLunasimbot::Drive { left, right } => {
                    self.base_mut()
                        .emit_signal("drive".into(), &[left.to_variant(), right.to_variant()]);
                }
            }
        }
    }
}

#[godot_api]
impl Lunasim {
    #[signal]
    fn fitted_points(points: Vec<Vector3>);
    #[signal]
    fn transform(transform: Transform3D);
    #[signal]
    fn drive(left: f32, right: f32);

    #[func]
    fn send_depth_map(&mut self, mut depth: Vec<f32>) {
        depth.iter_mut().for_each(|d| {
            *d += randfn(0.0, self.depth_deviation) as f32;
        });

        let _ = self
            .shared
            .to_lunasimbot
            .send(FromLunasim::DepthMap(depth.into_boxed_slice()));
    }

    #[func]
    fn send_accelerometer(&mut self, id: u64, mut accel: Vector3) {
        accel += Vector3 {
            x: randfn(0.0, self.accelerometer_deviation) as f32,
            y: randfn(0.0, self.accelerometer_deviation) as f32,
            z: randfn(0.0, self.accelerometer_deviation) as f32,
        };
        let _ = self.shared.to_lunasimbot.send(FromLunasim::Accelerometer {
            id: id as usize,
            acceleration: [accel.x, accel.y, accel.z],
        });
    }

    #[func]
    fn send_gyroscope(&mut self, id: u64, mut delta: Quaternion) {
        let mut rand_axis = Vector3 {
            x: randf_range(-1.0, 1.0) as f32,
            y: randf_range(-1.0, 1.0) as f32,
            z: randf_range(-1.0, 1.0) as f32,
        };
        rand_axis = rand_axis.normalized();
        let angle = randfn(0.0, self.gyroscope_deviation) as f32;
        delta = Quaternion::from_axis_angle(rand_axis, angle) * delta;
        let axis_angle = delta.get_axis() * delta.get_angle();
        let _ = self.shared.to_lunasimbot.send(FromLunasim::Gyroscope {
            id: id as usize,
            axisangle: [axis_angle.x, axis_angle.y, axis_angle.z],
        });
    }
}
