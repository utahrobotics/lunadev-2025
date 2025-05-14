use std::borrow::Cow;

use crate::{
    Behavior, CancelSafe, EternalBehavior, EternalStatus, FallibleBehavior, FallibleStatus,
    InfallibleBehavior, InfallibleStatus, Status,
};

pub struct InfallibleShim<A>(pub A);

impl<A, B> Behavior<B> for InfallibleShim<A>
where
    A: InfallibleBehavior<B>,
{
    fn run(&mut self, blackboard: &mut B) -> Status {
        match self.0.run_infallible(blackboard) {
            InfallibleStatus::Running => Status::Running,
            InfallibleStatus::Success => Status::Success,
        }
    }
}

impl<A> CancelSafe for InfallibleShim<A>
where
    A: CancelSafe,
{
    fn reset(&mut self) {
        self.0.reset();
    }
}

pub struct FallibleShim<A>(pub A);

impl<A, B> Behavior<B> for FallibleShim<A>
where
    A: FallibleBehavior<B>,
{
    fn run(&mut self, blackboard: &mut B) -> Status {
        match self.0.run_fallible(blackboard) {
            FallibleStatus::Running => Status::Running,
            FallibleStatus::Failure => Status::Failure,
        }
    }
}

impl<A> CancelSafe for FallibleShim<A>
where
    A: CancelSafe,
{
    fn reset(&mut self) {
        self.0.reset();
    }
}
pub struct EternalShim<A>(pub A);

impl<A, B> Behavior<B> for EternalShim<A>
where
    A: EternalBehavior<B>,
{
    fn run(&mut self, blackboard: &mut B) -> Status {
        self.0.run_eternal(blackboard).into()
    }
}

impl<A> CancelSafe for EternalShim<A>
where
    A: CancelSafe,
{
    fn reset(&mut self) {
        self.0.reset();
    }
}

pub struct Invert<A>(pub A);

impl<A, B> Behavior<B> for Invert<A>
where
    A: Behavior<B>,
{
    fn run(&mut self, blackboard: &mut B) -> Status {
        match self.0.run(blackboard) {
            Status::Failure => Status::Success,
            Status::Success => Status::Failure,
            Status::Running => Status::Running,
        }
    }
}

impl<A> CancelSafe for Invert<A>
where
    A: CancelSafe,
{
    fn reset(&mut self) {
        self.0.reset();
    }
}

impl<A, B> InfallibleBehavior<B> for Invert<A>
where
    A: FallibleBehavior<B>,
{
    fn run_infallible(&mut self, blackboard: &mut B) -> InfallibleStatus {
        match self.0.run_fallible(blackboard) {
            FallibleStatus::Running => InfallibleStatus::Running,
            FallibleStatus::Failure => InfallibleStatus::Success,
        }
    }
}

impl<A, B> FallibleBehavior<B> for Invert<A>
where
    A: InfallibleBehavior<B>,
{
    fn run_fallible(&mut self, blackboard: &mut B) -> FallibleStatus {
        match self.0.run_infallible(blackboard) {
            InfallibleStatus::Running => FallibleStatus::Running,
            InfallibleStatus::Success => FallibleStatus::Failure,
        }
    }
}

impl<A, B> EternalBehavior<B> for Invert<A>
where
    A: EternalBehavior<B>,
{
    fn run_eternal(&mut self, blackboard: &mut B) -> EternalStatus {
        self.0.run_eternal(blackboard)
    }
}

pub struct CatchPanic<A>(pub A);

impl<A, B> Behavior<B> for CatchPanic<A>
where
    A: Behavior<B>,
{
    fn run(&mut self, blackboard: &mut B) -> Status {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.0.run(blackboard))) {
            Ok(status) => status,
            Err(_) => Status::Failure,
        }
    }
}

impl<A> CancelSafe for CatchPanic<A>
where
    A: CancelSafe,
{
    fn reset(&mut self) {
        self.0.reset();
    }
}

impl<A, B> FallibleBehavior<B> for CatchPanic<A>
where
    A: FallibleBehavior<B>,
{
    fn run_fallible(&mut self, blackboard: &mut B) -> FallibleStatus {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.0.run_fallible(blackboard)
        })) {
            Ok(status) => status,
            Err(_) => FallibleStatus::Failure,
        }
    }
}
pub struct Rename<A> {
    pub name: Cow<'static, str>,
    pub behavior: A,
}

impl<A> Rename<A> {
    pub fn new(name: impl Into<Cow<'static, str>>, behavior: A) -> Self {
        Self {
            name: name.into(),
            behavior,
        }
    }
}

pub struct AssertCancelSafe<A>(pub A);

impl<A> CancelSafe for AssertCancelSafe<A> {
    fn reset(&mut self) {}
}

impl<A, B, T> FnMut<(&mut B,)> for AssertCancelSafe<A>
where
    A: FnMut(&mut B) -> T,
{
    extern "rust-call" fn call_mut(&mut self, args: (&mut B,)) -> Self::Output {
        self.0.call_mut(args)
    }
}

impl<A, B, T> FnOnce<(&mut B,)> for AssertCancelSafe<A>
where
    A: FnMut(&mut B) -> T,
{
    type Output = T;

    extern "rust-call" fn call_once(mut self, args: (&mut B,)) -> Self::Output {
        self.call_mut(args)
    }
}
