use std::time::{Duration, Instant};

use ares_bt::{Behavior, CancelSafe, InfallibleBehavior, InfallibleStatus, Status};

use crate::blackboard::LunabotBlackboard;

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

impl InfallibleBehavior<LunabotBlackboard> for WaitBehavior {
    fn run_infallible(&mut self, blackboard: &mut LunabotBlackboard) -> InfallibleStatus {
        if let Some(start) = self.start_time {
            if start.elapsed() >= self.duration {
                self.start_time = None;
                return InfallibleStatus::Success;
            }
        } else {
            self.start_time = Some(blackboard.get_now());
        }
        InfallibleStatus::Running
    }
}

impl CancelSafe for WaitBehavior {
    fn reset(&mut self) {
        self.start_time = None;
    }
}

impl Behavior<LunabotBlackboard> for WaitBehavior {
    fn run(&mut self, blackboard: &mut LunabotBlackboard) -> Status {
        self.run_infallible(blackboard).into()
    }
}