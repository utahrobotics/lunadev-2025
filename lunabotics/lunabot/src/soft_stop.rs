use bonsai_bt::Status;
use urobotics::log::info;

use crate::blackboard::Blackboard;

pub(super) fn soft_stop(bb: &mut Option<Blackboard>, dt: f64, first_time: bool) -> (Status, f64) {
    if first_time {
        info!("Entered SoftStop")
    }
    if let Some(_) = bb {
        if first_time {
            info!("Blackboard present, awaiting lunabase command");
        }
        // We have a connection to lunabase, so wait for commands
        // Operator may request Failure to trigger setup again
        // or Success to trigger run
        (Status::Running, 0.0)
    } else {
        if first_time {
            info!("No blackboard, so must trigger setup");
        }
        // No connection, so must trigger setup to form connection with lunabase
        (Status::Failure, dt)
    }
}
