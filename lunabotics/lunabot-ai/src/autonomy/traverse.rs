use ares_bt::{
    action::AlwaysSucceed, branching::IfElse, converters::{AssertCancelSafe, InfallibleShim}, sequence::Sequence,
    Behavior, CancelSafe, Status,
};
use common::LunabotStage;
use log::warn;
use nalgebra::Point3;

use crate::{blackboard::LunabotBlackboard, Action, PollWhen};

use super::{follow_path, Autonomy, AutonomyStage};

pub(super) fn traverse() -> impl Behavior<LunabotBlackboard> + CancelSafe {
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
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                warn!("Traversing obstacles");
                blackboard.enqueue_action(Action::SetSteering(Default::default()));
                blackboard.enqueue_action(Action::SetStage(LunabotStage::TraverseObstacles));
                Status::Success
            }),
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                blackboard.calculate_path(blackboard.get_robot_isometry().translation.vector.into(), Point3::new(-3.0, 0.0, -6.0));
                Status::Success
            }),
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                if blackboard.get_path().is_some() {
                    Status::Success
                } else {
                    Status::Running
                }
            }),
            InfallibleShim(AssertCancelSafe(follow_path)),
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                blackboard.get_autonomy().advance();
                Status::Success
            }),
        )),
        AlwaysSucceed,
    )
}
