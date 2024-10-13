pub mod action;
pub mod branching;
pub mod converters;
pub mod looping;
pub mod sequence;

#[derive(Clone, Copy, Debug)]
pub enum Status<T> {
    Running(T),
    Success,
    Failure,
}

impl<T> Status<T> {
    pub const fn is_ok(&self) -> bool {
        match self {
            Self::Running(_) => false,
            Self::Success => true,
            Self::Failure => false,
        }
    }

    pub const fn is_err(&self) -> bool {
        match self {
            Self::Running(_) => false,
            Self::Success => false,
            Self::Failure => true,
        }
    }

    pub const fn is_running(&self) -> bool {
        match self {
            Self::Running(_) => true,
            Self::Success => false,
            Self::Failure => false,
        }
    }
}

impl<T> From<bool> for Status<T> {
    fn from(value: bool) -> Self {
        if value {
            Status::Success
        } else {
            Status::Failure
        }
    }
}

pub enum FallibleStatus<T> {
    Running(T),
    Failure,
}

impl<T> FallibleStatus<T> {
    pub const fn is_ok(&self) -> bool {
        match self {
            Self::Running(_) => false,
            Self::Failure => false,
        }
    }

    pub const fn is_err(&self) -> bool {
        match self {
            Self::Running(_) => false,
            Self::Failure => true,
        }
    }

    pub const fn is_running(&self) -> bool {
        match self {
            Self::Running(_) => true,
            Self::Failure => false,
        }
    }
}

pub enum InfallibleStatus<T> {
    Running(T),
    Success,
}

impl<T> InfallibleStatus<T> {
    pub const fn is_ok(&self) -> bool {
        match self {
            Self::Running(_) => false,
            Self::Success => true,
        }
    }

    pub const fn is_err(&self) -> bool {
        match self {
            Self::Running(_) => false,
            Self::Success => false,
        }
    }

    pub const fn is_running(&self) -> bool {
        match self {
            Self::Running(_) => true,
            Self::Success => false,
        }
    }
}

/// A behavior that runs forever.
pub trait EternalBehavior<B, T> {
    fn run_eternal(&mut self, blackboard: &mut B) -> T;
}

/// A behavior that runs until it fails.
pub trait FallibleBehavior<B, T> {
    fn run_fallible(&mut self, blackboard: &mut B) -> FallibleStatus<T>;
}

/// A behavior that runs until it succeeds.
pub trait InfallibleBehavior<B, T> {
    fn run_infallible(&mut self, blackboard: &mut B) -> InfallibleStatus<T>;
}

/// A behavior that runs until it fails or succeeds.
pub trait Behavior<B, T> {
    fn run(&mut self, blackboard: &mut B) -> Status<T>;
}

#[cfg(test)]
mod tests {
    use looping::WhileLoop;

    use super::*;

    #[test]
    fn test_sum() {
        let mut sum = 0;
        let is_ok = WhileLoop::new(
            |sum: &mut usize| (*sum < 10).into(),
            |sum: &mut usize| {
                *sum += 1;
                InfallibleStatus::<()>::Success
            },
        )
        .run_infallible(&mut sum)
        .is_ok();
        assert!(is_ok);
        assert_eq!(sum, 10);
    }
}
