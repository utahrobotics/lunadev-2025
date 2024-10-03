use log::{error, info};
use luna_bt::{Behaviour, ERR, OK};

use crate::{Autonomy, AutonomyStage, LunabotBlackboard};


pub(super) fn dump<D, P, O, T>() -> Behaviour<'static, Option<LunabotBlackboard<D, P, O, T>>> {
    Behaviour::if_else(
            Behaviour::action(|bb: &mut Option<LunabotBlackboard<D, P, O, T>>| {
                let Some(bb) = bb else {
                    error!("Blackboard is missing in dump");
                    return ERR;
                };
                match bb.autonomy {
                    Autonomy::PartialAutonomy(AutonomyStage::Dump) => OK,
                    Autonomy::FullAutonomy(AutonomyStage::Dump) => OK,
                    _ => ERR,
                }
            }),
            Behaviour::sequence(
                [
                    Behaviour::action(|bb: &mut Option<LunabotBlackboard<D, P, O, T>>| {
                        let bb = bb.as_mut().unwrap();
                        info!("Dumping");
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        if let Autonomy::PartialAutonomy(_) = bb.autonomy {
                            bb.autonomy = Autonomy::None;
                        }
                        OK
                    }),
                ]
            ),
            Behaviour::constant(ERR)
        )
}