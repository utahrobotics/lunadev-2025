use bonsai_bt::Status;
use common::{FromLunabase, FromLunabot};
use urobotics::{
    callbacks::caller::try_drop_this_callback,
    log::{error, info},
    BlockOn,
};

use std::{
    cmp::Reverse,
    collections::BinaryHeap,
    net::SocketAddr,
    ops::ControlFlow,
    sync::mpsc,
    time::{Duration, Instant},
};

use cakap::{CakapSender, CakapSocket};

use crate::{run::RunState, LunabotApp};

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

#[derive(Debug)]
pub struct Blackboard {
    special_instants: BinaryHeap<Reverse<Instant>>,
    lunabase_conn: CakapSender,
    from_lunabase: mpsc::Receiver<FromLunabase>,
    ping_timer: f64,
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

        Ok(Self {
            special_instants: BinaryHeap::new(),
            lunabase_conn,
            from_lunabase,
            ping_timer: 0.0,
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
        &self,
        duration: Duration,
        mut f: impl FnMut(FromLunabase) -> ControlFlow<T>,
    ) -> Option<T> {
        let deadline = Instant::now() + duration;
        loop {
            let Ok(msg) = self.from_lunabase.recv_deadline(deadline) else {
                break None;
            };
            match f(msg) {
                ControlFlow::Continue(()) => (),
                ControlFlow::Break(val) => break Some(val),
            }
        }
    }
}
