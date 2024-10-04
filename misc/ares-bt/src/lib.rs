pub type Status = Result<(), ()>;

pub trait Behavior<B> {
    fn run(&mut self, blackboard: &mut B) -> Status;
}

impl<F: FnMut(&mut B) -> Status, B> Behavior<B> for F {
    fn run(&mut self, blackboard: &mut B) -> Status {
        self(blackboard)
    }
}

pub struct IfElse<A, B, C> {
    pub condition: A,
    pub if_true: B,
    pub if_false: C,
}

impl<A, B, C, D> Behavior<D> for IfElse<A, B, C>
where
    A: Behavior<D>,
    B: Behavior<D>,
    C: Behavior<D>,
{
    fn run(&mut self, blackboard: &mut D) -> Status {
        if self.condition.run(blackboard).is_ok() {
            self.if_true.run(blackboard)
        } else {
            self.if_false.run(blackboard)
        }
    }
}

pub struct Invert<A>(pub A);

impl<A, B> Behavior<B> for Invert<A>
where
    A: Behavior<B>,
{
    fn run(&mut self, blackboard: &mut B) -> Status {
        match self.0.run(blackboard) {
            Ok(_) => Err(()),
            Err(_) => Ok(()),
        }
    }
}

impl<B> Behavior<B> for Status {
    fn run(&mut self, _: &mut B) -> Status {
        *self
    }
}

pub struct WhileLoop<A, B> {
    pub condition: A,
    pub body: B,
}

macro_rules! impl_while {
    ($($name: ident $num: tt)+) => {
        impl<A1, C1, $($name,)+> Behavior<C1> for WhileLoop<A1, ($($name,)+)>
        where
            A1: Behavior<C1>,
            $($name: Behavior<C1>,)+
        {
            fn run(&mut self, blackboard: &mut C1) -> Status {
                while self.condition.run(blackboard).is_ok() {
                    $(
                        self.body.$num.run(blackboard)?;
                    )+
                }
                Ok(())
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
        impl<C1, $($name,)+> Behavior<C1> for Sequence<($($name,)+)>
        where
            $($name: Behavior<C1>,)+
        {
            fn run(&mut self, blackboard: &mut C1) -> Status {
                $(
                    self.body.$num.run(blackboard)?;
                )+
                Ok(())
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
        impl<C1, $($name,)+> Behavior<C1> for Select<($($name,)+)>
        where
            $($name: Behavior<C1>,)+
        {
            fn run(&mut self, blackboard: &mut C1) -> Status {
                $(
                    if self.body.$num.run(blackboard).is_ok() {
                        return Ok(());
                    }
                )+
                Err(())
            }
        }
    }
}

impl_sel!(A 0);
impl_sel!(A 0 B 1);
impl_sel!(A 0 B 1 C 2);

/// Returns `OK` if `status` is `true`, otherwise returns `ERR`.
pub fn status(status: bool) -> Status {
    if status {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sum() {
        let mut sum = 0;
        WhileLoop {
            condition: |sum: &mut usize| status(*sum < 10),
            body: (|sum: &mut usize| {
                *sum += 1;
                Ok(())
            },),
        }
        .run(&mut sum)
        .unwrap();
        assert_eq!(sum, 10);
    }
}
