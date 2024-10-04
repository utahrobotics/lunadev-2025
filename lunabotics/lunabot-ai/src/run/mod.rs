use common::FromLunabase;
use log::{error, warn};
use luna_bt::{status, Behaviour, ERR, OK};
use nalgebra::Isometry3;
use tasker::BlockOn;

use crate::{Autonomy, AutonomyStage, DriveComponent, LunabotBlackboard, PathfinderComponent, TeleOpComponent};

mod traverse_obstacles;
mod dig;
mod dump;

pub fn run<D, P, O, T>() -> Behaviour<'static, Option<LunabotBlackboard<D, P, O, T>>>
where
    D: DriveComponent,
    P: PathfinderComponent,
    O: Fn() -> Isometry3<f64>,
    T: TeleOpComponent,
{
    Behaviour::while_loop(
        Behaviour::constant(OK),
        [
            Behaviour::action_catch_panic(
                |bb: &mut Option<LunabotBlackboard<D, P, O, T>>| {
                    let Some(bb) = bb else {
                        error!("Blackboard is missing in manual control");
                        return ERR;
                    };
                    loop {
                        match bb.teleop.from_lunabase().block_on() {
                            FromLunabase::Steering(steering) => {
                                bb.drive.manual_drive(steering);
                                if bb.drive.had_drive_error() {
                                    break ERR;
                                }
                            }
                            FromLunabase::TraverseObstacles => bb.autonomy = Autonomy::PartialAutonomy(AutonomyStage::TraverseObstacles),
                            FromLunabase::SoftStop => {
                                warn!("Triggering Software Stop. This will show up as run failing.");
                                break ERR;
                            }
                            m => {
                                warn!("Unexpected message from Lunabase: {m:?}");
                            }
                        }
                    }
                },
                |info| {
                    error!("Panic in manual control");
                    Some(info)
                }
            ),
            Behaviour::while_loop(
                Behaviour::action(|bb: &mut Option<LunabotBlackboard<D, P, O, T>>| {
                    let Some(bb) = bb else {
                        error!("Blackboard is missing in autonomy");
                        return ERR;
                    };
                    
                    status(bb.autonomy != Autonomy::None)
                }),
                [
                    Behaviour::select(
                        [
                            traverse_obstacles::traverse_obstacles(),
                            dig::dig(),
                            dump::dump()
                        ]
                    )
                ]
            )
        ]
    )
}
