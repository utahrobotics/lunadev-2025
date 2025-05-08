use ares_bt::{converters::AssertCancelSafe, sequence::Sequence, Behavior, CancelSafe, Status};
use common::Obstacle;

use crate::{Action, LunabotBlackboard};

use super::traverse;


/// distance between shovel and center of robot 
const SHOVEL_DISTANCE_METERS: f64 = 0.3; // TODO set this to the actual value

/// adds a obstacle to `additional_obstacles` to prevent digging or moving over that spot again
/// 
/// to be used to avoid holes/mounds
fn add_hole_or_mound_obstacle(blackboard: &mut LunabotBlackboard) {
    let shovel_pos = (blackboard.get_robot_heading() * SHOVEL_DISTANCE_METERS) + blackboard.get_robot_pos().xz().coords;
    
    blackboard.enqueue_action(Action::AvoidObstacle(Obstacle::new_circle((shovel_pos.x, shovel_pos.y), 0.3)));
}

// pub(super) fn dig() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
//     Sequence::new((
//         AssertCancelSafe(
//             |blackboard: &mut LunabotBlackboard| {
//                 // TODO
                
//                 println!("digging!!"); 
                
                
                
//                 // add the hole we just dug to `additional_obstacles` to prevent digging or moving over that spot again
//                 add_hole_or_mound_obstacle(blackboard);
                
//                 Status::Success
//             }
//         ),
//     ))
// }


pub(super) fn dump() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
    Sequence::new((
        
        traverse(),     // knows to go to dump spot due to autonomy state
        
        AssertCancelSafe(
            |blackboard: &mut LunabotBlackboard| {
                // TODO move arms to dump 
                
                println!("dumping!!", );
                
                // add the mound we just dumped to `additional_obstacles` to prevent digging or moving over that spot again
                add_hole_or_mound_obstacle(blackboard);
                
                Status::Success
            }
        ),
    ))
}