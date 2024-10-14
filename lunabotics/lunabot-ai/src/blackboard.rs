use std::{collections::VecDeque, sync::Arc, time::Instant};

use common::{FromLunabase, LunabotStage};
use crossbeam::atomic::AtomicCell;

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
    stage: Arc<AtomicCell<LunabotStage>>,
}

impl LunabotBlackboard {
    pub fn new(stage: Arc<AtomicCell<LunabotStage>>) -> Self {
        Self {
            now: Instant::now(),
            from_lunabase: Default::default(),
            autonomy: Autonomy::None,
            stage,
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

    pub fn set_stage(&self, stage: LunabotStage) {
        self.stage.store(stage);
    }

    pub fn get_stage(&self) -> LunabotStage {
        self.stage.load()
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
