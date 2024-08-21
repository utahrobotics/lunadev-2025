use bonsai_bt::Status;
use common::{lunasim::FromLunasimbot, FromLunabase, FromLunabot};
use crossbeam::atomic::AtomicCell;
use fitter::utils::CameraProjection;
use k::Chain;
use nalgebra::{Vector2, Vector3};
use urobotics::{
    callbacks::caller::try_drop_this_callback, define_callbacks, get_tokio_handle, log::{error, info}, task::SyncTask, tokio::task::block_in_place, BlockOn
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

use crate::{localization::Localizer, run::RunState, LunabotApp, RunMode};

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

impl std::fmt::Debug for DriveCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriveCallbacks").finish()
    }
}
// define_callbacks!(Vector3Callbacks => Fn(vec3: Vector3<f64>) + Send);

// impl std::fmt::Debug for Vector3Callbacks {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.debug_struct("Vector3Callbacks").finish()
//     }
// }

#[derive(Debug)]
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
        
        let current_acceleration = Arc::new(AtomicCell::new(Vector3::default()));
        let current_acceleration2 = current_acceleration.clone();

        let mut drive_callbacks = DriveCallbacks::default();
        let lunasim_stdin = match &*lunabot_app.run_mode {
            RunMode::Simulation {
                lunasim_stdin,
                from_lunasim,
            } => {
                let depth_project = Arc::new(CameraProjection::new(10.39, Vector2::new(36, 24), 0.01).block_on()?);
                let lunasim_stdin2 = lunasim_stdin.clone();
                let robot_chain2 = robot_chain.clone();

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
                        let depth_project2 = depth_project.clone();
                        let lunasim_stdin2 = lunasim_stdin2.clone();
                        let robot_chain2 = robot_chain2.clone();

                        get_tokio_handle().spawn(async move {
                            depth_project2.project(&depths, robot_chain2.origin().cast(), |points| {
                                let points: Box<[_]> = points.iter()
                                    .filter_map(|p| {
                                        if p.w == 1.0 {
                                            Some([p.x, p.y, p.z])
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                // TODO Fit points
                                FromLunasimbot::FittedPoints(points).encode(|bytes| {
                                    block_in_place(|| {
                                        lunasim_stdin2.write(bytes);
                                    });
                                });
                            }).await;
                        });
                    }
                });

                Some(lunasim_stdin.clone())
            }
            _ => None,
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

        Ok(Self {
            special_instants: BinaryHeap::new(),
            lunabase_conn,
            from_lunabase,
            ping_timer: 0.0,
            drive_callbacks,
            // acceleration: current_acceleration,
            robot_chain,
            run_state: Some(RunState::new(lunabot_app)?),
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
