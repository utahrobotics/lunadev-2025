use ares_bt::{action::AlwaysSucceed, converters::Invert, looping::WhileLoop, sequence::Sequence, Behavior, CancelSafe, RunningOnce};

use crate::LunabotBlackboard;

use super::{find_path, follow_path, reset_steering};



pub(super) fn traverse() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
    // repeat until success
    Invert(WhileLoop::new(
        AlwaysSucceed,
        Invert(
            Sequence::new((
                RunningOnce::default(),
                reset_steering(), 
                find_path(),        // find_path() knows where to pathfind to based on autonomy state 
                follow_path(),
            ))
        ),
    ))
}