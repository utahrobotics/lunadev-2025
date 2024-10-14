use ares_bt::{
    action::{AlwaysSucceed, RunOnce},
    branching::IfElse,
    sequence::Sequence,
    Behavior, Status,
};
use common::LunabotStage;

use crate::{blackboard::LunabotBlackboard, Action};

use super::{Autonomy, AutonomyStage};

pub(super) fn dump() -> impl Behavior<LunabotBlackboard, Action> {
    IfElse::new(
        |blackboard: &mut LunabotBlackboard| {
            matches!(
                blackboard.get_autonomy(),
                Autonomy::FullAutonomy(AutonomyStage::Dump)
                    | Autonomy::PartialAutonomy(AutonomyStage::Dump)
            )
            .into()
        },
        Sequence::new((
            RunOnce::from(Action::SetStage(LunabotStage::Dump)),
            |blackboard: &mut LunabotBlackboard| {
                blackboard.get_autonomy().advance();
                Status::Success
            },
        )),
        AlwaysSucceed,
    )
}
