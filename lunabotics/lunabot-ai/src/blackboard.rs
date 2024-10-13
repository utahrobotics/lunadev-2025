use std::{collections::VecDeque, time::Instant};

use ares_bt::converters::AsSubBlackboard;
use common::FromLunabase;

use crate::autonomy::{Autonomy, AutonomyBlackboard};

pub enum Input {
    FromLunabase(FromLunabase),
    NoInput,
}

#[derive(Default, Debug)]
pub struct FromLunabaseQueue {
    queue: VecDeque<FromLunabase>,
}

impl FromLunabaseQueue {
    pub fn pop(&mut self) -> Option<FromLunabase> {
        self.queue.pop_front()
    }
}

#[derive(Debug)]
pub struct LunabotBlackboard {
    now: Instant,
    from_lunabase: FromLunabaseQueue,
    autonomy: Autonomy,
}

impl Default for LunabotBlackboard {
    fn default() -> Self {
        Self {
            now: Instant::now(),
            from_lunabase: FromLunabaseQueue::default(),
            autonomy: Autonomy::None,
        }
    }
}

impl LunabotBlackboard {
    pub fn digest_input(&mut self, input: Input) {
        match input {
            Input::FromLunabase(msg) => self.from_lunabase.queue.push_back(msg),
            Input::NoInput => {}
        }
        self.now = Instant::now();
    }
}

impl AsSubBlackboard<FromLunabaseQueue> for LunabotBlackboard {
    fn on_sub_blackboard<T>(&mut self, f: impl FnOnce(&mut FromLunabaseQueue) -> T) -> T {
        f(&mut self.from_lunabase)
    }
}

impl<'a> AsSubBlackboard<AutonomyBlackboard<'a>> for LunabotBlackboard {
    fn on_sub_blackboard<T>(&mut self, f: impl FnOnce(&mut AutonomyBlackboard) -> T) -> T {
        let mut autonomy_bb = AutonomyBlackboard {
            autonomy: self.autonomy,
            from_lunabase: &mut self.from_lunabase,
        };
        let result = f(&mut autonomy_bb);
        self.autonomy = autonomy_bb.autonomy;
        result
    }
}
