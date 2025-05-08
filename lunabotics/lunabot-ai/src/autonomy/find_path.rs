use ares_bt::{
    converters::AssertCancelSafe,
    sequence::Sequence,
    Behavior, CancelSafe, Status,
};
use common::{world_point_to_cell, PathKind};

use crate::blackboard::{LunabotBlackboard, PathfindingState};

use super::AutonomyState;


pub(super) fn find_path() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
    Sequence::new((
        
        // only continue if current path is none
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| blackboard.get_path().is_none().into()),
        
        // request for pathfind
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            
            let pathkind = match blackboard.get_autonomy_state() {
                AutonomyState::ToExcavationZone => PathKind::MoveOntoTarget,
                _ => PathKind::StopInFrontOfTarget,
            };
            
            blackboard.request_for_path(
                world_point_to_cell(blackboard.get_robot_pos()),
                blackboard.get_target_cell().unwrap_or_else(|| panic!("tried to pathfind while autonomy state is none")),
                pathkind
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
            }
        }),
    ))
}