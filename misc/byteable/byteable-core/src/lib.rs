use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use crossbeam::queue::SegQueue;

pub struct Recycler<T> {
    queue: Arc<SegQueue<T>>,
}

impl<T> Clone for Recycler<T> {
    fn clone(&self) -> Self {
        Self {
            queue: self.queue.clone(),
        }
    }
}

pub struct RecycleGuard<T> {
    value: Option<T>,
    queue: Option<Arc<SegQueue<T>>>,
}

impl<T> Drop for RecycleGuard<T> {
    fn drop(&mut self) {
        if let Some(queue) = self.queue.as_ref() {
            queue.push(self.value.take().unwrap());
        }
    }
}

impl<T> Deref for RecycleGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref().unwrap()
    }
}

impl<T> DerefMut for RecycleGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().unwrap()
    }
}

impl<T> Default for Recycler<T> {
    fn default() -> Self {
        Self {
            queue: Arc::new(SegQueue::new()),
        }
    }
}

impl<T> Recycler<T> {
    pub fn get(&self) -> Option<RecycleGuard<T>> {
        self.queue.pop().map(|value| RecycleGuard {
            value: Some(value),
            queue: Some(self.queue.clone()),
        })
    }

    pub fn get_or(&self, or: T) -> RecycleGuard<T> {
        let value = self.queue.pop().unwrap_or(or);
        RecycleGuard {
            value: Some(value),
            queue: Some(self.queue.clone()),
        }
    }

    pub fn get_or_else(&self, f: impl FnOnce() -> T) -> RecycleGuard<T> {
        let value = self.queue.pop().unwrap_or_else(f);
        RecycleGuard {
            value: Some(value),
            queue: Some(self.queue.clone()),
        }
    }

    pub fn wrap(&self, value: T) -> RecycleGuard<T> {
        RecycleGuard {
            value: Some(value),
            queue: Some(self.queue.clone()),
        }
    }
}

impl<T> RecycleGuard<T> {
    pub fn noop(value: T) -> Self {
        Self {
            value: Some(value),
            queue: None,
        }
    }

    pub fn unwrap(mut self) -> T {
        self.queue = None;
        self.value.take().unwrap()
    }
}

impl<T: Default> Recycler<T> {
    pub fn get_or_default(&self) -> RecycleGuard<T> {
        self.get_or_else(T::default)
    }
}

#[derive(Debug)]
pub struct EmptyVec<'a, T>(&'a mut Vec<T>);

#[derive(Clone, Copy, Debug)]
pub struct NotEmptyError;

impl std::fmt::Display for NotEmptyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "vector is not empty")
    }
}

impl std::error::Error for NotEmptyError {}

impl<'a, T> TryFrom<&'a mut Vec<T>> for EmptyVec<'a, T> {
    type Error = NotEmptyError;

    fn try_from(value: &'a mut Vec<T>) -> Result<Self, Self::Error> {
        if value.is_empty() {
            Ok(EmptyVec(value))
        } else {
            Err(NotEmptyError)
        }
    }
}

impl<'a, T> From<EmptyVec<'a, T>> for &'a mut Vec<T> {
    fn from(value: EmptyVec<'a, T>) -> Self {
        value.0
    }
}

pub trait FillByteVec {
    const SIZE_HINT: usize = 0;

    fn fill_bytes(&self, vec: EmptyVec<u8>);
}

pub trait IntoBytes {
    fn to_bytes_vec(&self) -> Vec<u8> {
        RecycleGuard::unwrap(self.to_bytes())
    }
    fn to_bytes_boxed(&self) -> Box<[u8]> {
        self.to_bytes_vec().into_boxed_slice()
    }
    fn to_bytes(&self) -> RecycleGuard<Vec<u8>>;
}

pub trait IntoBytesSlice {
    fn into_bytes_slice<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T;
}

macro_rules! impl_int {
    ($int: ty) => {
        impl FillByteVec for $int {
            const SIZE_HINT: usize = std::mem::size_of::<$int>();

            fn fill_bytes(&self, vec: EmptyVec<u8>) {
                let vec: &mut Vec<u8> = vec.into();
                vec.extend_from_slice(&self.to_ne_bytes());
            }
        }

        impl IntoBytesSlice for $int {
            fn into_bytes_slice<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T {
                f(&self.to_ne_bytes())
            }
        }

        impl IntoBytes for $int {
            fn to_bytes(&self) -> RecycleGuard<Vec<u8>> {
                thread_local! {
                    static RECYCLER: Recycler<Vec<u8>> = Recycler::default();
                }

                RECYCLER.with(|recycler| {
                    let mut bytes = recycler.get_or_else(|| Vec::with_capacity(<$int>::SIZE_HINT));
                    bytes.clear();
                    self.fill_bytes((&mut *bytes).try_into().unwrap());
                    bytes
                })
            }
        }
    };
}

impl_int!(u8);
impl_int!(u16);
impl_int!(u32);
impl_int!(u64);
impl_int!(u128);
impl_int!(usize);
impl_int!(i8);
impl_int!(i16);
impl_int!(i32);
impl_int!(i64);
impl_int!(i128);
impl_int!(isize);
