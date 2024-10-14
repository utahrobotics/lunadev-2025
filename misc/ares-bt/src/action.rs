use crate::{
    Behavior, EternalBehavior, FallibleBehavior, FallibleStatus, InfallibleBehavior,
    InfallibleStatus, IntoRon, Status,
};

impl<T, F: FnMut(&mut B) -> Status<T>, B> Behavior<B, T> for F {
    fn run(&mut self, blackboard: &mut B) -> Status<T> {
        self(blackboard)
    }
}

impl<T, F: FnMut(&mut B) -> InfallibleStatus<T>, B> InfallibleBehavior<B, T> for F {
    fn run_infallible(&mut self, blackboard: &mut B) -> InfallibleStatus<T> {
        self(blackboard)
    }
}

impl<T, F: FnMut(&mut B) -> FallibleStatus<T>, B> FallibleBehavior<B, T> for F {
    fn run_fallible(&mut self, blackboard: &mut B) -> FallibleStatus<T> {
        self(blackboard)
    }
}

impl<T, F: FnMut(&mut B) -> T, B> EternalBehavior<B, T> for F {
    fn run_eternal(&mut self, blackboard: &mut B) -> T {
        self(blackboard)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AlwaysSucceed;

impl<T, B> Behavior<B, T> for AlwaysSucceed {
    fn run(&mut self, _blackboard: &mut B) -> Status<T> {
        Status::Success
    }
}

impl<T, B> InfallibleBehavior<B, T> for AlwaysSucceed {
    fn run_infallible(&mut self, _blackboard: &mut B) -> InfallibleStatus<T> {
        InfallibleStatus::Success
    }
}

impl IntoRon for AlwaysSucceed {
    fn into_ron(&self) -> ron::Value {
        ron::Value::String("AlwaysSucceed".to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AlwaysFail;

impl<T, B> Behavior<B, T> for AlwaysFail {
    fn run(&mut self, _blackboard: &mut B) -> Status<T> {
        Status::Failure
    }
}

impl<T, B> FallibleBehavior<B, T> for AlwaysFail {
    fn run_fallible(&mut self, _blackboard: &mut B) -> FallibleStatus<T> {
        FallibleStatus::Failure
    }
}

impl IntoRon for AlwaysFail {
    fn into_ron(&self) -> ron::Value {
        ron::Value::String("AlwaysFail".to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AlwaysRunning;

impl<T: Default, B> Behavior<B, T> for AlwaysRunning {
    fn run(&mut self, _blackboard: &mut B) -> Status<T> {
        Status::Running(Default::default())
    }
}

impl<T: Default, B> InfallibleBehavior<B, T> for AlwaysRunning {
    fn run_infallible(&mut self, _blackboard: &mut B) -> InfallibleStatus<T> {
        InfallibleStatus::Running(Default::default())
    }
}

impl<T: Default, B> FallibleBehavior<B, T> for AlwaysRunning {
    fn run_fallible(&mut self, _blackboard: &mut B) -> FallibleStatus<T> {
        FallibleStatus::Running(Default::default())
    }
}

impl<T: Default, B> EternalBehavior<B, T> for AlwaysRunning {
    fn run_eternal(&mut self, _blackboard: &mut B) -> T {
        Default::default()
    }
}

impl IntoRon for AlwaysRunning {
    fn into_ron(&self) -> ron::Value {
        ron::Value::String("AlwaysRunning".to_string())
    }
}

pub struct RunOnce<T> {
    pub run_value: T,
    ran: bool,
}

impl<T> From<T> for RunOnce<T> {
    fn from(run_value: T) -> Self {
        Self {
            run_value,
            ran: false,
        }
    }
}

impl<B, T: Clone> Behavior<B, T> for RunOnce<T> {
    fn run(&mut self, _blackboard: &mut B) -> Status<T> {
        if self.ran {
            self.ran = false;
            Status::Success
        } else {
            self.ran = true;
            Status::Running(self.run_value.clone())
        }
    }
}

impl<B, T: Clone> InfallibleBehavior<B, T> for RunOnce<T> {
    fn run_infallible(&mut self, _blackboard: &mut B) -> InfallibleStatus<T> {
        if self.ran {
            self.ran = false;
            InfallibleStatus::Success
        } else {
            self.ran = true;
            InfallibleStatus::Running(self.run_value.clone())
        }
    }
}
