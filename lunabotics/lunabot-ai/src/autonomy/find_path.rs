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
    blackboard::{self, LunabotBlackboard, PathfindingState},
    Action,
};

use super::AutonomyState;

pub(super) fn find_path() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
    IfElse::new(
        
        // only find path if current path is none
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| blackboard.get_path().is_none().into()),
        
        // repeatedly (enqueue pathfind action, then wait for path) until body returns failure
        Invert(WhileLoop::new(
            AlwaysSucceed,
            Sequence::new((
                // pathfind
                AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    let robot_pos: Point3<f64> = blackboard.get_robot_isometry().translation.vector.into();
                    
                    let (target, path_kind) = match blackboard.get_autonomy() {
                        AutonomyState::Explore(cell) => (cell, PathKind::MoveOntoTarget),
                        AutonomyState::MoveToDumpSite(cell) => (cell, PathKind::ShovelAtTarget),
                        AutonomyState::MoveToDigSite(cell) => (cell, PathKind::ShovelAtTarget),
                        other_state => panic!("trying to pathfind during autonomy state {other_state:?}")
                    };
                    blackboard.request_for_path(
                        world_point_to_cell(robot_pos),
                        target,
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
        
        AlwaysSucceed
    )
}