use ares_bt::{converters::AssertCancelSafe, sequence::Sequence, Behavior, CancelSafe, Status};

use crate::LunabotBlackboard;


pub(super) fn dig() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
    Sequence::new((
        AssertCancelSafe(
            |_: &mut LunabotBlackboard| {
                // TODO
                
                println!("digging!!", ); 
                Status::Success
            }
        ),
    ))
}