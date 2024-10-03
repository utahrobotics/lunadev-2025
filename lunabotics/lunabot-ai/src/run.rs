use luna_bt::{Behaviour, Status, OK};
use nalgebra::Isometry3;

use crate::{DriveComponent, LunabotBlackboard, Pathfinder, TeleOp};

pub fn run<D, P, O, T>(bb: &mut LunabotBlackboard<D, P, O, T>) -> Status
where
    D: DriveComponent,
    P: Pathfinder,
    O: Fn() -> Isometry3<f64>,
    T: TeleOp,
{
    Behaviour::while_loop(
        Behaviour::constant(OK),
        [
            
        ]
    ).run(bb)
}