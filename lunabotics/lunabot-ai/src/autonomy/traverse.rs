use ares_bt::{action::AlwaysSucceed, branching::IfElse, converters::{AssertCancelSafe, Invert}, looping::WhileLoop, sequence::Sequence, Behavior, CancelSafe, Status};
use common::LunabotStage;

use crate::{Action, LunabotBlackboard};

use super::{find_path, follow_path, reset_steering};



pub(super) fn traverse() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
    // repeat until success
    Invert(WhileLoop::new(
        AlwaysSucceed,
        Invert(
            
            Sequence::new((
                reset_steering(), 
                find_path(),        // find_path() knows where to pathfind to based on autonomy state 
                follow_path(),
            ))
            
        ),
    ))
}