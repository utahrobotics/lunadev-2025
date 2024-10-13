use ares_bt::{
    action::AlwaysSucceed,
    branching::TryCatch,
    converters::{CatchPanic, Invert, WithSubBlackboard},
    looping::WhileLoop,
    sequence::Sequence,
    EternalBehavior, FallibleStatus, Status,
};
use autonomy::autonomy;
use blackboard::{FromLunabaseQueue, LunabotBlackboard};
use common::{FromLunabase, FromLunabot, Steering};
use tasker::task::SyncTask;

mod autonomy;
mod blackboard;

pub use blackboard::Input;

pub enum Action {
    WaitForLunabase,
    FromLunabot(FromLunabot),
    SetSteering(Steering),
}

pub struct LunabotAI<F>(pub F);

impl<F: FnMut(Action) -> Input + Send + 'static> SyncTask for LunabotAI<F> {
    type Output = ();

    fn run(mut self) -> Self::Output {
        let mut blackboard = LunabotBlackboard::default();
        let mut b = WhileLoop::new(
            AlwaysSucceed,
            Sequence::new((
                Invert(WhileLoop::new(
                    AlwaysSucceed,
                    WithSubBlackboard::from(|blackboard: &mut FromLunabaseQueue| {
                        while let Some(msg) = blackboard.pop() {
                            match msg {
                                FromLunabase::ContinueMission => return FallibleStatus::Failure,
                                _ => {}
                            }
                        }
                        FallibleStatus::Running(Action::WaitForLunabase)
                    }),
                )),
                TryCatch::new(
                    WhileLoop::new(
                        AlwaysSucceed,
                        Sequence::new((
                            WithSubBlackboard::from(|blackboard: &mut FromLunabaseQueue| {
                                while let Some(msg) = blackboard.pop() {
                                    match msg {
                                        FromLunabase::Steering(steering) => {
                                            return Status::Running(Action::SetSteering(steering))
                                        }
                                        FromLunabase::SoftStop => return Status::Failure,
                                        _ => {}
                                    }
                                }
                                Status::Running(Action::WaitForLunabase)
                            }),
                            CatchPanic(autonomy()),
                        )),
                    ),
                    AlwaysSucceed,
                ),
            )),
        );

        loop {
            let input = (self.0)(b.run_eternal(&mut blackboard));
            blackboard.digest_input(input);
        }
    }
}
