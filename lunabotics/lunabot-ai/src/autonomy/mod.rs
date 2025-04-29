
use std::time::Duration;

use ares_bt::{
    action::{AlwaysRunning, AlwaysSucceed}, branching::{IfElse, TryCatch}, converters::{AssertCancelSafe, Invert}, looping::WhileLoop, sequence::{ParallelAny, Sequence}, Behavior, CancelSafe, Status
};
use common::{FromLunabase, LunabotStage};
use tracing::{error, warn};
use find_next_target::find_next_target;
use find_path::find_path;
use follow_path::follow_path;
use dig::dig;
use dump::dump;

use crate::{blackboard::{self, LunabotBlackboard}, utils::WaitBehavior, Action};

mod find_next_target;
mod find_path;
mod follow_path;
mod dig;
mod dump;



#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutonomyState {
    Start,
    Explore((usize, usize)),
    MoveToDumpSite((usize, usize)),
    MoveToDigSite((usize, usize)),
    Dump,
    Dig,
    None,
}


pub fn autonomy() -> impl Behavior<LunabotBlackboard> {
    
    WhileLoop::new(
        autonomy_is_active(),
        
        ParallelAny::new((
            
            // exit autonomy upon interruption
            fail_if_autonomy_interrupted(),
            
            WhileLoop::new(
                autonomy_is_active(),
                
                TryCatch::new(
                    Sequence::new((
                        
                        // repeat until body returns success
                        Invert(WhileLoop::new(
                            AlwaysSucceed,
                            Invert(
                                
                                Sequence::new((
                                    reset_steering(), 
                                    find_next_target(), 
                                    find_path(), 
                                    follow_path(),
                                ))
                                
                            ),
                        )),
                        
                        IfElse::new(
                            autonomy_state_is_dig(), 
                            dig(), 
                            dump()
                        ),
                    )),
                    AlwaysSucceed
                )
            ),
        )),
    )
}

fn autonomy_state_is_dig() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    AssertCancelSafe(
        |blackboard: &mut LunabotBlackboard| {
            (blackboard.get_autonomy() == AutonomyState::Dig).into()
        }
    )
}
fn autonomy_is_active() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    AssertCancelSafe(
        |blackboard: &mut LunabotBlackboard| {
            (blackboard.get_autonomy() != AutonomyState::None).into()
        }
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

