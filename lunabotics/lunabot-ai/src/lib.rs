use std::{sync::Arc, time::Instant};

use ares_bt::{
    action::{AlwaysSucceed, RunOnce},
    branching::TryCatch,
    converters::{CatchPanic, Invert},
    looping::WhileLoop,
    sequence::Sequence,
    EternalBehavior, FallibleStatus,
};
use autonomy::autonomy;
use blackboard::LunabotBlackboard;
use common::{FromLunabase, LunabotStage, Steering};
use k::Chain;
use log::warn;
use nalgebra::Point3;
use teleop::teleop;

mod autonomy;
mod blackboard;
mod teleop;

pub use blackboard::Input;

#[derive(Debug, Clone)]
pub enum Action {
    /// Wait indefinitely for a message from lunabase.
    WaitForLunabase,
    SetSteering(Steering),
    SetStage(LunabotStage),
    CalculatePath {
        from: Point3<f64>,
        to: Point3<f64>,
        into: Vec<Point3<f64>>,
    },
    /// Wait until the given instant for any input, otherwise poll the ai again.
    WaitUntil(Instant),
    PollAgain,
}

pub fn run_ai(chain: Arc<Chain<f64>>, mut on_action: impl FnMut(Action, &mut Vec<Input>)) {
    let mut blackboard = LunabotBlackboard::new(chain);
    let mut b = WhileLoop::new(
        AlwaysSucceed,
        Sequence::new((
            RunOnce::from(|| Action::SetStage(LunabotStage::SoftStop)),
            RunOnce::from(|| Action::SetSteering(Steering::default())),
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
                    FallibleStatus::Running(Action::WaitForLunabase)
                },
            )),
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
        on_action(b.run_eternal(&mut blackboard).unwrap(), &mut inputs);
        for input in inputs.drain(..) {
            blackboard.digest_input(input);
        }
    }
}
