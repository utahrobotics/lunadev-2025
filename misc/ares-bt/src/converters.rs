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
