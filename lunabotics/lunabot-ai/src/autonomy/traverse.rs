use std::time::Duration;

use ares_bt::{
    action::AlwaysSucceed,
    branching::{IfElse, TryCatch},
    converters::{AssertCancelSafe, Invert},
    looping::WhileLoop,
    sequence::Sequence,
    Behavior, CancelSafe, Status,
};
use common::LunabotStage;
use nalgebra::Point3;
use tracing::warn;

use crate::{
    blackboard::{LunabotBlackboard, PathfindingState},
    utils::WaitBehavior,
    Action, PollWhen,
};

use super::{follow_path, Autonomy, AutonomyStage};

const PAUSE_AFTER_MOVING_DURATION: Duration = Duration::from_secs(2);

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
                    Invert(WhileLoop::new(
                        AlwaysSucceed,
                        Sequence::new((
                            // pathfind
                            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                                let target = *blackboard.get_target_mut();
                                blackboard.calculate_path(
                                    blackboard.get_robot_isometry().translation.vector.into(),
                                    target
                                );
                                *blackboard.get_poll_when() = PollWhen::ReceivedLunabase;
                                Status::Success
                            }),
                            // wait for path
                            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                                match blackboard.pathfinding_state() {
                                    PathfindingState::Idle => {
                                        if blackboard.get_path().is_some() {
                                            // failure breaks the loop
                                            Status::Failure
                                        } else {
                                            *blackboard.get_poll_when() = PollWhen::ReceivedLunabase;
                                            Status::Running
                                        }
                                    }
                                    PathfindingState::Pending => {
                                        *blackboard.get_poll_when() = PollWhen::ReceivedLunabase;
                                        Status::Running
                                    }
                                    PathfindingState::Failed => Status::Success
                                }
                            }),
                        )),
                    )),
                    // AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    //     match blackboard.pathfinding_state() {
                    //         PathfindingState::Idle => {
                    //             if blackboard.get_path().is_some() {
                    //                 // failure breaks the loop
                    //                 Status::Failure
                    //             } else {
                    //                 Status::Running
                    //             }
                    //         }
                    //         PathfindingState::Pending => unreachable!(),
                    //         PathfindingState::Failed => {
                    //             warn!("Pathfinding failed");
                    //             Status::Failure
                    //         }
                    //     }
                    // }),
                    // follow path, then pause regardless of result
                    TryCatch::new(
                        Sequence::new((
                            AssertCancelSafe(follow_path),
                            WaitBehavior::from(PAUSE_AFTER_MOVING_DURATION),
                        )),
                        Invert(WaitBehavior::from(PAUSE_AFTER_MOVING_DURATION)), // return false if `follow_path` returned false
                    ),
                    // TODO temporary
                    // if path following is successful, toggle next pathfinding target
                    AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                        let completed_target = blackboard.get_target_mut();
                        let dig_location = Point3::new(1.0, 0.0, 3.0);
                        let dump_location = Point3::new(3.0, 0.0, 4.0);
                        
                        if completed_target == &dig_location {
                            println!("now moving to dump location {}", dump_location);
                            *blackboard.get_target_mut() = dump_location;
                        }
                        else {
                            println!("now moving to dig location {}", dig_location);
                            *blackboard.get_target_mut() = dig_location;
                        }
                        
                        Status::Failure
                    })
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
