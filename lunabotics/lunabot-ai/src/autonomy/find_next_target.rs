use ares_bt::{action::{AlwaysFail, AlwaysSucceed}, branching::IfElse, converters::{AssertCancelSafe, Invert}, looping::WhileLoop, sequence::Sequence, Behavior, CancelSafe, Status};
use common::CellsRect;

use crate::{blackboard::{CheckIfExploredState, PathfindingState}, LunabotBlackboard};



pub(super) fn find_next_target() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
    
    
    Invert(Sequence::new((
        
        // start exploration check
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            match blackboard.exploring_state() {
                CheckIfExploredState::FinishedExploring => Status::Success,
                CheckIfExploredState::HaveToCheck => {
                    println!("exploration check: started", );
                    blackboard.check_if_explored(CellsRect::new((4., 3.), 4., 1.));
                    Status::Success
                }
                _ => Status::Failure
            }
        }),
        
        // wait for exploration check to finish
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            match blackboard.exploring_state() {
                CheckIfExploredState::HaveToCheck => panic!("waiting for exploration check to finish even though it hasnt started yet"),
                CheckIfExploredState::Pending => Status::Running,
                CheckIfExploredState::FinishedExploring => {
                    println!("exploration check: fully explored!", );
                    Status::Success
                },
                
                // if theres a cell that must be explored, don't go on to decide a dig/dump point
                CheckIfExploredState::NeedToExplore(_) => Status::Failure, 
            }
        }),
        
        // decide dig/dump point
        AssertCancelSafe(|_: &mut LunabotBlackboard| {
            println!("deciding a dig/dump point..");
            Status::Success
        }),
        
        AlwaysFail // fail so that outer `Invert()` returns success
    )))
}