use crate::{
    Behavior, EternalBehavior, EternalStatus, FallibleBehavior, FallibleStatus, InfallibleBehavior,
    InfallibleStatus, IntoRon, Status,
};

pub struct WhileLoop<A, B> {
    pub condition: A,
    pub body: B,
    check_condition: bool,
}

impl<A, B> IntoRon for WhileLoop<A, B>
where
    A: IntoRon,
    B: IntoRon,
{
    fn into_ron(&self) -> ron::Value {
        ron::Value::Map(
            [
                (
                    ron::Value::String("condition".to_string()),
                    self.condition.into_ron(),
                ),
                (ron::Value::String("body".to_string()), self.body.into_ron()),
            ]
            .into_iter()
            .collect(),
        )
    }
}

impl<A, B, D, T> Behavior<D, T> for WhileLoop<A, B>
where
    A: Behavior<D, T>,
    B: Behavior<D, T>,
{
    fn run(&mut self, blackboard: &mut D) -> Status<T> {
        loop {
            if self.check_condition {
                match self.condition.run(blackboard) {
                    Status::Running(t) => return Status::Running(t),
                    Status::Success => self.check_condition = false,
                    Status::Failure => return Status::Success,
                }
            }
            match self.body.run(blackboard) {
                Status::Running(t) => return Status::Running(t),
                Status::Success => self.check_condition = true,
                Status::Failure => {
                    self.check_condition = true;
                    return Status::Failure;
                }
            }
        }
    }
}

impl<A, B, D, T> FallibleBehavior<D, T> for WhileLoop<A, B>
where
    A: InfallibleBehavior<D, T>,
    B: FallibleBehavior<D, T>,
{
    fn run_fallible(&mut self, blackboard: &mut D) -> FallibleStatus<T> {
        loop {
            if self.check_condition {
                match self.condition.run_infallible(blackboard) {
                    InfallibleStatus::Running(t) => return FallibleStatus::Running(t),
                    InfallibleStatus::Success => self.check_condition = false,
                }
            }
            match self.body.run_fallible(blackboard) {
                FallibleStatus::Running(t) => return FallibleStatus::Running(t),
                FallibleStatus::Failure => {
                    self.check_condition = true;
                    return FallibleStatus::Failure;
                }
            }
        }
    }
}

impl<A, B, D, T> InfallibleBehavior<D, T> for WhileLoop<A, B>
where
    A: Behavior<D, T>,
    B: InfallibleBehavior<D, T>,
{
    fn run_infallible(&mut self, blackboard: &mut D) -> InfallibleStatus<T> {
        loop {
            if self.check_condition {
                match self.condition.run(blackboard) {
                    Status::Running(t) => return InfallibleStatus::Running(t),
                    Status::Success => self.check_condition = false,
                    Status::Failure => return InfallibleStatus::Success,
                }
            }
            match self.body.run_infallible(blackboard) {
                InfallibleStatus::Running(t) => return InfallibleStatus::Running(t),
                InfallibleStatus::Success => self.check_condition = true,
            }
        }
    }
}

impl<A, B, D, T> EternalBehavior<D, T> for WhileLoop<A, B>
where
    A: InfallibleBehavior<D, T>,
    B: InfallibleBehavior<D, T>,
{
    fn run_eternal(&mut self, blackboard: &mut D) -> EternalStatus<T> {
        loop {
            if self.check_condition {
                match self.condition.run_infallible(blackboard) {
                    InfallibleStatus::Running(t) => return EternalStatus::Running(t),
                    InfallibleStatus::Success => self.check_condition = false,
                }
            }
            match self.body.run_infallible(blackboard) {
                InfallibleStatus::Running(t) => return EternalStatus::Running(t),
                InfallibleStatus::Success => self.check_condition = true,
            }
        }
    }
}

impl<A, B> WhileLoop<A, B> {
    pub fn new(condition: A, body: B) -> Self {
        Self {
            condition,
            body,
            check_condition: true,
        }
    }
}
