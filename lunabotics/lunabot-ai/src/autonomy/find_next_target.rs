use ares_bt::{action::AlwaysFail, converters::{AssertCancelSafe, Invert}, sequence::Sequence, Behavior, CancelSafe, Status};
use common::{world_point_to_cell, CellsRect};
use nalgebra::Point3;

use crate::{autonomy::AutonomyState, blackboard::CheckIfExploredState, LunabotBlackboard};



pub(super) fn find_next_target() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
    Invert(Sequence::new((
        
        // start exploration check
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            blackboard.check_if_explored(CellsRect::new((4., 3.), 4., 2.));
            Status::Success
        }),
        
        // wait for exploration check to finish
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            match blackboard.exploring_state() {
                CheckIfExploredState::FinishedExploring => Status::Success,
                CheckIfExploredState::HaveToCheck => panic!("waiting for exploration check to finish when it hasn't started yet"),
                CheckIfExploredState::Pending => Status::Running,
                CheckIfExploredState::NeedToExplore(unexplored_cell) => {
                    println!("still need to explore {unexplored_cell:?}", );
                    blackboard.set_autonomy(AutonomyState::Explore(unexplored_cell));
                    
                    // if theres a cell that must be explored, don't go on to decide a dig/dump point
                    Status::Failure
                }, 
            }
        }),
        
        // decide next dig/dump site
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            let prev_state = blackboard.get_autonomy();
            match prev_state {
                AutonomyState::Dump | AutonomyState::Start => {
                    *blackboard.get_path_mut() = None;
                    
                    let site = find_next_dig_site();
                    println!("next dig site: {:?}", site);
                    blackboard.set_autonomy(AutonomyState::MoveToDigSite(site))
                }
                AutonomyState::Dig => {
                    *blackboard.get_path_mut() = None;
                    
                    let site = find_next_dump_site();
                    println!("next dump site: {:?}", site);
                    blackboard.set_autonomy(AutonomyState::MoveToDumpSite(site))
                }
                _ => {}
            }
            
            Status::Success
        }),
        
        AlwaysFail // fail so that outer `Invert()` returns success
    )))
}

// TODO
fn find_next_dig_site() -> (usize, usize) {
    world_point_to_cell(Point3::new(2., 0., 2.))
}

// TODO
fn find_next_dump_site() -> (usize, usize) {
    world_point_to_cell(Point3::new(2., 0., 5.))
}