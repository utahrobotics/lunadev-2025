use ares_bt::{sequence::Sequence, Behavior, Status};
use common::{FromLunabase, LunabotStage};
use log::{error, warn};

use crate::{
    autonomy::{Autonomy, AutonomyStage},
    blackboard::LunabotBlackboard,
    Action, PollWhen,
};

pub fn teleop() -> impl Behavior<LunabotBlackboard> {
    Sequence::new((
        |blackboard: &mut LunabotBlackboard| {
            blackboard.enqueue_action(Action::SetStage(LunabotStage::TeleOp));
            Status::Success
        },
        |blackboard: &mut LunabotBlackboard| {
            if *blackboard.lunabase_disconnected() {
                error!("Lunabase disconnected");
                return Status::Failure;
            }
            while let Some(msg) = blackboard.pop_from_lunabase() {
                match msg {
                    FromLunabase::Steering(steering) => {
                        blackboard.enqueue_action(Action::SetSteering(steering));
                        return Status::Running;
                    }
                    FromLunabase::SoftStop => {
                        warn!("Received SoftStop");
                        return Status::Failure;
                    }
                    FromLunabase::TraverseObstacles => {
                        *blackboard.get_autonomy() =
                            Autonomy::PartialAutonomy(AutonomyStage::TraverseObstacles);
                        return Status::Success;
                    }
                    _ => {}
                }
            }
            *blackboard.get_poll_when() = PollWhen::ReceivedLunabase;
            Status::Running
        },
    ))
}
