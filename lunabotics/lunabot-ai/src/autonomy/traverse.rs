use std::time::Duration;

use ares_bt::{
    action::AlwaysSucceed,
    branching::IfElse,
    converters::{AssertCancelSafe, Invert},
    looping::WhileLoop,
    sequence::Sequence,
    Behavior, CancelSafe, Status,
};
use common::LunabotStage;
use nalgebra::Point3;
use tracing::warn;

use crate::{blackboard::LunabotBlackboard, utils::WaitBehavior, Action};

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
            WhileLoop::new(
                AlwaysSucceed,
                Invert(Sequence::new((
                    // pathfind
                    AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                        blackboard.calculate_path(
                            blackboard.get_robot_isometry().translation.vector.into(),
                            Point3::new(-2.0, 0.0, -7.0),
                        );
                        Status::Success
                    }),
                    // wait for path
                    AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                        if blackboard.get_path().is_some() {
                            Status::Success
                        } else {
                            Status::Running
                        }
                    }),
                    // follow path
                    AssertCancelSafe(follow_path),
                    // pause to ensure the robot isn't moving while scanning in the "pathfind" node
                    WaitBehavior::from(Duration::from_secs(1)),
                ))),
            ),
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                blackboard.get_autonomy().advance();
                Status::Success
            }),
        )),
        AlwaysSucceed,
    )
}
