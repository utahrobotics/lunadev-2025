use std::ops::ControlFlow;

use bonsai_bt::{Behavior::*, Event, Status, UpdateArgs, BT, RUNNING};
use common::FromLunabase;
use urobotics::log::{error, info};

use crate::{setup::Blackboard, LunabotApp};

pub(super) fn run(
    bb: &mut Option<Blackboard>,
    dt: f64,
    first_time: bool,
    lunabot_app: &LunabotApp,
) -> (Status, f64) {
    if first_time {
        info!("Entered Run");
    }
    let Some(bb) = bb else {
        error!("Blackboard is null");
        return (Status::Failure, dt);
    };
    bb.poll_ping(dt);
    let Some(mut run_state) = bb.run_state.take() else {
        error!("RunState is null");
        return (Status::Failure, dt);
    };
    if first_time {
        run_state.behavior_tree.reset_bt();
    }
    let result = run_state
        .behavior_tree
        .tick(&Event::from(UpdateArgs { dt }), &mut |args, _run_bb| {
            let result = match args.action {
                RunActions::TraverseObstacles => todo!(),
                RunActions::Dig => todo!(),
                RunActions::Dump => todo!(),
                RunActions::ManualControl => {
                    bb.on_get_msg_from_lunabase(lunabot_app.get_target_delta(), |msg| {
                        match msg {
                            // FromLunabase::Pong => {}
                            FromLunabase::Steering(steering) => {
                                let (drive, steering) = steering.get_drive_and_steering();
                                info!("Received steering command: drive: {drive}, steering: {steering}");
                            }
                            FromLunabase::TraverseObstacles => {
                                info!("Commencing obstacle zone traversal");
                                return ControlFlow::Break((Status::Success, 0.0));
                            }
                            FromLunabase::SoftStop => {
                                return ControlFlow::Break((Status::Failure, 0.0));
                            }
                            FromLunabase::ContinueMission => {}
                            _ => {
                                error!("Unexpected msg: {msg:?}");
                            }
                        }
                        ControlFlow::Continue(())
                    }).unwrap_or(RUNNING)
                }
            };
            #[allow(unreachable_code)]
            result
        });

    bb.run_state = Some(run_state);
    result
}

#[derive(Debug, Clone, Copy)]
enum RunActions {
    TraverseObstacles,
    Dig,
    Dump,
    ManualControl,
}

#[derive(Debug)]
struct RunBlackboard {}

#[derive(Debug)]
pub(crate) struct RunState {
    behavior_tree: BT<RunActions, RunBlackboard>,
}

impl RunState {
    pub fn new(_lunabot_app: &LunabotApp) -> anyhow::Result<Self> {
        let blackboard = RunBlackboard {};
        let behavior = While(
            Box::new(WaitForever),
            vec![
                Action(RunActions::ManualControl),
                If(
                    Box::new(Action(RunActions::TraverseObstacles)),
                    Box::new(While(
                        Box::new(WaitForever),
                        vec![
                            While(
                                Box::new(WaitForever),
                                vec![Action(RunActions::Dig), Action(RunActions::Dump)],
                            ),
                            Action(RunActions::ManualControl),
                        ],
                    )),
                    Box::new(Wait(0.0)),
                ),
            ],
        );
        Ok(Self {
            behavior_tree: BT::new(behavior, blackboard),
        })
    }
}
