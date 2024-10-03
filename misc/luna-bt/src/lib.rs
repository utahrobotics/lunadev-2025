use std::{borrow::Cow, marker::PhantomData};

pub type Status = Result<(), ()>;
pub const OK: Status = Ok(());
pub const ERR: Status = Err(());
pub type Action<'a, B> = Box<dyn FnMut(&mut B) -> Status + 'a>;
pub type StaticBehaviour<B> = Behaviour<'static, B>;

/// Returns `OK` if `status` is `true`, otherwise returns `ERR`.
pub fn status(status: bool) -> Status {
    if status {
        OK
    } else {
        ERR
    }
}

pub enum Behaviour<'a, B> {
    /// A function that takes a mutable reference to the blackboard and returns a status
    Action(Action<'a, B>),
    /// A function that takes a mutable reference to the blackboard and returns nothing.
    ///
    /// `OK` is always returned.
    InfallibleAction(Box<dyn FnMut(&mut B) + 'a>),
    /// Runs `ok` if `condition` returns `OK`, otherwise runs `err`.
    If {
        condition: Box<Self>,
        ok: Box<Self>,
        err: Box<Self>,
    },
    /// Runs behaviors sequentially while the condition returns `OK`.
    While {
        condition: Box<Self>,
        body: Vec<Self>,
        /// If `true`, `condition` is checked before every behavior in `body`.
        pedantic: bool,
    },
    /// Runs behaviors sequentially, exiting early if one succeeds
    Select(Vec<Self>),
    /// Runs behaviors sequentially, exiting early if one fails
    Sequence(Vec<Self>),
    /// Runs a behavior, but ignores its status
    Ignore(Box<Self>, Status),
    /// Inverts the result of a behavior
    Invert(Box<Self>),
    /// Runs a behavior, but runs a behaviour if it succeeds, then exits the Behavior Tree with the status of `succeeded`
    NeverSucceed {
        behavior: Box<Self>,
        succeeded: Action<'a, B>,
    },
    /// Runs a behavior, but runs a behaviour if it fails, then exits the Behavior Tree with the status of `failed`
    NeverFail {
        behavior: Box<Self>,
        failed: Action<'a, B>,
    },
    /// Exits the Behavior Tree with the given status
    Exit(Status),
    /// Returns the given status
    Constant(Status),
}

enum ReturnStatus {
    Normal(Status),
    Exit(Status),
}

trait BehaviorTracer<B> {
    fn push(&mut self, name: Cow<'static, str>, bb: &B);

    fn pop(&mut self, bb: &B);
}

struct SimpleTracer<B, F> {
    stack: Vec<Cow<'static, str>>,
    trace_fn: F,
    _phantom: PhantomData<fn() -> B>,
}

impl<B, F: FnMut(&B, &[Cow<'static, str>])> BehaviorTracer<B> for SimpleTracer<B, F> {
    fn push(&mut self, name: Cow<'static, str>, bb: &B) {
        self.stack.push(name);
        (self.trace_fn)(bb, &self.stack);
    }
    fn pop(&mut self, bb: &B) {
        self.stack.pop();
        (self.trace_fn)(bb, &self.stack);
    }
}

impl<B> BehaviorTracer<B> for () {
    fn push(&mut self, _name: Cow<'static, str>, _bb: &B) {}
    fn pop(&mut self, _bb: &B) {}
}

impl<'a, B> Behaviour<'a, B> {
    pub fn run(&mut self, blackboard: &mut B) -> Status {
        match self.run_inner(blackboard, &mut ()) {
            ReturnStatus::Normal(x) => x,
            ReturnStatus::Exit(x) => x,
        }
    }

    pub fn run_traced(
        &mut self,
        blackboard: &mut B,
        trace_fn: impl FnMut(&B, &[Cow<'static, str>]),
    ) -> Status {
        let mut tracer = SimpleTracer {
            stack: Vec::new(),
            trace_fn,
            _phantom: PhantomData,
        };
        match self.run_inner(blackboard, &mut tracer) {
            ReturnStatus::Normal(x) => x,
            ReturnStatus::Exit(x) => x,
        }
    }

    fn run_inner(
        &mut self,
        blackboard: &mut B,
        trace_stack: &mut impl BehaviorTracer<B>,
    ) -> ReturnStatus {
        macro_rules! unwrap {
            ($val: expr) => {
                match $val {
                    ReturnStatus::Normal(x) => x,
                    ReturnStatus::Exit(x) => return ReturnStatus::Exit(x),
                }
            };
        }
        match self {
            Behaviour::Action(action) => {
                trace_stack.push("Action".into(), blackboard);
                let result = action(blackboard);
                trace_stack.pop(blackboard);
                ReturnStatus::Normal(result)
            }
            Behaviour::InfallibleAction(action) => {
                trace_stack.push("InfallibleAction".into(), blackboard);
                action(blackboard);
                trace_stack.pop(blackboard);
                ReturnStatus::Normal(OK)
            }
            Behaviour::If { condition, ok, err } => {
                trace_stack.push("If".into(), blackboard);
                let result = if unwrap!(condition.run_inner(blackboard, trace_stack)).is_ok() {
                    trace_stack.pop(blackboard);
                    trace_stack.push("If-Ok".into(), blackboard);
                    ok.run_inner(blackboard, trace_stack)
                } else {
                    trace_stack.pop(blackboard);
                    trace_stack.push("If-Err".into(), blackboard);
                    err.run_inner(blackboard, trace_stack)
                };
                trace_stack.pop(blackboard);
                result
            }
            Behaviour::While {
                condition,
                body,
                pedantic,
            } => {
                if *pedantic {
                    loop {
                        for (i, behavior) in body.iter_mut().enumerate() {
                            trace_stack.push("While-Condition".into(), blackboard);
                            if unwrap!(condition.run_inner(blackboard, trace_stack)).is_err() {
                                trace_stack.pop(blackboard);
                                break;
                            }
                            trace_stack.pop(blackboard);
                            trace_stack.push(format!("While-{i}").into(), blackboard);
                            if unwrap!(behavior.run_inner(blackboard, trace_stack)).is_err() {
                                trace_stack.pop(blackboard);
                                return ReturnStatus::Normal(ERR);
                            }
                            trace_stack.pop(blackboard);
                        }
                    }
                } else {
                    loop {
                        trace_stack.push("While-Condition".into(), blackboard);
                        if unwrap!(condition.run_inner(blackboard, trace_stack)).is_err() {
                            trace_stack.pop(blackboard);
                            break;
                        }
                        trace_stack.pop(blackboard);
                        for (i, behavior) in body.iter_mut().enumerate() {
                            trace_stack.push(format!("While-{i}").into(), blackboard);
                            if unwrap!(behavior.run_inner(blackboard, trace_stack)).is_err() {
                                trace_stack.pop(blackboard);
                                return ReturnStatus::Normal(ERR);
                            }
                            trace_stack.pop(blackboard);
                        }
                    }
                }
                ReturnStatus::Normal(OK)
            }
            Behaviour::Select(behaviors) => {
                for (i, behavior) in behaviors.iter_mut().enumerate() {
                    trace_stack.push(format!("Select-{i}").into(), blackboard);
                    if unwrap!(behavior.run_inner(blackboard, trace_stack)).is_ok() {
                        trace_stack.pop(blackboard);
                        return ReturnStatus::Normal(OK);
                    }
                    trace_stack.pop(blackboard);
                }
                ReturnStatus::Normal(ERR)
            }
            Behaviour::Sequence(behaviors) => {
                for (i, behavior) in behaviors.iter_mut().enumerate() {
                    trace_stack.push(format!("Sequence-{i}").into(), blackboard);
                    if unwrap!(behavior.run_inner(blackboard, trace_stack)).is_err() {
                        trace_stack.pop(blackboard);
                        return ReturnStatus::Normal(ERR);
                    }
                    trace_stack.pop(blackboard);
                }
                ReturnStatus::Normal(OK)
            }
            Behaviour::Ignore(behavior, replace) => {
                trace_stack.push("Ignore".into(), blackboard);
                let _ = unwrap!(behavior.run_inner(blackboard, trace_stack));
                trace_stack.pop(blackboard);
                ReturnStatus::Normal(*replace)
            }
            Behaviour::Invert(behavior) => {
                trace_stack.push("Invert".into(), blackboard);
                let result = if unwrap!(behavior.run_inner(blackboard, trace_stack)).is_ok() {
                    ReturnStatus::Normal(ERR)
                } else {
                    ReturnStatus::Normal(OK)
                };
                trace_stack.pop(blackboard);
                result
            }
            Behaviour::NeverSucceed {
                behavior,
                succeeded,
            } => {
                trace_stack.push("NeverSucceed".into(), blackboard);
                if unwrap!(behavior.run_inner(blackboard, trace_stack)).is_ok() {
                    trace_stack.pop(blackboard);
                    trace_stack.push("NeverSucceed-Exit".into(), blackboard);
                    ReturnStatus::Exit(succeeded(blackboard))
                } else {
                    trace_stack.pop(blackboard);
                    ReturnStatus::Normal(ERR)
                }
            }
            Behaviour::NeverFail { behavior, failed } => {
                trace_stack.push("NeverFail".into(), blackboard);
                if unwrap!(behavior.run_inner(blackboard, trace_stack)).is_err() {
                    trace_stack.pop(blackboard);
                    trace_stack.push("NeverFail-Exit".into(), blackboard);
                    ReturnStatus::Exit(failed(blackboard))
                } else {
                    trace_stack.pop(blackboard);
                    ReturnStatus::Normal(OK)
                }
            }
            Behaviour::Exit(status) => {
                trace_stack.push("Exit".into(), blackboard);
                ReturnStatus::Exit(*status)
            }
            Behaviour::Constant(status) => {
                trace_stack.push("Constant".into(), blackboard);
                trace_stack.pop(blackboard);
                ReturnStatus::Normal(*status)
            }
        }
    }

    pub fn action(action: impl FnMut(&mut B) -> Status + 'a) -> Self {
        Self::Action(Box::new(action))
    }

    pub fn if_else(condition: Self, ok: Self, err: Self) -> Self {
        Self::If {
            condition: Box::new(condition),
            ok: Box::new(ok),
            err: Box::new(err),
        }
    }

    pub fn while_loop(
        condition: impl Into<Box<Self>>,
        body: impl IntoIterator<Item = Self>,
    ) -> Self {
        Self::While {
            condition: condition.into(),
            body: body.into_iter().collect(),
            pedantic: false,
        }
    }

    pub fn while_loop_pedantic(
        condition: impl Into<Box<Self>>,
        body: impl IntoIterator<Item = Self>,
    ) -> Self {
        Self::While {
            condition: condition.into(),
            body: body.into_iter().collect(),
            pedantic: true,
        }
    }

    pub fn select(behaviors: impl IntoIterator<Item = Self>) -> Self {
        Self::Select(behaviors.into_iter().collect())
    }

    pub fn sequence(behaviors: impl IntoIterator<Item = Self>) -> Self {
        Self::Sequence(behaviors.into_iter().collect())
    }

    pub fn ignore(behavior: impl Into<Box<Self>>, with: Status) -> Self {
        Self::Ignore(behavior.into(), with)
    }

    pub fn invert(behavior: impl Into<Box<Self>>) -> Self {
        Self::Invert(behavior.into())
    }

    pub fn never_succeed(
        behavior: impl Into<Box<Self>>,
        succeeded: impl Fn(&mut B) -> Status + 'a,
    ) -> Self {
        Self::NeverSucceed {
            behavior: behavior.into(),
            succeeded: Box::new(succeeded),
        }
    }

    pub fn never_fail(
        behavior: impl Into<Box<Self>>,
        failed: impl Fn(&mut B) -> Status + 'a,
    ) -> Self {
        Self::NeverFail {
            behavior: behavior.into(),
            failed: Box::new(failed),
        }
    }

    pub fn exit(status: Status) -> Self {
        Self::Exit(status)
    }

    pub fn constant(status: Status) -> Self {
        Self::Constant(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_sum() {
        let mut sum = 0usize;
        Behaviour::while_loop(
            Behaviour::action(|sum| status(*sum < 10)),
            [Behaviour::action(|sum| {
                *sum += 1;
                OK
            })],
        )
        .run(&mut sum)
        .unwrap();

        assert_eq!(sum, 10);
    }

    #[test]
    fn early_exit_sum() {
        let mut sum = 0usize;
        Behaviour::while_loop(
            Behaviour::action(|sum| status(*sum < 10)),
            [
                Behaviour::action(|sum| {
                    *sum += 1;
                    OK
                }),
                Behaviour::if_else(
                    Behaviour::action(|sum| status(*sum == 7)),
                    Behaviour::exit(OK),
                    Behaviour::constant(OK),
                ),
            ],
        )
        .run(&mut sum)
        .unwrap();

        assert_eq!(sum, 7);
    }
}
