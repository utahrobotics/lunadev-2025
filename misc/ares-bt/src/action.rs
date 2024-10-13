use crate::{
    Behavior, EternalBehavior, FallibleBehavior, FallibleStatus, InfallibleBehavior,
    InfallibleStatus, Status,
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
