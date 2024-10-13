use std::marker::PhantomData;

use crate::{
    Behavior, EternalBehavior, FallibleBehavior, FallibleStatus, InfallibleBehavior,
    InfallibleStatus, Status,
};

pub struct InfallibleShim<A>(pub A);

impl<A, B, T> Behavior<B, T> for InfallibleShim<A>
where
    A: InfallibleBehavior<B, T>,
{
    fn run(&mut self, blackboard: &mut B) -> Status<T> {
        match self.0.run_infallible(blackboard) {
            InfallibleStatus::Running(t) => Status::Running(t),
            InfallibleStatus::Success => Status::Success,
        }
    }
}

pub struct FallibleShim<A>(pub A);

impl<A, B, T> Behavior<B, T> for FallibleShim<A>
where
    A: FallibleBehavior<B, T>,
{
    fn run(&mut self, blackboard: &mut B) -> Status<T> {
        match self.0.run_fallible(blackboard) {
            FallibleStatus::Running(t) => Status::Running(t),
            FallibleStatus::Failure => Status::Failure,
        }
    }
}

pub struct EternalShim<A>(pub A);

impl<A, B, T> Behavior<B, T> for EternalShim<A>
where
    A: FnMut(&mut B) -> T,
{
    fn run(&mut self, blackboard: &mut B) -> Status<T> {
        Status::Running((self.0)(blackboard))
    }
}

pub struct Invert<A>(pub A);

impl<A, B, T> Behavior<B, T> for Invert<A>
where
    A: Behavior<B, T>,
{
    fn run(&mut self, blackboard: &mut B) -> Status<T> {
        match self.0.run(blackboard) {
            Status::Failure => Status::Success,
            Status::Success => Status::Failure,
            Status::Running(t) => Status::Running(t),
        }
    }
}

impl<A, B, T> InfallibleBehavior<B, T> for Invert<A>
where
    A: FallibleBehavior<B, T>,
{
    fn run_infallible(&mut self, blackboard: &mut B) -> InfallibleStatus<T> {
        match self.0.run_fallible(blackboard) {
            FallibleStatus::Running(t) => InfallibleStatus::Running(t),
            FallibleStatus::Failure => InfallibleStatus::Success,
        }
    }
}

impl<A, B, T> FallibleBehavior<B, T> for Invert<A>
where
    A: InfallibleBehavior<B, T>,
{
    fn run_fallible(&mut self, blackboard: &mut B) -> FallibleStatus<T> {
        match self.0.run_infallible(blackboard) {
            InfallibleStatus::Running(t) => FallibleStatus::Running(t),
            InfallibleStatus::Success => FallibleStatus::Failure,
        }
    }
}

impl<A, B, T> EternalBehavior<B, T> for Invert<A>
where
    A: EternalBehavior<B, T>,
{
    fn run_eternal(&mut self, blackboard: &mut B) -> T {
        self.0.run_eternal(blackboard)
    }
}

pub struct WithSubBlackboard<A, B> {
    pub behavior: A,
    _phantom: PhantomData<fn() -> B>
}

impl<A, B> From<A> for WithSubBlackboard<A, B> {
    fn from(behavior: A) -> Self {
        WithSubBlackboard {
            behavior,
            _phantom: PhantomData,
        }
    }
}

pub trait AsSubBlackboard<B> {
    fn on_sub_blackboard<T>(&mut self, f: impl FnOnce(&mut B) -> T) -> T;
}

impl<A, B1, B2, T> Behavior<B1, T> for WithSubBlackboard<A, B2>
where
    A: Behavior<B2, T>,
    B1: AsSubBlackboard<B2>,
{
    fn run(&mut self, blackboard: &mut B1) -> Status<T> {
        blackboard.on_sub_blackboard(|sub_blackboard| self.behavior.run(sub_blackboard))
    }
}

impl<A, B1, B2, T> InfallibleBehavior<B1, T> for WithSubBlackboard<A, B2>
where
    A: InfallibleBehavior<B2, T>,
    B1: AsSubBlackboard<B2>,
{
    fn run_infallible(&mut self, blackboard: &mut B1) -> InfallibleStatus<T> {
        blackboard.on_sub_blackboard(|sub_blackboard| self.behavior.run_infallible(sub_blackboard))
    }
}

impl<A, B1, B2, T> FallibleBehavior<B1, T> for WithSubBlackboard<A, B2>
where
    A: FallibleBehavior<B2, T>,
    B1: AsSubBlackboard<B2>,
{
    fn run_fallible(&mut self, blackboard: &mut B1) -> FallibleStatus<T> {
        blackboard.on_sub_blackboard(|sub_blackboard| self.behavior.run_fallible(sub_blackboard))
    }
}

impl<A, B1, B2, T> EternalBehavior<B1, T> for WithSubBlackboard<A, B2>
where
    A: EternalBehavior<B2, T>,
    B1: AsSubBlackboard<B2>,
{
    fn run_eternal(&mut self, blackboard: &mut B1) -> T {
        blackboard.on_sub_blackboard(|sub_blackboard| self.behavior.run_eternal(sub_blackboard))
    }
}

pub struct CatchPanic<A>(pub A);

impl<A, B, T> Behavior<B, T> for CatchPanic<A>
where
    A: Behavior<B, T>,
{
    fn run(&mut self, blackboard: &mut B) -> Status<T> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.0.run(blackboard))) {
            Ok(status) => status,
            Err(_) => Status::Failure,
        }
    }
}

impl<A, B, T> FallibleBehavior<B, T> for CatchPanic<A>
where
    A: FallibleBehavior<B, T>,
{
    fn run_fallible(&mut self, blackboard: &mut B) -> FallibleStatus<T> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.0.run_fallible(blackboard))) {
            Ok(status) => status,
            Err(_) => FallibleStatus::Failure,
        }
    }
}