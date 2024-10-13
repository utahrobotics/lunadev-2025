use crate::{Behavior, InfallibleBehavior, InfallibleStatus, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IfElseState {
    Condition,
    IfTrue,
    IfFalse,
}

pub struct IfElse<A, B, C> {
    pub condition: A,
    pub if_true: B,
    pub if_false: C,
    state: IfElseState,
}

impl<A, B, C, D, T> Behavior<D, T> for IfElse<A, B, C>
where
    A: Behavior<D, T>,
    B: Behavior<D, T>,
    C: Behavior<D, T>,
{
    fn run(&mut self, blackboard: &mut D) -> Status<T> {
        let result = match self.state {
            IfElseState::Condition => {
                match self.condition.run(blackboard) {
                    Status::Running(t) => return Status::Running(t),
                    Status::Success => {
                        self.state = IfElseState::IfTrue;
                        self.if_true.run(blackboard)
                    }
                    Status::Failure => {
                        self.state = IfElseState::IfFalse;
                        self.if_false.run(blackboard)
                    }
                }
            }
            IfElseState::IfTrue => self.if_true.run(blackboard),
            IfElseState::IfFalse => self.if_false.run(blackboard),
        };
        
        if !result.is_running() {
            self.state = IfElseState::Condition;
        }
        
        result
    }
}

pub struct TryCatch<A, B> {
    pub try_behavior: A,
    pub catch: B,
    trying: bool
}

impl<A, B, D, T> Behavior<D, T> for TryCatch<A, B>
where
    A: Behavior<D, T>,
    B: Behavior<D, T>,
{
    fn run(&mut self, blackboard: &mut D) -> Status<T> {
        let result = if self.trying {
            match self.try_behavior.run(blackboard) {
                Status::Running(t) => return Status::Running(t),
                Status::Success => return Status::Success,
                Status::Failure => {
                    self.trying = false;
                    self.catch.run(blackboard)
                }
            }
        } else {
            self.catch.run(blackboard)
        };
        
        if !result.is_running() {
            self.trying = true;
        }
        
        result
    }
}

impl<A, B, D, T> InfallibleBehavior<D, T> for TryCatch<A, B>
where
    A: Behavior<D, T>,
    B: InfallibleBehavior<D, T>,
{
    fn run_infallible(&mut self, blackboard: &mut D) -> InfallibleStatus<T> {
        let result = if self.trying {
                match self.try_behavior.run(blackboard) {
                    Status::Running(t) => return InfallibleStatus::Running(t),
                    Status::Success => return InfallibleStatus::Success,
                    Status::Failure => {
                        self.trying = false;
                        self.catch.run_infallible(blackboard)
                    }
                }
            } else {
                self.catch.run_infallible(blackboard)
            };
            
            if !result.is_running() {
                self.trying = true;
            }
            
            result
    }
}