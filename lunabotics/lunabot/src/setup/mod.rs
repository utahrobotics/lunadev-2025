use bonsai_bt::Status;
use common::{lunasim::FromLunasimbot, FromLunabase, FromLunabot};
use fitter::utils::CameraProjection;
use k::{Chain, Isometry3, UnitQuaternion};
use nalgebra::{UnitVector3, Vector2, Vector3, Vector4};
use urobotics::{
    callbacks::caller::try_drop_this_callback,
    define_callbacks, fn_alias, get_tokio_handle,
    log::{error, info},
    task::SyncTask,
    tokio::task::block_in_place,
    BlockOn,
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

use crate::{
    localization::{Localizer, LocalizerRef},
    run::RunState,
    utils::Recycler,
    LunabotApp, RunMode,
};

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
    type PointCloudCallbacksRef = CallbacksRef(&[Vector4<f32>]) + Send + Sync
}
define_callbacks!(PointCloudCallbacks => Fn(point_cloud: &[Vector4<f32>]) + Send + Sync);

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
    // raw_point_cloud_callbacks: PointCloudCallbacksRef,
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

        let robot_chain = Arc::new(Chain::<f64>::from_urdf_file("lunabot.urdf")?);

        let localizer_ref = LocalizerRef::default();
        let localizer_ref2 = localizer_ref.clone();

        let raw_pcl_callbacks = Arc::new(PointCloudCallbacks::default());
        let raw_pcl_callbacks_ref = raw_pcl_callbacks.get_ref();

        let mut drive_callbacks = DriveCallbacks::default();
        let lunasim_stdin = match &*lunabot_app.run_mode {
            RunMode::Simulation {
                lunasim_stdin,
                from_lunasim,
            } => {
                let depth_project =
                    Arc::new(CameraProjection::new(10.392, Vector2::new(36, 24), 0.01).block_on()?);
                // let lunasim_stdin2 = lunasim_stdin.clone();
                let camera_link = robot_chain.find_link("depth_camera_link").unwrap().clone();
                let points_buffer_recycler = Recycler::<Box<[Vector4<f32>]>>::default();

                let axis_angle = |axis: [f32; 3], angle: f32| {
                    let axis = UnitVector3::new_normalize(Vector3::new(
                        axis[0] as f64,
                        axis[1] as f64,
                        axis[2] as f64,
                    ));

                    UnitQuaternion::from_axis_angle(&axis, angle as f64)
                };

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
                        localizer_ref2.set_acceleration(acceleration);
                    }
                    common::lunasim::FromLunasim::Gyroscope { id: _, axis, angle } => {
                        localizer_ref2.set_angular_velocity(axis_angle(axis, angle));
                    }
                    common::lunasim::FromLunasim::DepthMap(depths) => {
                        let Some(camera_transform) = camera_link.world_transform() else {
                            return;
                        };
                        let mut points_buffer = points_buffer_recycler
                            .get_or_else(|| vec![Vector4::default(); 36 * 24].into_boxed_slice());
                        let depth_project = depth_project.clone();
                        let raw_pcl_callbacks = raw_pcl_callbacks.clone();

                        get_tokio_handle().spawn(async move {
                            depth_project
                                .project_buffer(
                                    &depths,
                                    camera_transform.cast(),
                                    &mut **points_buffer,
                                )
                                .await;
                            block_in_place(|| {
                                raw_pcl_callbacks.call_immut(&points_buffer);
                            });
                        });
                    }
                    common::lunasim::FromLunasim::ExplicitApriltag {
                        robot_origin,
                        robot_axis,
                        robot_angle,
                    } => {
                        let isometry = Isometry3::from_parts(
                            Vector3::new(
                                robot_origin[0] as f64,
                                robot_origin[1] as f64,
                                robot_origin[2] as f64,
                            )
                            .into(),
                            axis_angle(robot_axis, robot_angle),
                        );
                        localizer_ref2.set_april_tag_isometry(isometry);
                    }
                });

                Some(lunasim_stdin.clone())
            }
            RunMode::Production => None,
        };

        if let Some(lunasim_stdin) = lunasim_stdin.clone() {
            let lunasim_stdin2 = lunasim_stdin.clone();
            drive_callbacks.add_dyn_fn(Box::new(move |left, right| {
                FromLunasimbot::Drive {
                    left: left as f32,
                    right: right as f32,
                }
                .encode(|bytes| {
                    lunasim_stdin.write(bytes);
                });
            }));
            raw_pcl_callbacks_ref.add_dyn_fn(Box::new(move |point_cloud| {
                FromLunasimbot::PointCloud(point_cloud.iter().map(|p| [p.x, p.y, p.z]).collect())
                    .encode(|bytes| {
                        lunasim_stdin2.write(bytes);
                    });
            }));
        }

        let localizer = Localizer {
            robot_chain: robot_chain.clone(),
            lunasim_stdin: lunasim_stdin.clone(),
            localizer_ref,
        };
        localizer.spawn();

        Ok(Self {
            special_instants: BinaryHeap::new(),
            lunabase_conn,
            from_lunabase,
            ping_timer: 0.0,
            drive_callbacks,
            // acceleration: current_acceleration,
            robot_chain,
            run_state: Some(RunState::new(lunabot_app)?),
            // raw_point_cloud_callbacks: raw_pcl_callbacks_ref,
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
