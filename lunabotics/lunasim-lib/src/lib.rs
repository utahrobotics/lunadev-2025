#![feature(try_blocks)]

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

const DEPTH_SCALE: f32 = 0.01;

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
    #[var]
    explicit_apriltag_rotation_deviation: f64,
    #[var]
    explicit_apriltag_translation_deviation: f64,

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
                let mut bitcode_buffer = bitcode::Buffer::new();

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
                    match bitcode_buffer.decode(&bytes) {
                        Ok(msg) => shared2.from_lunasimbot.push(msg),
                        Err(e) => {
                            godot_error!("Failed to deserialize from lunasimbot: {e}");
                            continue;
                        }
                    }
                }
            });

            std::thread::spawn(move || {
                let mut stdout = stdout().lock();
                let mut bitcode_buffer = bitcode::Buffer::new();

                loop {
                    let Ok(msg) = to_lunasimbot_rx.recv() else {
                        break;
                    };
                    let bytes = bitcode_buffer.encode(&msg);
                    if bytes.len() > u32::MAX as usize {
                        godot_error!("Message is too large");
                    } else {
                        let len = bytes.len() as u32;
                        let result: std::io::Result<()> = try {
                            stdout.write_all(&len.to_ne_bytes())?;
                            stdout.write_all(bytes)?
                        };
                        if let Err(e) = result {
                            godot_error!("Faced the following error while writing to stdout: {e}");
                        }
                    }
                }
            });
        }

        Self {
            accelerometer_deviation: 0.0,
            gyroscope_deviation: 0.0,
            depth_deviation: 0.0,
            explicit_apriltag_rotation_deviation: 0.0,
            explicit_apriltag_translation_deviation: 0.0,
            shared,
            base,
        }
    }

    fn process(&mut self, _delta: f64) {
        while let Some(msg) = self.shared.from_lunasimbot.pop() {
            match msg {
                FromLunasimbot::PointCloud(points) => {
                    let points: Vec<_> = Box::into_iter(points)
                        .map(|[x, y, z]| Vector3 { x, y, z })
                        .collect();

                    self.base_mut()
                        .emit_signal("fitted_points", &[points.to_variant()]);
                }
                FromLunasimbot::Isometry {
                    axis,
                    angle,
                    origin,
                } => {
                    let axis = Vector3 {
                        x: axis[0],
                        y: axis[1],
                        z: axis[2],
                    };
                    let basis = Basis::from_axis_angle(axis, angle);
                    let [x, y, z] = origin;
                    let origin = Vector3 { x, y, z };

                    self.base_mut()
                        .emit_signal("transform", &[Transform3D { basis, origin }.to_variant()]);
                }
                FromLunasimbot::Drive { left, right } => {
                    self.base_mut()
                        .emit_signal("drive", &[left.to_variant(), right.to_variant()]);
                }
                FromLunasimbot::Thalassic { heightmap, gradmap } => {
                    let heights: PackedFloat32Array = Box::into_iter(heightmap).collect();
                    let grads: PackedFloat32Array = Box::into_iter(gradmap).collect();
                    
                    self.base_mut()
                        .emit_signal("thalassic", &[heights.to_variant(), grads.to_variant()]);
                }
            }
        }
    }
}

fn rand_quat(deviation: f64) -> Quaternion {
    let mut rand_axis = Vector3 {
        x: randf_range(-1.0, 1.0) as f32,
        y: randf_range(-1.0, 1.0) as f32,
        z: randf_range(-1.0, 1.0) as f32,
    };
    rand_axis = rand_axis.normalized();
    let rand_angle = randfn(0.0, deviation) as f32;
    Quaternion::from_axis_angle(rand_axis, rand_angle)
}

fn rand_vec(deviation: f64) -> Vector3 {
    Vector3 {
        x: randfn(0.0, deviation) as f32,
        y: randfn(0.0, deviation) as f32,
        z: randfn(0.0, deviation) as f32,
    }
}

#[godot_api]
impl Lunasim {
    #[signal]
    fn fitted_points(points: Vec<Vector3>);
    #[signal]
    fn thalassic(heights: PackedFloat32Array, grads: PackedFloat32Array);
    #[signal]
    fn transform(transform: Transform3D);
    #[signal]
    fn drive(left: f32, right: f32);

    #[func]
    fn send_depth_map(&mut self, depth: Vec<f32>) {
        let depth = depth
            .into_iter()
            .map(|d| {
                (randfn(d as f64, (d as f64).powi(2) * self.depth_deviation).abs() as f32
                    / DEPTH_SCALE)
                    .round() as u16
            })
            .collect();

        let _ = self.shared.to_lunasimbot.send(FromLunasim::DepthMap(depth));
    }

    #[func]
    fn send_accelerometer(&mut self, id: u64, mut accel: Vector3) {
        accel += rand_vec(self.accelerometer_deviation);
        let _ = self.shared.to_lunasimbot.send(FromLunasim::Accelerometer {
            id: id as usize,
            acceleration: [accel.x, accel.y, accel.z],
        });
    }

    #[func]
    fn send_gyroscope(&mut self, id: u64, mut angular_difference: Quaternion, delta: f32) {
        let mut angle = angular_difference.get_angle() / delta;

        angular_difference = rand_quat(self.gyroscope_deviation) * angular_difference;
        let mut axis = angular_difference.get_axis();

        if !angle.is_finite()
            || angle.abs() < 0.001
            || !axis.x.is_finite()
            || !axis.y.is_finite()
            || !axis.z.is_finite()
            || (axis.x == 0.0 && axis.y == 0.0 && axis.z == 0.0)
        {
            axis = Vector3::new(0.0, 1.0, 0.0);
            angle = 0.0;
        }

        let _ = self.shared.to_lunasimbot.send(FromLunasim::Gyroscope {
            id: id as usize,
            axis: [axis.x, axis.y, axis.z],
            angle,
        });
    }

    #[func]
    fn send_explicit_apriltag(&mut self, robot_transform: Transform3D) {
        let quat =
            rand_quat(self.explicit_apriltag_rotation_deviation) * robot_transform.basis.to_quat();
        let robot_axis = quat.get_axis();
        let robot_axis = [robot_axis.x, robot_axis.y, robot_axis.z];
        let origin =
            robot_transform.origin + rand_vec(self.explicit_apriltag_translation_deviation);
        let _ = self
            .shared
            .to_lunasimbot
            .send(FromLunasim::ExplicitApriltag {
                robot_axis,
                robot_angle: quat.get_angle(),
                robot_origin: [origin.x, origin.y, origin.z],
            });
    }
}
