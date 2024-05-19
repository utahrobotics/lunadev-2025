use std::{
    any::Any,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::{Arc, OnceLock},
};

/// A `State` is a unit of computation.
///
/// In a State Machine, only 1 `State` can run at a time. When a `State` finishes,
/// it is transitioned into a new `State`, which immediately begins to run, or the
/// State Machine terminates. `State`s are stateless, which means the information
/// they work on should be contained in the blackboard, which is passed from `State`
/// to `State`. All `State`s in a State Machine must use the same type of blackboard.
///
/// In this framework, a `State` is simply an async `fn` pointer, which is more likely to
/// be pure unlike closures. The return value of this `fn` is passed to the transition
/// function (if it exists) to determine the next state to transition to.
pub struct State<S> {
    main: Arc<dyn Fn(S) -> Pin<Box<dyn Future<Output = (S, Box<dyn Any>)> + Send>> + Send + Sync>,
    transition: Arc<OnceLock<Box<dyn Fn(Box<dyn Any>) -> Option<State<S>> + Send + Sync>>>,
}

impl<S> Clone for State<S> {
    fn clone(&self) -> Self {
        Self {
            main: self.main.clone(),
            transition: self.transition.clone(),
        }
    }
}

/// The logic for the transition of a finished `State`.
///
/// When a `State` finishes, it returns a value that is handled
/// by its transition, which tells the State Machine what to do.
pub struct StateTransition<S, R> {
    transition: Arc<OnceLock<Box<dyn Fn(Box<dyn Any>) -> Option<State<S>> + Send + Sync>>>,
    _phantom: PhantomData<R>,
}

/// The return value of all `State`s.
///
/// Since borrowing closures do not exist yet, async closures must
/// receive and yield ownership of the blackboard when they are entered or exited.
/// This struct ensures that.
pub struct StateResult<S, R = ()> {
    pub blackboard: S,
    pub result: R,
}

impl<S, R> StateResult<S, R> {
    #[must_use]
    pub fn new(blackboard: S, result: R) -> Self {
        Self { blackboard, result }
    }
}

impl<S> From<S> for StateResult<S, ()> {
    fn from(blackboard: S) -> Self {
        Self {
            blackboard,
            result: (),
        }
    }
}

impl<S: 'static + Send> State<S> {
    /// Creates a new `State` from the given async `fn` pointer.
    ///
    /// The `State`, and its `StateTransition` is returned. The `StateTransition` can be
    /// used to configure the logic of the transition.
    #[must_use]
    pub fn new<R: 'static, Fut: Future<Output = StateResult<S, R>> + Send + 'static>(
        main: fn(S) -> Fut,
    ) -> (Self, StateTransition<S, R>) {
        let transition = Arc::new(OnceLock::new());
        let main: Box<
            dyn Fn(S) -> Pin<Box<dyn Future<Output = (S, Box<dyn Any>)> + Send>> + Send + Sync,
        > = Box::new(move |s| {
            Box::pin(async move {
                let StateResult { blackboard, result } = main(s).await;
                let result: Box<dyn Any> = Box::new(result);
                (blackboard, result)
            })
        });
        (
            Self {
                main: Arc::from(main),
                transition: transition.clone(),
            },
            StateTransition {
                transition,
                _phantom: PhantomData,
            },
        )
    }

    /// Starts up a State Machine with this `State` being
    /// the initial `State`.
    ///
    /// An initial blackboard is provided, and is subsequently returned
    /// when a `State` has no transitions.
    ///
    /// The State Machine is defined by the combination of `State`s
    /// and transitions, so there isn't a State Machine type. As long
    /// as several `State`s are transitioning between each other, any
    /// `State` can be used as the initial `State` for a State Machine.
    ///
    /// All `State`s in the State Machine are ran on the current thread.
    pub async fn start(&self, blackboard: S) -> S {
        let mut fut = (self.main)(blackboard);
        let mut transition = self.transition.clone();

        loop {
            let (bb, r) = fut.await;
            let Some(next) = transition.get().and_then(|x| x(r)) else {
                break bb;
            };
            transition = next.transition.clone();
            fut = (next.main)(bb);
        }
    }
}

impl<S: 'static, R: 'static> StateTransition<S, R> {
    /// Sets the transition logic for the associated `State`.
    ///
    /// The given closure must accept the return value of the finished `State`
    /// and return the next `State` to transition to. If `None` is returned,
    /// the `State` will have no transitions, causing the State Machine to terminate.
    ///
    /// Not calling this method has the same effect as passing a closure that always returns `None`.
    ///
    /// It is strongly encouraged that the given closure be pure. If the transition logic depends on
    /// the blackboard, then that should be inside the `State` itself and reflected in its returned
    /// value.
    pub fn set_transition(self, f: impl Fn(R) -> Option<State<S>> + Send + Sync + 'static) {
        let _ = self.transition.set(Box::new(move |any| {
            f(*Box::<dyn Any>::downcast::<R>(any).unwrap())
        }));
    }
}

#[cfg(test)]
mod tests {
    use crate::{State, StateResult};

    #[tokio::test]
    async fn adder_test() {
        let (adder, adder_trans) = State::new(|mut num: usize| async move {
            num += 1;
            StateResult::from(num)
        });
        let (checker, checker_trans) =
            State::new(|num: usize| async move { StateResult::new(num, num == 10000) });

        let start_state = adder.clone();

        adder_trans.set_transition(move |_| Some(checker.clone()));
        checker_trans.set_transition(move |equals| if equals { None } else { Some(adder.clone()) });

        assert_eq!(start_state.start(0).await, 10000);
    }

    #[tokio::test]
    async fn null_test() {
        let (adder, _) = State::new(|mut num: usize| async move {
            num += 1;
            StateResult::from(num)
        });
        let (_, checker_trans) =
            State::new(|num: usize| async move { StateResult::new(num, num == 10000) });

        let start_state = adder.clone();

        checker_trans.set_transition(move |equals| if equals { None } else { Some(adder.clone()) });

        assert_eq!(start_state.start(0).await, 1);
    }
}
