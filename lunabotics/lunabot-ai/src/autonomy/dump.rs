use ares_bt::{
    action::AlwaysFail, branching::IfElse, converters::WithSubBlackboard, Behavior, Status,
};

use crate::{blackboard::LunabotBlackboard, Action};

use super::{Autonomy, AutonomyBlackboard, AutonomyStage};

pub(super) fn dump() -> impl Behavior<LunabotBlackboard, Action> {
    WithSubBlackboard::<_, AutonomyBlackboard>::from(IfElse::new(
        |blackboard: &mut AutonomyBlackboard| {
            matches!(
                blackboard.autonomy,
                Autonomy::FullAutonomy(AutonomyStage::Dump)
                    | Autonomy::PartialAutonomy(AutonomyStage::Dump)
            )
            .into()
        },
        |blackboard: &mut AutonomyBlackboard| {
            blackboard.autonomy.advance();
            Status::Success
        },
        AlwaysFail,
    ))
}
