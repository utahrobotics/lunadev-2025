use ares_bt::{converters::AssertCancelSafe, looping::WhileLoop, sequence::{ParallelAny, Sequence}, Behavior, Status};
use common::FromLunabase;
use dig::dig;
use dump::dump;
use traverse::traverse;

use crate::{blackboard::LunabotBlackboard, Action};

mod dig;
mod dump;
mod traverse;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutonomyStage {
    TraverseObstacles,
    Dig,
    Dump,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Autonomy {
    FullAutonomy(AutonomyStage),
    PartialAutonomy(AutonomyStage),
    None,
}

impl Autonomy {
    fn advance(&mut self) {
        match *self {
            Autonomy::FullAutonomy(autonomy_stage) => match autonomy_stage {
                AutonomyStage::TraverseObstacles => {
                    *self = Autonomy::FullAutonomy(AutonomyStage::Dig)
                }
                AutonomyStage::Dig => *self = Autonomy::FullAutonomy(AutonomyStage::Dump),
                AutonomyStage::Dump => *self = Autonomy::FullAutonomy(AutonomyStage::Dig),
            },
            Autonomy::PartialAutonomy(_) => *self = Self::None,
            Autonomy::None => {}
        }
    }
}

pub fn autonomy() -> impl Behavior<LunabotBlackboard, Action> {
    WhileLoop::new(
        |blackboard: &mut LunabotBlackboard| (*blackboard.get_autonomy() != Autonomy::None).into(),
        ParallelAny::new((
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                while let Some(msg) = blackboard.peek_from_lunabase() {
                    match msg {
                        FromLunabase::Steering(_) => return Status::Success,
                        FromLunabase::SoftStop => {
                            blackboard.pop_from_lunabase();
                            return Status::Failure;
                        }
                        _ => blackboard.pop_from_lunabase(),
                    };
                }
                Status::Running(Action::PollAgain)
            }),
            Sequence::new((dig(), dump(), traverse())),
        ))
    )
}
