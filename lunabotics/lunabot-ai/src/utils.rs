use std::time::{Duration, Instant};

use ares_bt::{Behavior, CancelSafe, InfallibleBehavior, InfallibleStatus, Status};

use crate::{blackboard::LunabotBlackboard, Action};

pub struct WaitBehavior {
    pub duration: Duration,
    start_time: Option<Instant>
}

impl From<Duration> for WaitBehavior {
    fn from(duration: Duration) -> Self {
        Self {
            duration,
            start_time: None
        }
    }
}

impl InfallibleBehavior<LunabotBlackboard, Action> for WaitBehavior {
    fn run_infallible(&mut self, blackboard: &mut LunabotBlackboard) -> InfallibleStatus<Action> {
        if let Some(start) = self.start_time {
            if start.elapsed() >= self.duration {
                self.start_time = None;
                return InfallibleStatus::Success;
            }
        } else {
            self.start_time = Some(blackboard.get_now());
        }
        InfallibleStatus::Running(Action::WaitUntil(self.start_time.unwrap() + self.duration))
    }
}

impl CancelSafe for WaitBehavior {
    fn reset(&mut self) {
        self.start_time = None;
    }
}

impl Behavior<LunabotBlackboard, Action> for WaitBehavior {
    fn run(&mut self, blackboard: &mut LunabotBlackboard) -> Status<Action> {
        self.run_infallible(blackboard).into()
    }
}