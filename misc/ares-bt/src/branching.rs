use crate::{
    Behavior, EternalBehavior, FallibleBehavior, FallibleStatus, InfallibleBehavior,
    InfallibleStatus, IntoRon, Status,
};

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

impl<A, B, C> IntoRon for IfElse<A, B, C>
where
    A: IntoRon,
    B: IntoRon,
    C: IntoRon,
{
    fn into_ron(&self) -> ron::Value {
        ron::Value::Map(
            [
                (
                    ron::Value::String("condition".to_string()),
                    self.condition.into_ron(),
                ),
                (
                    ron::Value::String("success".to_string()),
                    self.if_true.into_ron(),
                ),
                (
                    ron::Value::String("failure".to_string()),
                    self.if_false.into_ron(),
                ),
            ]
            .into_iter()
            .collect(),
        )
    }
}

impl<A, B, C, D, T> Behavior<D, T> for IfElse<A, B, C>
where
    A: Behavior<D, T>,
    B: Behavior<D, T>,
    C: Behavior<D, T>,
{
    fn run(&mut self, blackboard: &mut D) -> Status<T> {
        let result = match self.state {
            IfElseState::Condition => match self.condition.run(blackboard) {
                Status::Running(t) => return Status::Running(t),
                Status::Success => {
                    self.state = IfElseState::IfTrue;
                    self.if_true.run(blackboard)
                }
                Status::Failure => {
                    self.state = IfElseState::IfFalse;
                    self.if_false.run(blackboard)
                }
            },
            IfElseState::IfTrue => self.if_true.run(blackboard),
            IfElseState::IfFalse => self.if_false.run(blackboard),
        };

        if !result.is_running() {
            self.state = IfElseState::Condition;
        }

        result
    }
}

impl<A, B, C, D, T> InfallibleBehavior<D, T> for IfElse<A, B, C>
where
    A: Behavior<D, T>,
    B: InfallibleBehavior<D, T>,
    C: InfallibleBehavior<D, T>,
{
    fn run_infallible(&mut self, blackboard: &mut D) -> InfallibleStatus<T> {
        let result = match self.state {
            IfElseState::Condition => match self.condition.run(blackboard) {
                Status::Running(t) => return InfallibleStatus::Running(t),
                Status::Success => {
                    self.state = IfElseState::IfTrue;
                    self.if_true.run_infallible(blackboard)
                }
                Status::Failure => {
                    self.state = IfElseState::IfFalse;
                    self.if_false.run_infallible(blackboard)
                }
            },
            IfElseState::IfTrue => self.if_true.run_infallible(blackboard),
            IfElseState::IfFalse => self.if_false.run_infallible(blackboard),
        };

        if !result.is_running() {
            self.state = IfElseState::Condition;
        }

        result
    }
}

impl<A, B, C, D, T> FallibleBehavior<D, T> for IfElse<A, B, C>
where
    A: Behavior<D, T>,
    B: FallibleBehavior<D, T>,
    C: FallibleBehavior<D, T>,
{
    fn run_fallible(&mut self, blackboard: &mut D) -> FallibleStatus<T> {
        let result = match self.state {
            IfElseState::Condition => match self.condition.run(blackboard) {
                Status::Running(t) => return FallibleStatus::Running(t),
                Status::Success => {
                    self.state = IfElseState::IfTrue;
                    self.if_true.run_fallible(blackboard)
                }
                Status::Failure => {
                    self.state = IfElseState::IfFalse;
                    self.if_false.run_fallible(blackboard)
                }
            },
            IfElseState::IfTrue => self.if_true.run_fallible(blackboard),
            IfElseState::IfFalse => self.if_false.run_fallible(blackboard),
        };

        if !result.is_running() {
            self.state = IfElseState::Condition;
        }

        result
    }
}

impl<A, B, C, D, T> EternalBehavior<D, T> for IfElse<A, B, C>
where
    A: Behavior<D, T>,
    B: EternalBehavior<D, T>,
    C: EternalBehavior<D, T>,
{
    fn run_eternal(&mut self, blackboard: &mut D) -> T {
        match self.state {
            IfElseState::Condition => match self.condition.run(blackboard) {
                Status::Running(t) => return t,
                Status::Success => {
                    self.state = IfElseState::IfTrue;
                    self.if_true.run_eternal(blackboard)
                }
                Status::Failure => {
                    self.state = IfElseState::IfFalse;
                    self.if_false.run_eternal(blackboard)
                }
            },
            IfElseState::IfTrue => self.if_true.run_eternal(blackboard),
            IfElseState::IfFalse => self.if_false.run_eternal(blackboard),
        }
    }
}

impl<A, B, C> IfElse<A, B, C> {
    pub fn new(condition: A, if_true: B, if_false: C) -> Self {
        Self {
            condition,
            if_true,
            if_false,
            state: IfElseState::Condition,
        }
    }
}

pub struct TryCatch<A, B> {
    pub try_behavior: A,
    pub catch: B,
    trying: bool,
}

impl<A, B> IntoRon for TryCatch<A, B>
where
    A: IntoRon,
    B: IntoRon,
{
    fn into_ron(&self) -> ron::Value {
        ron::Value::Map(
            [
                (
                    ron::Value::String("try".to_string()),
                    self.try_behavior.into_ron(),
                ),
                (
                    ron::Value::String("catch".to_string()),
                    self.catch.into_ron(),
                ),
            ]
            .into_iter()
            .collect(),
        )
    }
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

impl<A, B, D, T> FallibleBehavior<D, T> for TryCatch<A, B>
where
    A: FallibleBehavior<D, T>,
    B: FallibleBehavior<D, T>,
{
    fn run_fallible(&mut self, blackboard: &mut D) -> FallibleStatus<T> {
        let result = if self.trying {
            match self.try_behavior.run_fallible(blackboard) {
                FallibleStatus::Running(t) => return FallibleStatus::Running(t),
                FallibleStatus::Failure => {
                    self.trying = false;
                    self.catch.run_fallible(blackboard)
                }
            }
        } else {
            self.catch.run_fallible(blackboard)
        };

        if !result.is_running() {
            self.trying = true;
        }

        result
    }
}

impl<A, B, D, T> EternalBehavior<D, T> for TryCatch<A, B>
where
    A: EternalBehavior<D, T>,
{
    fn run_eternal(&mut self, blackboard: &mut D) -> T {
        self.trying = true;
        self.try_behavior.run_eternal(blackboard)
    }
}

impl<A, B> TryCatch<A, B> {
    pub fn new(try_behavior: A, catch: B) -> Self {
        Self {
            try_behavior,
            catch,
            trying: true,
        }
    }
}
