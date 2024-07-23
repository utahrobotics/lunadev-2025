use bonsai_bt::Status;
use urobotics::log::{error, get_program_time, info};

use crate::blackboard::Blackboard;

pub(super) fn run(bb: &mut Option<Blackboard>, dt: f64, first_time: bool) -> (Status, f64) {
    if first_time {
        info!("Entered Run");
    }
    let Some(_bb) = bb else {
        error!("Blackboard is null");
        return (Status::Failure, dt);
    };
    if get_program_time().as_secs_f64() > 3.0 {
        error!("Encountered scheduled error");
        (Status::Failure, 0.0)
    } else {
        (Status::Running, 0.0)
    }
}
