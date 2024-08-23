use std::{ops::{Add, Deref, DerefMut, Mul, Sub}, sync::Arc};

use crossbeam::queue::SegQueue;

pub fn lerp_value(delta: f64, speed: f64) -> f64 {
    0.5f64.powf(speed * delta)
}

#[allow(dead_code)]
pub fn lerp<T>(from: T, to: T, delta: f64, speed: f64) -> T
where
    T: Sub<Output = T> + Add<Output = T> + Mul<f64, Output = T> + Copy,
{
    let diff = to - from;
    from + diff * lerp_value(delta, speed)
}


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

    pub fn associate(&self, value: T) -> RecycleGuard<T> {
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
}

impl<T: Default> Recycler<T> {
    pub fn get_or_default(&self) -> RecycleGuard<T> {
        self.get_or_else(T::default)
    }
}