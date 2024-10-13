pub mod converters;
pub mod action;
pub mod branching;
pub mod sequence;
pub mod looping;

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

/// Returns `Success` if `status` is `true`, otherwise returns `Failure`.
pub fn status<T>(status: bool) -> Status<T> {
    if status {
        Status::Success
    } else {
        Status::Failure
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_sum() {
//         let mut sum = 0;
//         let is_ok = WhileLoop {
//             condition: |sum: &mut usize| status::<()>(*sum < 10),
//             body: (|sum: &mut usize| {
//                 *sum += 1;
//                 Status::Success
//             },),
//         }
//         .run(&mut sum)
//         .is_ok();
//         assert!(is_ok);
//         assert_eq!(sum, 10);
//     }
// }
