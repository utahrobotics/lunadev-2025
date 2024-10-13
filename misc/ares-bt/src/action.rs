use crate::{Behavior, EternalBehavior, FallibleBehavior, FallibleStatus, InfallibleBehavior, InfallibleStatus, Status};


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
