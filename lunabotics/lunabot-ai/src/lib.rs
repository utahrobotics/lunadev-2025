use std::{sync::Arc, time::Instant, vec};

use ares_bt::{
    action::{AlwaysFail, AlwaysSucceed},
    branching::TryCatch,
    converters::{CatchPanic, Invert},
    looping::WhileLoop,
    sequence::Sequence,
    EternalBehavior, FallibleStatus, InfallibleStatus,
};
use autonomy::autonomy;
use blackboard::LunabotBlackboard;
use common::{FromLunabase, LunabotStage, Steering};
use k::Chain;
use nalgebra::Point3;
use teleop::teleop;
use tracing::warn;

mod autonomy;
mod blackboard;
mod teleop;
mod utils;

pub use blackboard::Input;

#[derive(Debug, Clone)]
pub enum Action {
    SetSteering(Steering),
    SetStage(LunabotStage),
    CalculatePath {
        from: Point3<f64>,
        to: Point3<f64>,
        into: Vec<Point3<f64>>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum PollWhen {
    /// Wait indefinitely for a message from lunabase.
    ReceivedLunabase,
    /// Wait until the given instant for any input, otherwise poll the ai again.
    Instant(Instant),
    /// Poll instantly.
    NoDelay,
}

pub fn run_ai(
    chain: Arc<Chain<f64>>,
    mut on_action: impl FnMut(Action, &mut Vec<Input>),
    mut polling: impl FnMut(PollWhen, &mut Vec<Input>),
) {
    let mut blackboard = LunabotBlackboard::new(chain);
    let mut b = WhileLoop::new(
        AlwaysSucceed,
        Sequence::new((
            |blackboard: &mut LunabotBlackboard| {
                blackboard.enqueue_action(Action::SetStage(LunabotStage::SoftStop));
                blackboard.enqueue_action(Action::SetSteering(Steering::default()));
                InfallibleStatus::Success
            },
            Invert(WhileLoop::new(
                AlwaysSucceed,
                |blackboard: &mut LunabotBlackboard| {
                    while let Some(msg) = blackboard.pop_from_lunabase() {
                        match msg {
                            FromLunabase::ContinueMission => {
                                warn!("Continuing mission");
                                *blackboard.lunabase_disconnected() = false;
                                return FallibleStatus::Failure;
                            }
                            _ => {}
                        }
                    }
                    *blackboard.get_poll_when() = PollWhen::ReceivedLunabase;
                    FallibleStatus::Running
                },
            )),
            // follow_path,
            TryCatch::new(
                WhileLoop::new(
                    AlwaysSucceed,
                    Sequence::new((CatchPanic(teleop()), CatchPanic(autonomy()))),
                ),
                AlwaysSucceed,
            ),
        )),
    );

    let mut inputs = vec![];
    loop {
        blackboard.update_now();
        b.run_eternal(&mut blackboard);
        for action in blackboard.drain_actions() {
            std::thread::sleep(std::time::Duration::from_millis(16));
            on_action(action, &mut inputs);
        }
        for input in inputs.drain(..) {
            blackboard.digest_input(input);
        }
        polling(*blackboard.get_poll_when(), &mut inputs);
        *blackboard.get_poll_when() = PollWhen::NoDelay;
        for input in inputs.drain(..) {
            blackboard.digest_input(input);
        }
    }
}
