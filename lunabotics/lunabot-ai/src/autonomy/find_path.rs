use std::time::Duration;

use ares_bt::{
    action::AlwaysSucceed,
    branching::IfElse,
    converters::{AssertCancelSafe, Invert},
    looping::WhileLoop,
    sequence::Sequence,
    Behavior, CancelSafe, Status,
};
use common::{world_point_to_cell, LunabotStage, PathKind};
use nalgebra::Point3;
use tracing::warn;

use crate::{
    blackboard::{LunabotBlackboard, PathfindingState},
    Action,
};

use super::AutonomyState;

pub(super) fn find_path() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    IfElse::new(
        
        // exit if autonomy state is `None`
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            (blackboard.get_autonomy() != AutonomyState::None).into()
        }),
        
        // repeatedly (enqueue pathfind action, then wait for path) until success
        Invert(WhileLoop::new(
            AlwaysSucceed,
            Sequence::new((
                // pathfind
                AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    let robot_pos: Point3<f64> = blackboard.get_robot_isometry().translation.vector.into();
                    
                    let path_kind = match blackboard.get_autonomy() {
                        AutonomyState::Explore(_) => PathKind::MoveOntoTarget,
                        AutonomyState::MoveToDumpSite(_) => PathKind::ShovelAtTarget,
                        AutonomyState::MoveToDigSite(_) => PathKind::ShovelAtTarget,
                        other_state => panic!("trying to pathfind during autonomy state {other_state:?}")
                    };
                    
                    blackboard.request_for_path(
                        world_point_to_cell(robot_pos),
                        blackboard.get_pathfinding_target(),
                        path_kind
                    );
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
                                Status::Running
                            }
                        }
                        PathfindingState::Pending => {
                            Status::Running
                        }
                        PathfindingState::Failed => Status::Success
                    }
                }),
            )),
        )),
        
        AlwaysSucceed,
    )
}