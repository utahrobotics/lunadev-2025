use actions::dump;
use ares_bt::{
    action::AlwaysSucceed,
    branching::{IfElse, TryCatch},
    converters::{AssertCancelSafe, Invert},
    looping::WhileLoop,
    sequence::{ParallelAny, Sequence},
    Behavior, CancelSafe, Status,
};
use common::{FromLunabase, LunabotStage};
use find_path::find_path;
use follow_path::follow_path;
use nalgebra::Point2;
use tracing::{error, warn};
use traverse::traverse;

use crate::{blackboard::LunabotBlackboard, Action};

mod actions;
mod find_path;
mod follow_path;
mod move_actuator;
mod traverse;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AutonomyState {
    ToExcavationZone(Point2<f64>),
    Dump(Point2<f64>),
    None,
}

pub fn autonomy() -> impl Behavior<LunabotBlackboard> {
    ParallelAny::new((
        // exit autonomy upon interruption
        fail_if_autonomy_interrupted(),
        IfElse::new(
            autonomy_is_active(),
            TryCatch::new(
                Sequence::new((
                    // repeat until success
                    Invert(WhileLoop::new(
                        AlwaysSucceed,
                        Invert(IfElse::new(going_to_excavation_zone(), traverse(), dump())),
                    )),
                    AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                        warn!("finished {:?}", blackboard.get_autonomy_state());
                        blackboard.set_autonomy_state(AutonomyState::None);
                        Status::Success
                    }),
                )),
                AlwaysSucceed,
            ),
            AlwaysSucceed,
        ),
    ))
}

fn going_to_excavation_zone() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
        (matches!(
            blackboard.get_autonomy_state(),
            AutonomyState::ToExcavationZone(_)
        ))
        .into()
    })
}
fn autonomy_is_active() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
        (blackboard.get_autonomy_state() != AutonomyState::None).into()
    })
}

/// stop if received stop or steering input
fn fail_if_autonomy_interrupted() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
        if *blackboard.lunabase_disconnected() {
            error!("Lunabase disconnected");
            return Status::Failure;
        }
        while let Some(msg) = blackboard.peek_from_lunabase() {
            match msg {
                FromLunabase::Steering(steering) => {
                    let (left, right) = steering.get_left_and_right();
                    blackboard.set_autonomy_state(AutonomyState::None);
                    warn!("Received steering message while in autonomy mode {left} {right}");
                    return Status::Success;
                }
                FromLunabase::SoftStop => {
                    blackboard.pop_from_lunabase();
                    return Status::Failure;
                }
                _ => blackboard.pop_from_lunabase(),
            };
        }
        Status::Running
    })
}

fn reset_steering() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    // reset
    AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
        warn!("Traversing obstacles");
        blackboard.enqueue_action(Action::SetSteering(Default::default()));
        blackboard.enqueue_action(Action::SetStage(LunabotStage::Autonomy));
        Status::Success
    })
}
