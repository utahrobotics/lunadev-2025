use std::{collections::VecDeque, time::Instant};

use common::FromLunabase;
use nalgebra::{Isometry3, Point3};
use simple_motion::StaticImmutableNode;

use crate::{autonomy::Autonomy, Action, PollWhen};

pub enum Input {
    FromLunabase(FromLunabase),
    PathCalculated(Vec<Point3<f64>>),
    LunabaseDisconnected,
}

pub(crate) struct LunabotBlackboard {
    now: Instant,
    from_lunabase: VecDeque<FromLunabase>,
    autonomy: Autonomy,
    chain: StaticImmutableNode,
    path: Vec<Point3<f64>>,
    lunabase_disconnected: bool,
    actions: Vec<Action>,
    poll_when: PollWhen,
}

impl LunabotBlackboard {
    pub fn new(chain: StaticImmutableNode) -> Self {
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
        self.chain.get_global_isometry()
    }

    pub fn get_path(&self) -> Option<&[Point3<f64>]> {
        if self.path.is_empty() {
            None
        } else {
            Some(&self.path)
        }
    }

    pub fn lunabase_disconnected(&mut self) -> &mut bool {
        &mut self.lunabase_disconnected
    }

    pub fn get_now(&self) -> Instant {
        self.now
    }

    pub(crate) fn update_now(&mut self) {
        self.now = Instant::now();
    }

    pub fn digest_input(&mut self, input: Input) {
        match input {
            Input::FromLunabase(msg) => self.from_lunabase.push_back(msg),
            Input::PathCalculated(path) => self.path = path,
            Input::LunabaseDisconnected => self.lunabase_disconnected = true,
        }
    }

    pub fn calculate_path(&mut self, from: Point3<f64>, to: Point3<f64>) {
        let into = std::mem::take(&mut self.path);
        self.enqueue_action(Action::CalculatePath { from, to, into });
    }

    pub fn enqueue_action(&mut self, action: Action) {
        self.actions.push(action);
    }

    pub fn drain_actions(&mut self) -> std::vec::Drain<Action> {
        self.actions.drain(..)
    }
}
