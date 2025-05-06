use ares_bt::{
    action::AlwaysSucceed, branching::{IfElse, TryCatch}, converters::{AssertCancelSafe, Invert}, looping::WhileLoop, sequence::{ParallelAny, Sequence}, Behavior, CancelSafe, Status
};
use common::{FromLunabase, LunabotStage};
use tracing::{error, warn};
use find_next_target::find_next_target;
use find_path::find_path;
use follow_path::follow_path;
use actions::{dig, dump};

use crate::{blackboard::{self, CheckIfExploredState, LunabotBlackboard}, utils::WaitBehavior, Action};

mod find_next_target;
mod find_path;
mod follow_path;
mod actions;



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
                        
                        // repeat until successfully moved to target position
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
                            finished_exploring(),
                            IfElse::new(
                                autonomy_state_is_dig(), 
                                dig(), 
                                dump()
                            ),
                            AlwaysSucceed
                        )
                    )),
                    AlwaysSucceed
                )
            ),
        )),
    )
}

pub fn finished_exploring() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    AssertCancelSafe(
        |blackboard: &mut LunabotBlackboard| {
            (blackboard.exploring_state() == CheckIfExploredState::FinishedExploring).into()
        }
    )
}
fn autonomy_state_is_dig() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    AssertCancelSafe(
        |blackboard: &mut LunabotBlackboard| {
            (blackboard.get_autonomy_state() == AutonomyState::Dig).into()
        }
    )
}
fn autonomy_is_active() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    AssertCancelSafe(
        |blackboard: &mut LunabotBlackboard| {
            (blackboard.get_autonomy_state() != AutonomyState::None).into()
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
                    blackboard.set_autonomy_state(AutonomyState::None);
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
        println!("reset steering!!!", );
        warn!("Traversing obstacles");
        blackboard.enqueue_action(Action::SetSteering(Default::default()));
        blackboard.enqueue_action(Action::SetStage(LunabotStage::Autonomy));
        Status::Success
    })
}

