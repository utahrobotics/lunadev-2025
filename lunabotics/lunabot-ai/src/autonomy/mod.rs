
use std::time::Duration;

use ares_bt::{
    branching::TryCatch, converters::{AssertCancelSafe, Invert}, looping::WhileLoop, sequence::{ParallelAny, Sequence}, Behavior, CancelSafe, Status
};
use common::{FromLunabase, LunabotStage};
use tracing::{error, warn};
use find_next_target::find_next_target;
use find_path::find_path;
use follow_path::follow_path;

use crate::{blackboard::LunabotBlackboard, utils::WaitBehavior, Action};

mod find_next_target;
mod find_path;
mod follow_path;

const PAUSE_AFTER_MOVING_DURATION: Duration = Duration::from_secs(2);


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutonomyState {
    StartAutonomy,
    Explore((usize, usize)),
    MoveToDumpSite((usize, usize)),
    MoveToDigSite((usize, usize)),
    Dump,
    Dig,
    None,
}


pub fn autonomy() -> impl Behavior<LunabotBlackboard> {
    WhileLoop::new(
        |blackboard: &mut LunabotBlackboard| (blackboard.get_autonomy() != AutonomyState::None).into(),
        ParallelAny::new((
            
            // exit loop upon interruption
            fail_if_autonomy_interrupted(),
            
            Sequence::new(( 
                reset_steering(), 
                find_next_target(), 
                find_path(), 
                
                do_then_wait(
                    AssertCancelSafe(follow_path), 
                    PAUSE_AFTER_MOVING_DURATION
                ),
            )),
        )),
    )
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
                FromLunabase::Steering(_) => {
                    blackboard.set_autonomy(AutonomyState::None);
                    warn!("Received steering message while in autonomy mode");
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
        blackboard.enqueue_action(Action::SetStage(LunabotStage::TraverseObstacles));
        Status::Success
    })
}

/// after `behavior` ends, waits for `duration` then returns the same status that `behavior` did
fn do_then_wait(behavior: impl Behavior<LunabotBlackboard> + CancelSafe, duration: Duration) -> impl Behavior<LunabotBlackboard> + CancelSafe
{
    TryCatch::new(
        Sequence::new((
            behavior,
            WaitBehavior::from(duration),
        )),
        Invert(WaitBehavior::from(duration)),
    )
}