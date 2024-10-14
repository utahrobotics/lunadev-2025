use std::{collections::VecDeque, time::Instant};

use common::FromLunabase;

use crate::autonomy::Autonomy;

pub enum Input {
    FromLunabase(FromLunabase),
    NoInput,
}

#[derive(Debug)]
pub(crate) struct LunabotBlackboard {
    now: Instant,
    from_lunabase: VecDeque<FromLunabase>,
    autonomy: Autonomy,
}

impl Default for LunabotBlackboard {
    fn default() -> Self {
        Self {
            now: Instant::now(),
            from_lunabase: Default::default(),
            autonomy: Autonomy::None,
        }
    }
}

impl LunabotBlackboard {
    pub fn pop_from_lunabase(&mut self) -> Option<FromLunabase> {
        self.from_lunabase.pop_front()
    }

    pub fn get_autonomy(&mut self) -> &mut Autonomy {
        &mut self.autonomy
    }

    // pub fn get_now(&self) -> Instant {
    //     self.now
    // }

    pub fn digest_input(&mut self, input: Input) {
        match input {
            Input::FromLunabase(msg) => self.from_lunabase.push_back(msg),
            Input::NoInput => {}
        }
        self.now = Instant::now();
    }
}
