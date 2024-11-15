use ares_bt::{
    action::AlwaysSucceed,
    branching::IfElse,
    converters::AssertCancelSafe,
    sequence::Sequence,
    Behavior, CancelSafe, Status,
};
use common::LunabotStage;

use crate::{blackboard::LunabotBlackboard, Action};

use super::{Autonomy, AutonomyStage};

pub(super) fn dig() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    IfElse::new(
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            matches!(
                blackboard.get_autonomy(),
                Autonomy::FullAutonomy(AutonomyStage::Dig)
                    | Autonomy::PartialAutonomy(AutonomyStage::Dig)
            )
            .into()
        }),
        Sequence::new((
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                blackboard.enqueue_action(Action::SetStage(LunabotStage::Dig));
                Status::Success
            }),
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                blackboard.get_autonomy().advance();
                Status::Success
            }),
        )),
        AlwaysSucceed,
    )
}
