use ares_bt::{action::AlwaysFail, converters::{AssertCancelSafe, Invert}, sequence::Sequence, Behavior, CancelSafe, Status};
use common::{world_point_to_cell, CellsRect};
use nalgebra::Point3;

use crate::{autonomy::AutonomyState, blackboard::CheckIfExploredState, LunabotBlackboard};
use std::time::Duration;

use ares_bt::{action::{AlwaysFail, AlwaysSucceed}, branching::IfElse, converters::{AssertCancelSafe, Invert}, looping::WhileLoop, sequence::Sequence, Behavior, CancelSafe, Status};
use common::{world_point_to_cell, CellsRect, LunabotStage};
use nalgebra::{distance, Point3};
use tracing::warn;

use crate::{autonomy::AutonomyState, blackboard::{self, CheckIfExploredState, FindActionSiteState, PathfindingState}, utils::WaitBehavior, Action, LunabotBlackboard};

use super::finished_exploring;



pub(super) fn find_next_target() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
    Invert(Sequence::new((
        
        // only continue if autonomy state is Start, Dump, or Dig 
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            println!("find_next_target {:?}", blackboard.get_autonomy_state());
            match blackboard.get_autonomy_state() {
                AutonomyState::Start | AutonomyState::Dump | AutonomyState::Dig => Status::Success,
                _ => {
                    println!("failing");
                    Status::Failure
                }
            }
        }),
        
        // start exploration check if exploration hasnt finished
        IfElse::new(
            finished_exploring(),
            AlwaysSucceed, 
            Sequence::new((
                AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    blackboard.check_if_explored(CellsRect::new((4., 0.), 4., 4.)); // TODO set as actual exploration area
                    Status::Success
                }),
                
                // wait for exploration check to finish
                AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    match blackboard.exploring_state() {
                        CheckIfExploredState::FinishedExploring => {
                            println!("done exploring! {:?}", blackboard.get_autonomy_state());
                            Status::Success
                        },
                        CheckIfExploredState::HaveToCheck => panic!("waiting for exploration check to finish when it hasn't started yet"),
                        CheckIfExploredState::Pending => Status::Running,
                        CheckIfExploredState::NeedToExplore(unexplored_cell) => {
                            println!("still need to explore {unexplored_cell:?}", );
                            blackboard.set_autonomy_state(AutonomyState::Explore(unexplored_cell));
                            
                            // if theres a cell that must be explored, don't go on to decide a dig/dump point
                            Status::Failure
                        }, 
                    }
                }),
            ))
        ),
        
        // pause for 10 seconds if haven't already
        IfElse::new(
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| blackboard.finished_post_explore_pause().into()),
            AlwaysSucceed,
            Sequence::new((
                WaitBehavior::from(Duration::from_secs(10)),
                AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    println!("finished 10 sec pause", );
                    blackboard.set_finished_post_explore_pause(true);
                    Status::Success
                }),
            ))
        ),
        
        
        // start deciding next dig/dump site
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            *blackboard.get_path_mut() = None;
            
            let prev_state = blackboard.get_autonomy_state();
            match prev_state {
                AutonomyState::Dump | AutonomyState::Start => {
                    blackboard.find_next_dig_site();
                    Status::Success
                }
                AutonomyState::Dig => {
                    blackboard.find_next_dump_site();
                    Status::Success
                }
                _ => Status::Failure
            }
        }),
        
        // wait for finding next dig/dump site to finish
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            match blackboard.next_action_site_state() {
                FindActionSiteState::Start => panic!("waiting for finding action site to finish before its started"),
                FindActionSiteState::Pending => Status::Running,
                FindActionSiteState::FoundSite(cell) => {
                    let prev_state = blackboard.get_autonomy_state();
                    match prev_state {
                        AutonomyState::Dump | AutonomyState::Start => {
                            blackboard.set_autonomy_state(AutonomyState::MoveToDigSite(cell));
                        }
                        AutonomyState::Dig => {
                            blackboard.set_autonomy_state(AutonomyState::MoveToDumpSite(cell));
                        }
                        _ => {}
                    }
                    Status::Success
                },
                FindActionSiteState::NotFound => {
                    let prev_state = blackboard.get_autonomy_state();
                    match prev_state {
                        AutonomyState::Dump | AutonomyState::Start => {
                            warn!("couldn't find a place to dig")
                        }
                        AutonomyState::Dig => {
                            warn!("couldn't find a place to dump")
                        }
                        _ => {}
                    }
                    blackboard.enqueue_action(Action::SetStage(LunabotStage::SoftStop));
                    Status::Failure
                }
            }
        }),
        
        AlwaysFail // fail so that outer `Invert()` returns success
    )))
}
