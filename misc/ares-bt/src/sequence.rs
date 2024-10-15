use crate::{
    Behavior, CancelSafe, EternalBehavior, EternalStatus, FallibleBehavior, FallibleStatus,
    InfallibleBehavior, InfallibleStatus, IntoRon, Status,
};

pub struct Sequence<A> {
    pub body: A,
    index: usize,
}

macro_rules! impl_seq {
    ($len: literal $($name: ident $num: tt)+) => {
        impl<C1, T, $($name,)+> Behavior<C1, T> for Sequence<($($name,)+)>
        where
            $($name: Behavior<C1, T>,)+
        {
            fn run(&mut self, blackboard: &mut C1) -> Status<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run(blackboard) {
                                Status::Running(t) => return Status::Running(t),
                                Status::Success => {
                                    self.index += 1;
                                }
                                Status::Failure => {
                                    self.index = 0;
                                    return Status::Failure;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                            return Status::Success;
                        }
                    }
                }
            }
        }
        impl<$($name,)+> CancelSafe for Sequence<($($name,)+)>
        where
            $($name: CancelSafe,)+
        {
            fn reset(&mut self) {
                self.index = 0;
                $(
                    self.body.$num.reset();
                )+
            }
        }
        impl<C1, T, $($name,)+> InfallibleBehavior<C1, T> for Sequence<($($name,)+)>
        where
            $($name: InfallibleBehavior<C1, T>,)+
        {
            fn run_infallible(&mut self, blackboard: &mut C1) -> InfallibleStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_infallible(blackboard) {
                                InfallibleStatus::Running(t) => return InfallibleStatus::Running(t),
                                InfallibleStatus::Success => {
                                    self.index += 1;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                            return InfallibleStatus::Success;
                        }
                    }
                }
            }
        }
        impl<C1, T, $($name,)+> FallibleBehavior<C1, T> for Sequence<($($name,)+)>
        where
            $($name: FallibleBehavior<C1, T>,)+
        {
            fn run_fallible(&mut self, blackboard: &mut C1) -> FallibleStatus<T> {
                self.index = 0;
                self.body.0.run_fallible(blackboard)
            }
        }
        impl<C1, T, $($name,)+> EternalBehavior<C1, T> for Sequence<($($name,)+)>
        where
            $($name: EternalBehavior<C1, T>,)+
        {
            fn run_eternal(&mut self, blackboard: &mut C1) -> EternalStatus<T> {
                self.index = 0;
                self.body.0.run_eternal(blackboard)
            }
        }

        impl<$($name,)+> IntoRon for Sequence<($($name,)+)>
        where
            $($name: IntoRon,)+
        {
            fn into_ron(&self) -> ron::Value {
                ron::Value::Map(
                    [
                        (ron::Value::String("sequence".to_string()), ron::Value::Seq(
                            vec![
                                $(
                                    self.body.$num.into_ron(),
                                )+
                            ].into_iter().collect()
                        ))
                    ].into_iter().collect()
                )
            }
        }
    }
}

impl_seq!(1 A 0);
impl_seq!(2 A 0 B 1);
impl_seq!(3 A 0 B 1 C 2);

impl<A> Sequence<A> {
    pub fn new(body: A) -> Self {
        Self { body, index: 0 }
    }
}

pub struct Select<A> {
    pub body: A,
    index: usize,
}

macro_rules! impl_sel {
    ($len: literal $($name: ident $num: tt)+) => {
        impl<C1, T, $($name,)+> Behavior<C1, T> for Select<($($name,)+)>
        where
            $($name: Behavior<C1, T>,)+
        {
            fn run(&mut self, blackboard: &mut C1) -> Status<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run(blackboard) {
                                Status::Running(t) => return Status::Running(t),
                                Status::Success => {
                                    self.index = 0;
                                    return Status::Success;
                                }
                                Status::Failure => {
                                    self.index += 1;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                            return Status::Failure;
                        }
                    }
                }
            }
        }
        impl<$($name,)+> CancelSafe for Select<($($name,)+)>
        where
            $($name: CancelSafe,)+
        {
            fn reset(&mut self) {
                self.index = 0;
                $(
                    self.body.$num.reset();
                )+
            }
        }
        impl<C1, T, $($name,)+> InfallibleBehavior<C1, T> for Select<($($name,)+)>
        where
            $($name: InfallibleBehavior<C1, T>,)+
        {
            fn run_infallible(&mut self, blackboard: &mut C1) -> InfallibleStatus<T> {
                self.index = 0;
                self.body.0.run_infallible(blackboard)
            }
        }
        impl<C1, T, $($name,)+> FallibleBehavior<C1, T> for Select<($($name,)+)>
        where
            $($name: FallibleBehavior<C1, T>,)+
        {
            fn run_fallible(&mut self, blackboard: &mut C1) -> FallibleStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_fallible(blackboard) {
                                FallibleStatus::Running(t) => return FallibleStatus::Running(t),
                                FallibleStatus::Failure => {
                                    self.index += 1;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                            return FallibleStatus::Failure;
                        }
                    }
                }
            }
        }
        impl<C1, T, $($name,)+> EternalBehavior<C1, T> for Select<($($name,)+)>
        where
            $($name: EternalBehavior<C1, T>,)+
        {
            fn run_eternal(&mut self, blackboard: &mut C1) -> EternalStatus<T> {
                self.index = 0;
                self.body.0.run_eternal(blackboard)
            }
        }

        impl<$($name,)+> IntoRon for Select<($($name,)+)>
        where
            $($name: IntoRon,)+
        {
            fn into_ron(&self) -> ron::Value {
                ron::Value::Map(
                    [
                        (ron::Value::String("select".to_string()), ron::Value::Seq(
                            vec![
                                $(
                                    self.body.$num.into_ron(),
                                )+
                            ].into_iter().collect()
                        ))
                    ].into_iter().collect()
                )
            }
        }
    }
}

impl_sel!(1 A 0);
impl_sel!(2 A 0 B 1);
impl_sel!(3 A 0 B 1 C 2);

impl<A> Select<A> {
    pub fn new(body: A) -> Self {
        Self { body, index: 0 }
    }
}

pub struct ParallelSequence<A> {
    pub body: A,
    index: usize,
    succeeded: usize,
}

macro_rules! impl_seq {
    ($len: literal $($name: ident $num: tt)+) => {
        impl<C1, T, $($name,)+> Behavior<C1, T> for ParallelSequence<($($name,)+)>
        where
            $($name: Behavior<C1, T>,)+
            Self: CancelSafe
        {
            fn run(&mut self, blackboard: &mut C1) -> Status<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run(blackboard) {
                                Status::Running(t) => {
                                    self.index += 1;
                                    return Status::Running(t);
                                }
                                Status::Success => {
                                    self.index += 1;
                                    self.succeeded += 1;
                                    if self.succeeded == $len {
                                        self.index = 0;
                                        self.succeeded = 0;
                                        return Status::Success;
                                    }
                                }
                                Status::Failure => {
                                    self.reset();
                                    return Status::Failure;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }
        impl<$($name,)+> CancelSafe for ParallelSequence<($($name,)+)>
        where
            $($name: CancelSafe,)+
        {
            fn reset(&mut self) {
                self.index = 0;
                self.succeeded = 0;
                $(
                    self.body.$num.reset();
                )+
            }
        }
        impl<C1, T, $($name,)+> InfallibleBehavior<C1, T> for ParallelSequence<($($name,)+)>
        where
            $($name: InfallibleBehavior<C1, T>,)+
        {
            fn run_infallible(&mut self, blackboard: &mut C1) -> InfallibleStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_infallible(blackboard) {
                                InfallibleStatus::Running(t) => {
                                    self.index += 1;
                                    return InfallibleStatus::Running(t);
                                }
                                InfallibleStatus::Success => {
                                    self.index += 1;
                                    self.succeeded += 1;
                                    if self.succeeded == $len {
                                        self.index = 0;
                                        self.succeeded = 0;
                                        return InfallibleStatus::Success;
                                    }
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }
        impl<C1, T, $($name,)+> FallibleBehavior<C1, T> for ParallelSequence<($($name,)+)>
        where
            $($name: FallibleBehavior<C1, T>,)+
            Self: CancelSafe
        {
            fn run_fallible(&mut self, blackboard: &mut C1) -> FallibleStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_fallible(blackboard) {
                                FallibleStatus::Running(t) => {
                                    self.index += 1;
                                    return FallibleStatus::Running(t);
                                }
                                FallibleStatus::Failure => {
                                    self.reset();
                                    return FallibleStatus::Failure;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }
        impl<C1, T, $($name,)+> EternalBehavior<C1, T> for ParallelSequence<($($name,)+)>
        where
            $($name: EternalBehavior<C1, T>,)+
        {
            fn run_eternal(&mut self, blackboard: &mut C1) -> EternalStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_eternal(blackboard) {
                                t => {
                                    self.index += 1;
                                    return t;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }

        impl<$($name,)+> IntoRon for ParallelSequence<($($name,)+)>
        where
            $($name: IntoRon,)+
        {
            fn into_ron(&self) -> ron::Value {
                ron::Value::Map(
                    [
                        (ron::Value::String("sequence".to_string()), ron::Value::Seq(
                            vec![
                                $(
                                    self.body.$num.into_ron(),
                                )+
                            ].into_iter().collect()
                        ))
                    ].into_iter().collect()
                )
            }
        }
    }
}

impl_seq!(1 A 0);
impl_seq!(2 A 0 B 1);
impl_seq!(3 A 0 B 1 C 2);

impl<A> ParallelSequence<A> {
    pub fn new(body: A) -> Self {
        Self {
            body,
            index: 0,
            succeeded: 0,
        }
    }
}

pub struct ParallelSelect<A> {
    pub body: A,
    index: usize,
    failed: usize,
}

macro_rules! impl_sel {
    ($len: literal $($name: ident $num: tt)+) => {
        impl<C1, T, $($name,)+> Behavior<C1, T> for ParallelSelect<($($name,)+)>
        where
            $($name: Behavior<C1, T>,)+
            Self: CancelSafe
        {
            fn run(&mut self, blackboard: &mut C1) -> Status<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run(blackboard) {
                                Status::Running(t) => {
                                    self.index += 1;
                                    return Status::Running(t);
                                },
                                Status::Success => {
                                    self.reset();
                                    return Status::Success;
                                }
                                Status::Failure => {
                                    self.index += 1;
                                    self.failed += 1;
                                    if self.failed == $len {
                                        self.index = 0;
                                        self.failed = 0;
                                        return Status::Failure;
                                    }
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }
        impl<$($name,)+> CancelSafe for ParallelSelect<($($name,)+)>
        where
            $($name: CancelSafe,)+
        {
            fn reset(&mut self) {
                self.index = 0;
                self.failed = 0;
                $(
                    self.body.$num.reset();
                )+
            }
        }
        impl<C1, T, $($name,)+> InfallibleBehavior<C1, T> for ParallelSelect<($($name,)+)>
        where
            $($name: InfallibleBehavior<C1, T>,)+
            Self: CancelSafe
        {
            fn run_infallible(&mut self, blackboard: &mut C1) -> InfallibleStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_infallible(blackboard) {
                                InfallibleStatus::Running(t) => {
                                    self.index += 1;
                                    return InfallibleStatus::Running(t);
                                },
                                InfallibleStatus::Success => {
                                    self.reset();
                                    return InfallibleStatus::Success;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }
        impl<C1, T, $($name,)+> FallibleBehavior<C1, T> for ParallelSelect<($($name,)+)>
        where
            $($name: FallibleBehavior<C1, T>,)+
        {
            fn run_fallible(&mut self, blackboard: &mut C1) -> FallibleStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_fallible(blackboard) {
                                FallibleStatus::Running(t) => {
                                    self.index += 1;
                                    return FallibleStatus::Running(t);
                                },
                                FallibleStatus::Failure => {
                                    self.index += 1;
                                    self.failed += 1;
                                    if self.failed == $len {
                                        self.index = 0;
                                        self.failed = 0;
                                        return FallibleStatus::Failure;
                                    }
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }
        impl<C1, T, $($name,)+> EternalBehavior<C1, T> for ParallelSelect<($($name,)+)>
        where
            $($name: EternalBehavior<C1, T>,)+
        {
            fn run_eternal(&mut self, blackboard: &mut C1) -> EternalStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_eternal(blackboard) {
                                t => {
                                    self.index += 1;
                                    return t;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }

        impl<$($name,)+> IntoRon for ParallelSelect<($($name,)+)>
        where
            $($name: IntoRon,)+
        {
            fn into_ron(&self) -> ron::Value {
                ron::Value::Map(
                    [
                        (ron::Value::String("select".to_string()), ron::Value::Seq(
                            vec![
                                $(
                                    self.body.$num.into_ron(),
                                )+
                            ].into_iter().collect()
                        ))
                    ].into_iter().collect()
                )
            }
        }
    }
}

impl_sel!(1 A 0);
impl_sel!(2 A 0 B 1);
impl_sel!(3 A 0 B 1 C 2);

impl<A> ParallelSelect<A> {
    pub fn new(body: A) -> Self {
        Self {
            body,
            index: 0,
            failed: 0,
        }
    }
}

pub struct ParallelAny<A> {
    pub body: A,
    index: usize,
}

macro_rules! impl_sel {
    ($len: literal $($name: ident $num: tt)+) => {
        impl<C1, T, $($name,)+> Behavior<C1, T> for ParallelAny<($($name,)+)>
        where
            $($name: Behavior<C1, T>,)+
            Self: CancelSafe
        {
            fn run(&mut self, blackboard: &mut C1) -> Status<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run(blackboard) {
                                Status::Running(t) => {
                                    self.index += 1;
                                    return Status::Running(t);
                                },
                                Status::Success => {
                                    self.reset();
                                    return Status::Success;
                                }
                                Status::Failure => {
                                    self.reset();
                                    return Status::Failure;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }
        impl<$($name,)+> CancelSafe for ParallelAny<($($name,)+)>
        where
            $($name: CancelSafe,)+
        {
            fn reset(&mut self) {
                self.index = 0;
                $(
                    self.body.$num.reset();
                )+
            }
        }
        impl<C1, T, $($name,)+> InfallibleBehavior<C1, T> for ParallelAny<($($name,)+)>
        where
            $($name: InfallibleBehavior<C1, T>,)+
            Self: CancelSafe
        {
            fn run_infallible(&mut self, blackboard: &mut C1) -> InfallibleStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_infallible(blackboard) {
                                InfallibleStatus::Running(t) => {
                                    self.index += 1;
                                    return InfallibleStatus::Running(t);
                                },
                                InfallibleStatus::Success => {
                                    self.reset();
                                    return InfallibleStatus::Success;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }
        impl<C1, T, $($name,)+> FallibleBehavior<C1, T> for ParallelAny<($($name,)+)>
        where
            $($name: FallibleBehavior<C1, T>,)+
            Self: CancelSafe
        {
            fn run_fallible(&mut self, blackboard: &mut C1) -> FallibleStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_fallible(blackboard) {
                                FallibleStatus::Running(t) => {
                                    self.index += 1;
                                    return FallibleStatus::Running(t);
                                },
                                FallibleStatus::Failure => {
                                    self.reset();
                                    return FallibleStatus::Failure;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }
        impl<C1, T, $($name,)+> EternalBehavior<C1, T> for ParallelAny<($($name,)+)>
        where
            $($name: EternalBehavior<C1, T>,)+
        {
            fn run_eternal(&mut self, blackboard: &mut C1) -> EternalStatus<T> {
                loop {
                    match self.index {
                        $(
                            $num => match self.body.$num.run_eternal(blackboard) {
                                t => {
                                    self.index += 1;
                                    return t;
                                }
                            }
                        )+
                        _ => {
                            self.index = 0;
                        }
                    }
                }
            }
        }

        impl<$($name,)+> IntoRon for ParallelAny<($($name,)+)>
        where
            $($name: IntoRon,)+
        {
            fn into_ron(&self) -> ron::Value {
                ron::Value::Map(
                    [
                        (ron::Value::String("select".to_string()), ron::Value::Seq(
                            vec![
                                $(
                                    self.body.$num.into_ron(),
                                )+
                            ].into_iter().collect()
                        ))
                    ].into_iter().collect()
                )
            }
        }
    }
}

impl_sel!(1 A 0);
impl_sel!(2 A 0 B 1);
impl_sel!(3 A 0 B 1 C 2);

impl<A> ParallelAny<A> {
    pub fn new(body: A) -> Self {
        Self { body, index: 0 }
    }
}
