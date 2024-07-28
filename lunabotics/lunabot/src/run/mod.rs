use bonsai_bt::{Behavior::*, Event, Status, UpdateArgs, BT};
use urobotics::log::{error, info};

use crate::{setup::Blackboard, LunabotApp};

pub(super) fn run(
    bb: &mut Option<Blackboard>,
    dt: f64,
    first_time: bool,
    _lunabot_app: &LunabotApp,
) -> (Status, f64) {
    if first_time {
        info!("Entered Run");
    }
    let Some(bb) = bb else {
        error!("Blackboard is null");
        return (Status::Failure, dt);
    };
    if first_time {
        bb.run_state.behavior_tree.reset_bt();
    }
    bb.run_state
        .behavior_tree
        .tick(&Event::from(UpdateArgs { dt }), &mut |args, bb| {
            let result = match args.action {
                RunActions::TraverseObstacles => todo!(),
                RunActions::Dig => todo!(),
                RunActions::Dump => todo!(),
                RunActions::ManualControl => todo!(),
            };
            result
        })
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
