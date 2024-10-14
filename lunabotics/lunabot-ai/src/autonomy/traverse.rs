use ares_bt::{action::AlwaysSucceed, branching::IfElse, Behavior, Status};

use crate::{blackboard::LunabotBlackboard, Action};

use super::{Autonomy, AutonomyStage};

pub(super) fn traverse() -> impl Behavior<LunabotBlackboard, Action> {
    IfElse::new(
        |blackboard: &mut LunabotBlackboard| {
            matches!(
                blackboard.get_autonomy(),
                Autonomy::FullAutonomy(AutonomyStage::TraverseObstacles)
                    | Autonomy::PartialAutonomy(AutonomyStage::TraverseObstacles)
            )
            .into()
        },
        |blackboard: &mut LunabotBlackboard| {
            while let Some(msg) = blackboard.pop_from_lunabase() {
                match msg {
                    common::FromLunabase::SoftStop => return Status::Failure,
                    _ => {}
                }
            }
            blackboard.get_autonomy().advance();
            Status::Success
        },
        AlwaysSucceed,
    )
}
