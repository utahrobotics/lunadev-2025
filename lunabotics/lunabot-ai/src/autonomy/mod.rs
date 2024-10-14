use ares_bt::{looping::WhileLoop, sequence::Select, Behavior};
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
        Select::new((dig(), dump(), traverse())),
    )
}
