use std::time::Duration;

use ares_bt::{sequence::Sequence, Behavior, Status};
use common::{FromLunabase, LunabotStage, Steering};
use tracing::{error, warn};

use crate::{
    autonomy::{Autonomy, AutonomyStage},
    blackboard::LunabotBlackboard,
    Action, PollWhen,
};

pub fn teleop() -> impl Behavior<LunabotBlackboard> {
    let mut last_steering = Steering::default();
    let mut last_lift_actuator = None;
    let mut last_bucket_actuator = None;

    Sequence::new((
        |blackboard: &mut LunabotBlackboard| {
            blackboard.enqueue_action(Action::SetStage(LunabotStage::TeleOp));
            Status::Success
        },
        move |blackboard: &mut LunabotBlackboard| {
            if *blackboard.lunabase_disconnected() {
                error!("Lunabase disconnected");
                return Status::Failure;
            }
            macro_rules! handle {
                ($msg: ident) => {
                    match $msg {
                        FromLunabase::Steering(steering) => {
                            last_steering = steering;
                        }
                        FromLunabase::LiftActuators(_) => {
                            last_lift_actuator = $msg.get_lift_actuator_commands();
                        }
                        FromLunabase::BucketActuators(_) => {
                            last_bucket_actuator = $msg.get_bucket_actuator_commands();
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
                };
            }
            // if let Some(msg) = blackboard.pop_from_lunabase() {
            //     handle!(msg);
            // } else {
            //     blackboard.enqueue_action(Action::SetSteering(last_steering));
            //     *blackboard.get_poll_when() =
            //         PollWhen::Instant(blackboard.get_now() + Duration::from_millis(90));
            //     return Status::Running;
            // }
            while let Some(msg) = blackboard.pop_from_lunabase() {
                handle!(msg);
            }
            blackboard.enqueue_action(Action::SetSteering(last_steering));
            if let Some([a, b]) = last_lift_actuator {
                blackboard.enqueue_action(Action::SetActuators(a));
                blackboard.enqueue_action(Action::SetActuators(b));
            }
            if let Some([a, b]) = last_bucket_actuator {
                blackboard.enqueue_action(Action::SetActuators(a));
                blackboard.enqueue_action(Action::SetActuators(b));
            }
            *blackboard.get_poll_when() =
                PollWhen::Instant(blackboard.get_now() + Duration::from_millis(90));
            Status::Running
        },
    ))
}
