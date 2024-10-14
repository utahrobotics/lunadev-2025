use ares_bt::{
    action::{AlwaysSucceed, RunOnce},
    branching::IfElse,
    sequence::Sequence,
    Behavior, Status,
};
use common::{FromLunabase, LunabotStage};

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
        Sequence::new((
            RunOnce::from(Action::SetStage(LunabotStage::TraverseObstacles)),
            |blackboard: &mut LunabotBlackboard| {
                while let Some(msg) = blackboard.pop_from_lunabase() {
                    match msg {
                        FromLunabase::SoftStop => return Status::Failure,
                        FromLunabase::Steering(_) => return Status::Success,
                        _ => {}
                    }
                }
                blackboard.get_autonomy().advance();
                Status::Success
            },
        )),
        AlwaysSucceed,
    )
}
