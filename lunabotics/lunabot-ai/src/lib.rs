use std::marker::PhantomData;

use common::FromLunabase;
pub use drive::DriveComponent;
use log::{error, warn};
use luna_bt::{Behaviour, ERR, OK};
use nalgebra::Isometry3;
pub use pathfinding::Pathfinder;
use tasker::{task::SyncTask, BlockOn};
pub use teleop::TeleOp;

mod drive;
mod pathfinding;
mod teleop;
mod run;

pub struct LunabotAI<F, D=(), P=(), O=(), T=()> {
    pub make_blackboard: F,
    _phantom: PhantomData<fn() -> (D, P, O, T)>
}

impl<D, P, O, T, F> From<F> for LunabotAI<F, D, P, O, T> {
    fn from(value: F) -> Self {
        Self {
            make_blackboard: value,
            _phantom: PhantomData
        }
    }
}

pub struct LunabotBlackboard<D, P, O, T> {
    pub drive: D,
    pub pathfinder: P,
    pub get_isometry: O,
    pub teleop: T
}

impl<D, P, O: Fn() -> Isometry3<f64>, T> LunabotBlackboard<D, P, O, T> {
    #[allow(dead_code)]
    fn get_isometry(&self) -> Isometry3<f64> {
        (self.get_isometry)()
    }
}

impl<D, P, O, T, F> SyncTask for LunabotAI<F, D, P, O, T>
where
    D: DriveComponent,
    P: Pathfinder,
    O: Fn() -> Isometry3<f64>,
    T: TeleOp,
    F: FnMut(Option<LunabotBlackboard<D, P, O, T>>) -> LunabotBlackboard<D, P, O, T>,
    Self: Send + 'static
{
    type Output = ();

    fn run(mut self) -> Self::Output {
        let mut bb: Option<LunabotBlackboard<D, P, O, T>> = None;
        let _ = Behaviour::while_loop(
                Behaviour::constant(OK),
                [
                    // Setup, Software Stop, loop
                    Behaviour::ignore(
                        Behaviour::while_loop(
                            Behaviour::constant(OK),
                            [
                                // Setup
                                // 
                                // Initialize the blackboard. The blackboard may already exist, so the
                                // make_blackboard function can modify it if needed.
                                Behaviour::action(|bb: &mut Option<LunabotBlackboard<D, P, O, T>>| {
                                    *bb = Some((self.make_blackboard)(bb.take()));
                                    OK
                                }),
                                // Software Stop
                                // 
                                // Wait for the operator to send a message indicating if it is safe
                                // to continue the mission or if the blackboard needs to be re-initialized.
                                // For now, the blackboard is dropped when re-initialization is requested,
                                // but we could allow for partial re-initialization if needed in the future.
                                Behaviour::action(|bb: &mut Option<LunabotBlackboard<D, P, O, T>>| {
                                    let bb_mut = bb.as_mut().expect("Blackboard should be initialized");
                                    loop {
                                        match bb_mut.teleop.from_lunabase().block_on() {
                                            FromLunabase::ContinueMission => break ERR,
                                            FromLunabase::TriggerSetup => {
                                                *bb = None;
                                                break OK;
                                            },
                                            m => warn!("Unexpected message: {m:?}")
                                        }
                                    }
                                }),
                            ]
                        ),
                        OK
                    ),
                    // Run
                    // 
                    // If run fails, log it, but replace the fail with OK so the loop continues.
                    Behaviour::if_else(
                        Behaviour::action(|bb: &mut Option<LunabotBlackboard<D, P, O, T>>| {
                            let bb_mut = bb.as_mut().expect("Blackboard should be initialized");
                            run::run(bb_mut)
                        }), 
                        Behaviour::Constant(OK),
                        Behaviour::action(|_| {
                            error!("Run behaviour tree failed");
                            OK
                        })
                    )
                ]
            )
            .run(&mut bb);
        unreachable!("BT while_loop should never return");
    }
}
