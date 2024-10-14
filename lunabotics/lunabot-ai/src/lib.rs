use ares_bt::{
    action::{AlwaysSucceed, RunOnce},
    branching::TryCatch,
    converters::{CatchPanic, Invert},
    looping::WhileLoop,
    sequence::Sequence,
    EternalBehavior, FallibleStatus,
};
use autonomy::autonomy;
use blackboard::LunabotBlackboard;
use common::{FromLunabase, LunabotStage, Steering};
use log::warn;
use teleop::teleop;

mod autonomy;
mod blackboard;
mod teleop;

pub use blackboard::Input;

#[derive(Debug, Clone, Copy)]
pub enum Action {
    WaitForLunabase,
    SetSteering(Steering),
    SetStage(LunabotStage)
}

pub fn run_ai(mut on_action: impl FnMut(Action) -> Input) {
    let mut blackboard = LunabotBlackboard::default();
    let mut b = WhileLoop::new(
        AlwaysSucceed,
        Sequence::new((
            RunOnce::from(Action::SetStage(LunabotStage::SoftStop)),
            Invert(WhileLoop::new(
                AlwaysSucceed,
                |blackboard: &mut LunabotBlackboard| {
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
