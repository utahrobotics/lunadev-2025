use bonsai_bt::Status;
use urobotics::log::info;

use crate::blackboard::Blackboard;

pub(super) fn setup(bb: &mut Option<Blackboard>, dt: f64, first_time: bool) -> (Status, f64) {
    if first_time {
        info!("Entered Setup");
    }
    if let Some(_) = bb {
        // Review the existing blackboard for any necessary setup
        (Status::Success, dt)
    } else {
        // Create a new blackboard
        *bb = Some(Blackboard::default());
        (Status::Success, dt)
    }
}
