use common::FromLunabase;
use log::{error, warn};
use luna_bt::{Behaviour, ERR, OK};
use nalgebra::Isometry3;
use tasker::BlockOn;

use crate::{Autonomy, AutonomyStage, DriveComponent, LunabotBlackboard, Pathfinder, TeleOp};

pub fn run<D, P, O, T>() -> Behaviour<'static, Option<LunabotBlackboard<D, P, O, T>>>
where
    D: DriveComponent,
    P: Pathfinder,
    O: Fn() -> Isometry3<f64>,
    T: TeleOp,
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
                            FromLunabase::TraverseObstacles => bb.autonomy_stage = Autonomy::PartialAutonomy(AutonomyStage::TraverseObstacles),
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
            Behaviour::sequence(
                [
                    // Behaviour::action(|bb: &mut LunabotBlackboard<D, P, O, T>| {
                    //     OK
                    // })
                ]
            )
        ]
    )
}
