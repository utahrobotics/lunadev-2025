use ares_bt::{action::RunOnce, sequence::Sequence, Behavior, Status};
use common::{FromLunabase, LunabotStage};
use log::warn;

use crate::{
    autonomy::{Autonomy, AutonomyStage},
    blackboard::LunabotBlackboard,
    Action,
};

pub fn teleop() -> impl Behavior<LunabotBlackboard, Action> {
    Sequence::new((
        RunOnce::from(|| Action::SetStage(LunabotStage::TeleOp)),
        |blackboard: &mut LunabotBlackboard| {
            while let Some(msg) = blackboard.pop_from_lunabase() {
                match msg {
                    FromLunabase::Steering(steering) => {
                        return Status::Running(Action::SetSteering(steering));
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
            Status::Running(Action::WaitForLunabase)
        },
    ))
}
