#![feature(never_type)]

#[derive(Clone, Copy, Debug)]
pub enum Status<T> {
    Running(T),
    Success,
    Failure,
}

impl<T> Status<T> {
    pub fn is_ok(&self) -> bool {
        match self {
            Status::Running(_) => false,
            Status::Success => true,
            Status::Failure => false,
        }
    }

    pub fn is_err(&self) -> bool {
        match self {
            Status::Running(_) => false,
            Status::Success => false,
            Status::Failure => true,
        }
    }
}

pub trait Behavior<B, T> {
    fn run(&mut self, blackboard: &mut B) -> Status<T>;
}

impl<T, F: FnMut(&mut B) -> Status<T>, B> Behavior<B, T> for F {
    fn run(&mut self, blackboard: &mut B) -> Status<T> {
        self(blackboard)
    }
}

pub struct IfElse<A, B, C> {
    pub condition: A,
    pub if_true: B,
    pub if_false: C,
}

impl<A, B, C, D, T> Behavior<D, T> for IfElse<A, B, C>
where
    A: Behavior<D, T>,
    B: Behavior<D, T>,
    C: Behavior<D, T>,
{
    fn run(&mut self, blackboard: &mut D) -> Status<T> {
        if self.condition.run(blackboard).is_ok() {
            self.if_true.run(blackboard)
        } else {
            self.if_false.run(blackboard)
        }
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

impl<B, T: Clone> Behavior<B, T> for Status<T> {
    fn run(&mut self, _: &mut B) -> Status<T> {
        match self {
            Status::Running(x) => Status::Running(x.clone()),
            Status::Success => Status::Success,
            Status::Failure => Status::Failure,
        }
    }
}

pub struct WhileLoop<A, B> {
    pub condition: A,
    pub body: B,
}

macro_rules! impl_while {
    ($($name: ident $num: tt)+) => {
        impl<A1, C1, T, $($name,)+> Behavior<C1, T> for WhileLoop<A1, ($($name,)+)>
        where
            A1: Behavior<C1, T>,
            $($name: Behavior<C1, T>,)+
        {
            fn run(&mut self, blackboard: &mut C1) -> Status<T> {
                while self.condition.run(blackboard).is_ok() {
                    $(
                        if self.body.$num.run(blackboard).is_err() {
                            return Status::Failure;
                        }
                    )+
                }
                Status::Success
            }
        }
    }
}

impl_while!(A 0);
impl_while!(A 0 B 1);
impl_while!(A 0 B 1 C 2);

pub struct Sequence<A> {
    pub body: A,
}

macro_rules! impl_seq {
    ($($name: ident $num: tt)+) => {
        impl<C1, T, $($name,)+> Behavior<C1, T> for Sequence<($($name,)+)>
        where
            $($name: Behavior<C1, T>,)+
        {
            fn run(&mut self, blackboard: &mut C1) -> Status<T> {
                $(
                    if self.body.$num.run(blackboard).is_err() {
                        return Status::Failure;
                    }
                )+
                Status::Success
            }
        }
    }
}

impl_seq!(A 0);
impl_seq!(A 0 B 1);
impl_seq!(A 0 B 1 C 2);

pub struct Select<A> {
    pub body: A,
}

macro_rules! impl_sel {
    ($($name: ident $num: tt)+) => {
        impl<C1, T, $($name,)+> Behavior<C1, T> for Select<($($name,)+)>
        where
            $($name: Behavior<C1, T>,)+
        {
            fn run(&mut self, blackboard: &mut C1) -> Status<T> {
                $(
                    if self.body.$num.run(blackboard).is_ok() {
                        return Status::Success;
                    }
                )+
                Status::Failure
            }
        }
    }
}

impl_sel!(A 0);
impl_sel!(A 0 B 1);
impl_sel!(A 0 B 1 C 2);

/// Returns `OK` if `status` is `true`, otherwise returns `ERR`.
pub fn status<T>(status: bool) -> Status<T> {
    if status {
        Status::Success
    } else {
        Status::Failure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sum() {
        let mut sum = 0;
        let is_ok = WhileLoop {
            condition: |sum: &mut usize| status::<()>(*sum < 10),
            body: (|sum: &mut usize| {
                *sum += 1;
                Status::Success
            },),
        }
        .run(&mut sum)
        .is_ok();
        assert!(is_ok);
        assert_eq!(sum, 10);
    }
}
