use bonsai_bt::Status;
use common::{lunasim::FromLunasimbot, FromLunabase, FromLunabot};
use compute_shader::buffers::{DynamicSize, ReadOnlyBuffer};
use crossbeam::atomic::AtomicCell;
use fitter::{utils::CameraProjection, BufferFitterBuilder, Plane};
use k::{Chain, Isometry3, UnitQuaternion};
use nalgebra::{Rotation3, UnitVector3, Vector2, Vector3, Vector4};
use urobotics::{
    callbacks::caller::try_drop_this_callback, define_callbacks, fn_alias, get_tokio_handle, log::{error, info}, task::SyncTask, tokio::task::{block_in_place, spawn_blocking}, BlockOn
};

use std::{
    cmp::Reverse,
    collections::BinaryHeap,
    net::SocketAddr,
    ops::ControlFlow,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};

use cakap::{CakapSender, CakapSocket};

use crate::{localization::Localizer, run::RunState, utils::{RecycleGuard, Recycler}, LunabotApp, RunMode};

pub(super) fn setup(
    bb: &mut Option<Blackboard>,
    dt: f64,
    first_time: bool,
    lunabot_app: &LunabotApp,
) -> (Status, f64) {
    if first_time {
        info!("Entered Setup");
    }
    if let Some(_) = bb {
        // Review the existing blackboard for any necessary setup
        (Status::Success, dt)
    } else {
        // Create a new blackboard
        let tmp = match Blackboard::new(lunabot_app) {
            Ok(x) => x,
            Err(e) => {
                info!("Failed to create blackboard: {e}");
                return (Status::Failure, dt);
            }
        };
        *bb = Some(tmp);
        (Status::Success, dt)
    }
}

const PING_DELAY: f64 = 1.0;
define_callbacks!(DriveCallbacks => Fn(left: f64, right: f64) + Send);

fn_alias! {
    type FittedCallbacksRef = CallbacksRef(&[Vector4<f32>]) + Send + Sync
}
define_callbacks!(FittedPointsCallbacks => Fn(points: &[Vector4<f32>]) + Send + Sync);


pub struct Blackboard {
    special_instants: BinaryHeap<Reverse<Instant>>,
    lunabase_conn: CakapSender,
    from_lunabase: mpsc::Receiver<FromLunabase>,
    ping_timer: f64,
    drive_callbacks: DriveCallbacks,
    // acceleration: Arc<AtomicCell<Vector3<f64>>>,
    // accelerometer_callbacks: Vector3Callbacks,
    robot_chain: Arc<Chain<f64>>,
    pub(crate) run_state: Option<RunState>,
    unfitted_points_tx: mpsc::Sender<RecycleGuard<ReadOnlyBuffer<[Vector4<f32>]>>>,
    fitted_points_callbacks_ref: FittedCallbacksRef,
}

impl std::fmt::Debug for Blackboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Blackboard").finish()
    }
}

impl Blackboard {
    pub fn new(lunabot_app: &LunabotApp) -> anyhow::Result<Self> {
        let socket = CakapSocket::bind(0).block_on()?;
        let lunabase_conn = socket.get_stream();
        lunabase_conn.set_send_addr(SocketAddr::V4(lunabot_app.lunabase_address));
        match socket.local_addr() {
            Ok(addr) => info!("Bound to {addr}"),
            Err(e) => error!("Failed to get local address: {e}"),
        }
        let (from_lunabase_tx, from_lunabase) = mpsc::channel();
        socket
            .get_bytes_callback_ref()
            .add_dyn_fn(Box::new(move |bytes| {
                let msg: FromLunabase = match TryFrom::try_from(bytes) {
                    Ok(x) => x,
                    Err(e) => {
                        error!("Failed to parse message from lunabase: {e}");
                        return;
                    }
                };
                if from_lunabase_tx.send(msg).is_err() {
                    try_drop_this_callback();
                }
            }));
        socket.spawn_looping();

        let (unfitted_points_tx, unfitted_points_rx) = mpsc::channel::<RecycleGuard<ReadOnlyBuffer<[Vector4<f32>]>>>();
        let unfitted_points_tx2 = unfitted_points_tx.clone();
        let points_fitter_builder;

        let robot_chain = Arc::new(Chain::<f64>::from_urdf_file("lunabot.urdf")?);

        let current_acceleration = Arc::new(AtomicCell::new(Vector3::default()));
        let current_acceleration2 = current_acceleration.clone();

        let mut drive_callbacks = DriveCallbacks::default();
        let lunasim_stdin = match &*lunabot_app.run_mode {
            RunMode::Simulation {
                lunasim_stdin,
                from_lunasim,
            } => {
                points_fitter_builder = BufferFitterBuilder {
                    point_count: 36 * 24,
                    iterations: 10,
                    max_translation: 0.1,
                    max_rotation: 0.1,
                    sample_count: 10,
                    distance_resolution: 0.01,
                };

                let depth_project =
                    Arc::new(CameraProjection::new(10.392, Vector2::new(36, 24), 0.01).block_on()?);
                // let lunasim_stdin2 = lunasim_stdin.clone();
                let camera_link = robot_chain.find_link("depth_camera_link").unwrap().clone();
                let robot_chain2 = robot_chain.clone();
                let buffer_recycler = Recycler::<ReadOnlyBuffer<[Vector4<f32>]>>::default();

                from_lunasim.add_fn(move |msg| match msg {
                    common::lunasim::FromLunasim::Accelerometer {
                        id: _,
                        acceleration,
                    } => {
                        let acceleration = Vector3::new(
                            acceleration[0] as f64,
                            acceleration[1] as f64,
                            acceleration[2] as f64,
                        );
                        current_acceleration2.store(acceleration);
                    }
                    common::lunasim::FromLunasim::Gyroscope { .. } => {}
                    common::lunasim::FromLunasim::DepthMap(depths) => {
                        let Some(camera_transform) = camera_link.world_transform() else {
                            return;
                        };
                        let mut points_buffer = if let Some(x) = buffer_recycler.get() {
                            x
                        } else {
                            match block_in_place(|| {ReadOnlyBuffer::new(DynamicSize::<Vector4<f32>>::new(36 * 24)).block_on()}) {
                                Ok(x) => buffer_recycler.associate(x),
                                Err(e) => {
                                    error!("Failed to create points buffer: {e}");
                                    return;
                                }
                            }
                        };
                        let depth_project2 = depth_project.clone();
                        let unfitted_points_tx2 = unfitted_points_tx2.clone();

                        get_tokio_handle().spawn(async move {
                            depth_project2
                                .project_buffer(&depths, camera_transform.cast(), &mut *points_buffer)
                                .await;
                            let _ = unfitted_points_tx2.send(points_buffer);
                        });
                    }
                    common::lunasim::FromLunasim::ExplicitApriltag {
                        robot_origin,
                        robot_axis,
                        robot_angle,
                    } => {
                        let robot_axis = UnitVector3::new_normalize(Vector3::new(
                            robot_axis[0] as f64,
                            robot_axis[1] as f64,
                            robot_axis[2] as f64,
                        ));
                        let isometry = Isometry3::from_parts(
                            Vector3::new(
                                robot_origin[0] as f64,
                                robot_origin[1] as f64,
                                robot_origin[2] as f64,
                            )
                            .into(),
                            UnitQuaternion::from_axis_angle(&robot_axis, robot_angle as f64),
                        );
                        robot_chain2.set_origin(isometry);
                        robot_chain2.update_transforms();
                    }
                });

                Some(lunasim_stdin.clone())
            }
            RunMode::Production => {
                // TODO match point count to depth camera
                points_fitter_builder = BufferFitterBuilder {
                    point_count: 36 * 24,
                    iterations: 10,
                    max_translation: 0.1,
                    max_rotation: 0.1,
                    sample_count: 10,
                    distance_resolution: 0.01,
                };
                None
            }
        };

        if let Some(lunasim_stdin) = lunasim_stdin.clone() {
            drive_callbacks.add_dyn_fn(Box::new(move |left, right| {
                FromLunasimbot::Drive {
                    left: left as f32,
                    right: right as f32,
                }
                .encode(|bytes| {
                    lunasim_stdin.write(bytes);
                });
            }));
        }

        let localizer = Localizer {
            robot_chain: robot_chain.clone(),
            lunasim_stdin: lunasim_stdin.clone(),
            acceleration: current_acceleration.clone(),
        };
        localizer.spawn();

        let points_fitter = points_fitter_builder.build(&[
            Plane {
                rotation_matrix: Rotation3::identity(),
                origin: Vector3::new(-1.5, 1.0, 0.5),
                size: Vector2::new(4.0, 2.0),
            },
            Plane {
                rotation_matrix: Rotation3::identity(),
                origin: Vector3::new(-1.5, 1.0, -7.5),
                size: Vector2::new(4.0, 2.0),
            },
            Plane {
                rotation_matrix: Rotation3::from_axis_angle(&UnitVector3::new_normalize(Vector3::new(0.0, 1.0, 0.0)), std::f32::consts::FRAC_PI_2),
                origin: Vector3::new(0.5, 1.0, -3.5),
                size: Vector2::new(8.0, 2.0),
            },
            Plane {
                rotation_matrix: Rotation3::from_axis_angle(&UnitVector3::new_normalize(Vector3::new(0.0, 1.0, 0.0)), std::f32::consts::FRAC_PI_2),
                origin: Vector3::new(-3.5, 1.0, -3.5),
                size: Vector2::new(8.0, 2.0),
            }
        ]).block_on()?;

        let fitted_points_callbacks = Arc::new(FittedPointsCallbacks::default());
        let fitted_points_callbacks_ref = fitted_points_callbacks.get_ref();

        std::thread::spawn(move || {
            let points_fitter = Arc::new(points_fitter);
            loop {
                let Ok(mut points) = unfitted_points_rx.recv() else {
                    break;
                };
                let points_fitter = points_fitter.clone();
                let fitted_points_callbacks = fitted_points_callbacks.clone();

                get_tokio_handle().spawn(async move {
                    if let Err(e) = points_fitter.fit_buffer(&mut *points).await {
                        error!("Failed to fit points: {e}");
                        return;
                    }
                    points.get_slice(|points| {
                        fitted_points_callbacks.call_immut(points);
                    }).await;
                });
            }
        });

        if let Some(lunasim_stdin) = lunasim_stdin.clone() {
            fitted_points_callbacks_ref.add_dyn_fn(Box::new(move |points| {
                let points = points.iter()
                    .filter_map(|point| {
                        if point[3] == 1.0 {
                            Some([point[0], point[1], point[2]])
                        } else {
                            None
                        }
                    })
                    .collect();
                let lunasim_stdin = lunasim_stdin.clone();
                spawn_blocking(move || {
                    FromLunasimbot::FittedPoints(points).encode(|bytes| {
                        lunasim_stdin.write(bytes);
                    });
                });
            }));
        }

        Ok(Self {
            special_instants: BinaryHeap::new(),
            lunabase_conn,
            from_lunabase,
            ping_timer: 0.0,
            drive_callbacks,
            // acceleration: current_acceleration,
            robot_chain,
            run_state: Some(RunState::new(lunabot_app)?),
            unfitted_points_tx,
            fitted_points_callbacks_ref,
        })
    }
    /// A special instant is an instant that the behavior tree will attempt
    /// to tick on regardless of the target delta.
    ///
    /// For example, if the target delta is 0.3 seconds, and a special
    /// instant was set to 1.05 seconds in the future from now, the
    /// behavior tree will tick at 0.3s, 0.6s, 0.9s, and 1.05s,
    /// then 1.35s, etc.
    pub fn add_special_instant(&mut self, instant: Instant) {
        self.special_instants.push(Reverse(instant));
    }

    pub(super) fn pop_special_instant(&mut self) -> Option<Instant> {
        self.special_instants.pop().map(|Reverse(instant)| instant)
    }

    pub(super) fn peek_special_instant(&mut self) -> Option<&Instant> {
        self.special_instants.peek().map(|Reverse(instant)| instant)
    }

    pub fn get_lunabase_conn(&self) -> &CakapSender {
        &self.lunabase_conn
    }

    pub fn poll_ping(&mut self, delta: f64) {
        self.ping_timer -= delta;
        if self.ping_timer <= 0.0 {
            self.ping_timer = PING_DELAY;
            FromLunabot::Ping.encode(|bytes| {
                let _ = self.get_lunabase_conn().send_unreliable(bytes);
            })
        }
    }

    pub fn on_get_msg_from_lunabase<T>(
        &mut self,
        duration: Duration,
        mut f: impl FnMut(&mut Self, FromLunabase) -> ControlFlow<T>,
    ) -> Option<T> {
        let deadline = Instant::now() + duration;
        loop {
            let Ok(msg) = self.from_lunabase.recv_deadline(deadline) else {
                break None;
            };
            match f(self, msg) {
                ControlFlow::Continue(()) => (),
                ControlFlow::Break(val) => break Some(val),
            }
        }
    }

    pub fn set_drive(&mut self, left: f64, right: f64) {
        self.drive_callbacks.call(left, right);
    }

    pub fn get_robot_chain(&self) -> Arc<Chain<f64>> {
        self.robot_chain.clone()
    }
}
