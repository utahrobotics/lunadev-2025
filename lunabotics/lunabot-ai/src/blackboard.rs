use std::{collections::VecDeque, sync::Arc, time::Instant};

use common::FromLunabase;
use k::Chain;
use nalgebra::{Isometry3, Point3};

use crate::{autonomy::Autonomy, Action, PollWhen};

pub enum Input {
    FromLunabase(FromLunabase),
    PathCalculated(Vec<Point3<f64>>),
    LunabaseDisconnected,
}

#[derive(Debug)]
pub(crate) struct LunabotBlackboard {
    now: Instant,
    from_lunabase: VecDeque<FromLunabase>,
    autonomy: Autonomy,
    chain: Arc<Chain<f64>>,
    path: Vec<Point3<f64>>,
    lunabase_disconnected: bool,
    actions: Vec<Action>,
    poll_when: PollWhen,
}

impl LunabotBlackboard {
    pub fn new(chain: Arc<Chain<f64>>) -> Self {
        Self {
            now: Instant::now(),
            from_lunabase: Default::default(),
            autonomy: Autonomy::None,
            path: vec![],
            chain,
            lunabase_disconnected: true,
            actions: vec![],
            poll_when: PollWhen::NoDelay,
        }
    }
}

impl LunabotBlackboard {
    pub fn peek_from_lunabase(&self) -> Option<&FromLunabase> {
        self.from_lunabase.front()
    }

    pub fn pop_from_lunabase(&mut self) -> Option<FromLunabase> {
        self.from_lunabase.pop_front()
    }

    pub fn get_autonomy(&mut self) -> &mut Autonomy {
        &mut self.autonomy
    }

    pub fn get_poll_when(&mut self) -> &mut PollWhen {
        &mut self.poll_when
    }

    pub fn get_robot_isometry(&self) -> Isometry3<f64> {
        self.chain.origin()
    }

    pub fn get_path(&self) -> Option<&[Point3<f64>]> {
        if self.path.is_empty() {
            None
        } else {
            Some(&self.path)
        }
    }

    pub fn invalidate_path(&mut self) {
        self.path.clear();
    }

    pub fn lunabase_disconnected(&mut self) -> &mut bool {
        &mut self.lunabase_disconnected
    }

    // pub fn get_now(&self) -> Instant {
    //     self.now
    // }

    pub fn digest_input(&mut self, input: Input) {
        match input {
            Input::FromLunabase(msg) => self.from_lunabase.push_back(msg),
            Input::PathCalculated(path) => self.path = path,
            Input::LunabaseDisconnected => self.lunabase_disconnected = true,
        }
        self.now = Instant::now();
    }

    pub fn enqueue_action(&mut self, action: Action) {
        self.actions.push(action);
    }

    pub fn drain_actions(&mut self) -> std::vec::Drain<Action> {
        self.actions.drain(..)
    }
}
