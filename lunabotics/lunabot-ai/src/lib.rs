use std::{time::Instant, vec};

use ares_bt::{
    action::AlwaysSucceed,
    branching::TryCatch,
    converters::{CatchPanic, Invert},
    looping::WhileLoop,
    sequence::Sequence,
    EternalBehavior, FallibleStatus, InfallibleStatus,
};
use autonomy::autonomy;
use blackboard::LunabotBlackboard;
use common::{CellsRect, FromLunabase, LunabotStage, PathKind, Steering};
use embedded_common::{Actuator, ActuatorCommand};
use simple_motion::StaticImmutableNode;
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
    SetActuators(ActuatorCommand),
    SetStage(LunabotStage),
    CalculatePath {
        from: (usize, usize),
        to: (usize, usize),
        kind: PathKind,
        fail_if_dest_is_known: bool
    },
    LiftShake,
    AvoidCell((usize, usize)),
    ClearPointsToAvoid,
    CheckIfExplored {
        area: CellsRect,
        robot_cell_pos: (usize, usize),
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
    chain: StaticImmutableNode,
    mut on_action: impl FnMut(Action, &mut Vec<Input>),
    mut polling: impl FnMut(PollWhen, &mut Vec<Input>),
) {
    let mut blackboard = LunabotBlackboard::new(chain);
    let mut b = WhileLoop::new(
        AlwaysSucceed,
        Sequence::new((
            
            // reset 
            |blackboard: &mut LunabotBlackboard| {
                blackboard.enqueue_action(Action::SetStage(LunabotStage::SoftStop));
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(0.0, Actuator::Lift)));
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(0.0, Actuator::Bucket)));
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::StopPercuss));
                blackboard.enqueue_action(Action::SetSteering(Steering::default()));
                InfallibleStatus::Success
            },
            
            // wait until receive "continue mission" from lunabase
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
            
            
            TryCatch::new(
                WhileLoop::new(
                    AlwaysSucceed,
                    Sequence::new(( CatchPanic(teleop()), CatchPanic(autonomy()), )), 
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
