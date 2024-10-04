use std::{marker::PhantomData, panic::UnwindSafe};

use common::FromLunabase;
pub use drive::{DriveComponent, FailedToDrive};
use log::{error, warn};
use luna_bt::{Behaviour, ERR, OK};
use nalgebra::Isometry3;
pub use pathfinding::Pathfinder;
use tasker::{task::SyncTask, BlockOn};
pub use teleop::TeleOp;

mod drive;
mod pathfinding;
mod run;
mod teleop;

pub struct LunabotAI<F, D = (), P = (), O = (), T = ()> {
    pub make_blackboard: F,
    _phantom: PhantomData<fn() -> (D, P, O, T)>,
}

impl<D, P, O, T, F> From<F> for LunabotAI<F, D, P, O, T> {
    fn from(value: F) -> Self {
        Self {
            make_blackboard: value,
            _phantom: PhantomData,
        }
    }
}

pub struct LunabotInterfaces<D, P, O, T> {
    pub drive: D,
    pub pathfinder: P,
    pub get_isometry: O,
    pub teleop: T,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AutonomyStage {
    TraverseObstacles,
    Dig,
    Dump
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Autonomy {
    FullAutonomy(AutonomyStage),
    PartialAutonomy(AutonomyStage),
    None
}

struct LunabotBlackboard<D, P, O, T> {
    drive: D,
    pathfinder: P,
    get_isometry: O,
    teleop: T,
    autonomy: Autonomy
}

impl<D, P, O, T> From<LunabotInterfaces<D, P, O, T>> for LunabotBlackboard<D, P, O, T> {
    fn from(value: LunabotInterfaces<D, P, O, T>) -> Self {
        Self {
            drive: value.drive,
            pathfinder: value.pathfinder,
            get_isometry: value.get_isometry,
            teleop: value.teleop,
            autonomy: Autonomy::PartialAutonomy(AutonomyStage::TraverseObstacles)
        }
    }
}

impl<D, P, O, T> From<LunabotBlackboard<D, P, O, T>> for LunabotInterfaces<D, P, O, T> {
    fn from(value: LunabotBlackboard<D, P, O, T>) -> Self {
        Self {
            drive: value.drive,
            pathfinder: value.pathfinder,
            get_isometry: value.get_isometry,
            teleop: value.teleop
        }
    }
}

impl<D, P, O, T, F> SyncTask for LunabotAI<F, D, P, O, T>
where
    D: DriveComponent,
    P: Pathfinder,
    O: Fn() -> Isometry3<f64>,
    T: TeleOp,
    F: FnMut(Option<LunabotInterfaces<D, P, O, T>>) -> Result<LunabotInterfaces<D, P, O, T>, ()> + UnwindSafe,
    Self: Send + 'static,
    for<'a> &'a mut Option<LunabotBlackboard<D, P, O, T>>: UnwindSafe,
{
    type Output = ();

    fn run(mut self) -> Self::Output {
        let mut bb: Option<LunabotBlackboard<D, P, O, T>> = None;
        let _ = Behaviour::while_loop(
            Behaviour::constant(OK),
            [
                // Setup, Software Stop, loop
                Behaviour::invert(
                    Behaviour::while_loop(
                        Behaviour::constant(OK),
                        [
                            // Setup
                            //
                            // Initialize the blackboard. The blackboard may already exist, so the
                            // make_blackboard function can modify it if needed. If setup fails due
                            // to a panic (or returns an ERR), two things can happen. If the blackboard
                            // is not initialized, setup will be called again after a delay. Otherwise,
                            // Software Stop will be triggered.
                            Behaviour::invert(Behaviour::while_loop(
                                Behaviour::constant(OK),
                                [
                                    Behaviour::invert(Behaviour::action_catch_panic(
                                        move |bb: &mut Option<LunabotBlackboard<D, P, O, T>>| {
                                            *bb = Some((self.make_blackboard)(bb.take().map(Into::into))?.into());
                                            OK
                                        },
                                        |info| {
                                            error!("Setup panicked");
                                            Some(info)
                                        },
                                    )),
                                    Behaviour::action(
                                        |bb: &mut Option<LunabotBlackboard<D, P, O, T>>| {
                                            if bb.is_none() {
                                                std::thread::sleep(std::time::Duration::from_secs(
                                                    2,
                                                ));
                                                OK
                                            } else {
                                                ERR
                                            }
                                        },
                                    ),
                                ],
                            )),
                            // Software Stop
                            //
                            // Wait for the operator to send a message indicating if it is safe
                            // to continue the mission or if the blackboard needs to be re-initialized.
                            // For now, the blackboard is dropped when re-initialization is requested,
                            // but we could allow for partial re-initialization if needed in the future.
                            Behaviour::invert(Behaviour::action_catch_panic(
                                |bb: &mut Option<LunabotBlackboard<D, P, O, T>>| {
                                    let bb_mut =
                                        bb.as_mut().expect("Blackboard should be initialized");
                                    loop {
                                        match bb_mut.teleop.from_lunabase().block_on() {
                                            FromLunabase::ContinueMission => break OK,
                                            FromLunabase::TriggerSetup => {
                                                *bb = None;
                                                break ERR;
                                            }
                                            m => warn!("Unexpected message: {m:?}"),
                                        }
                                    }
                                },
                                |info| {
                                    error!("Software stop panicked");
                                    Some(info)
                                },
                            )),
                        ],
                    ),
                ),
                // Run
                //
                // If run fails, log it but replace the fail with OK so the loop continues.
                Behaviour::if_else(
                    run::run(),
                    Behaviour::Constant(OK),
                    Behaviour::action(|_| {
                        error!("Run behaviour tree failed");
                        OK
                    }),
                ),
            ],
        )
        .run(&mut bb);
        unreachable!("BT while_loop should never return");
    }
}
