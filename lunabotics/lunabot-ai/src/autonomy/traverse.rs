use ares_bt::{
    action::{AlwaysSucceed, RunOnce}, branching::IfElse, converters::AssertCancelSafe, sequence::Sequence, Behavior, CancelSafe, Status
};
use common::LunabotStage;

use crate::{blackboard::LunabotBlackboard, Action};

use super::{Autonomy, AutonomyStage};

pub(super) fn traverse() -> impl Behavior<LunabotBlackboard, Action> + CancelSafe {
    IfElse::new(
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            matches!(
                blackboard.get_autonomy(),
                Autonomy::FullAutonomy(AutonomyStage::TraverseObstacles)
                    | Autonomy::PartialAutonomy(AutonomyStage::TraverseObstacles)
            )
            .into()
        }),
        Sequence::new((
            RunOnce::from(|| Action::SetStage(LunabotStage::TraverseObstacles)),
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                blackboard.get_autonomy().advance();
                Status::Success
            }),
        )),
        AlwaysSucceed,
    )
}
