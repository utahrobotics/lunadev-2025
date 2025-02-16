use std::time::Duration;

use ares_bt::{
    action::{AlwaysFail, AlwaysSucceed},
    branching::IfElse,
    converters::{AssertCancelSafe, InfallibleShim},
    looping::WhileLoop,
    sequence::{ParallelAny, Sequence},
    Behavior, CancelSafe, Status,
};
use common::LunabotStage;
use nalgebra::Point3;
use tracing::{info, warn};

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
                IfElse::new(
                    ParallelAny::new((
                        Sequence::new((
                            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                                blackboard.calculate_path(
                                    blackboard.get_robot_isometry().translation.vector.into(),
                                    Point3::new(-2.0, 0.0, -7.0),
                                );
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
                        )),
                        Sequence::new((WaitBehavior::from(Duration::from_secs(4)), AlwaysFail)),
                    )),
                    AlwaysFail,
                    Sequence::new((
                        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                            info!("Scanning pause");
                            blackboard.enqueue_action(Action::SetSteering(Default::default()));
                            Status::Success
                        }),
                        WaitBehavior::from(Duration::from_secs(2)),
                    )),
                ),
            ),
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                blackboard.get_autonomy().advance();
                Status::Success
            }),
        )),
        AlwaysSucceed,
    )
}
