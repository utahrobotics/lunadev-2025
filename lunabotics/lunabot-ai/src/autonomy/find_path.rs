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
    
    Sequence::new((
        
        // continue only if current path is none
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| blackboard.get_path().is_none().into()),
        
        // request for pathfind
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            
            let (target, path_kind, fail_if_dest_is_known) = match blackboard.get_autonomy() {
                
                // if in explore state and the destination turns out to be known, 
                // pathfinder should send `Input::PathDestIsKnown`
                AutonomyState::Explore(cell) => (cell, PathKind::StopInFrontOfTarget, true),
                
                AutonomyState::MoveToDumpSite(cell) => (cell, PathKind::StopInFrontOfTarget, false),
                AutonomyState::MoveToDigSite(cell) => (cell, PathKind::StopInFrontOfTarget, false),
                
                other_state => {
                    warn!("trying to pathfind during autonomy state {other_state:?}. reset autonomy to Start");
                    blackboard.set_autonomy(AutonomyState::Start);
                    return Status::Failure
                }
            };
            
            blackboard.request_for_path(
                world_point_to_cell(blackboard.get_robot_pos()),
                target,
                path_kind,
                fail_if_dest_is_known
            );
            Status::Success
        }),
        
        // wait for path
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            match blackboard.pathfinding_state() {
                PathfindingState::Idle => {
                    if blackboard.get_path().is_some() {
                        Status::Success
                    } else {
                        Status::Running
                    }
                }
                PathfindingState::Pending => {
                    Status::Running
                }
                PathfindingState::Failed => Status::Failure,
                PathfindingState::PathDestIsKnown => {
                    blackboard.set_autonomy(AutonomyState::Start);
                    Status::Failure 
                }
            }
        }),
    ))
}