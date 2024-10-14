use std::sync::Arc;

use ares_bt::{
    action::AlwaysSucceed,
    branching::TryCatch,
    converters::{CatchPanic, Invert},
    looping::WhileLoop,
    sequence::Sequence,
    EternalBehavior, FallibleStatus,
};
use autonomy::autonomy;
use blackboard::LunabotBlackboard;
use common::{FromLunabase, LunabotStage, Steering};
use crossbeam::atomic::AtomicCell;
use log::warn;
use teleop::teleop;

mod autonomy;
mod blackboard;
mod teleop;

pub use blackboard::Input;

pub enum Action {
    WaitForLunabase,
    SetSteering(Steering),
}

pub fn run_ai(stage: Arc<AtomicCell<LunabotStage>>, mut on_action: impl FnMut(Action) -> Input) {
    let mut blackboard = LunabotBlackboard::new(stage);
    let mut b = WhileLoop::new(
        AlwaysSucceed,
        Sequence::new((
            Invert(WhileLoop::new(
                AlwaysSucceed,
                |blackboard: &mut LunabotBlackboard| {
                    blackboard.set_stage(LunabotStage::SoftStop);
                    while let Some(msg) = blackboard.pop_from_lunabase() {
                        match msg {
                            FromLunabase::ContinueMission => {
                                warn!("Continuing mission");
                                return FallibleStatus::Failure;
                            }
                            _ => {}
                        }
                    }
                    FallibleStatus::Running(Action::WaitForLunabase)
                },
            )),
            TryCatch::new(
                WhileLoop::new(
                    AlwaysSucceed,
                    Sequence::new((CatchPanic(teleop()), CatchPanic(autonomy()))),
                ),
                AlwaysSucceed,
            ),
        )),
    );

    loop {
        let input = on_action(b.run_eternal(&mut blackboard));
        blackboard.digest_input(input);
    }
}
