use std::{
    ops::{Add, Deref, DerefMut, Mul, Sub},
    sync::Arc,
};

use crossbeam::queue::SegQueue;
use k::UnitQuaternion;
use nalgebra::{Quaternion, SimdRealField, UnitVector3, Vector3};

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

/// Decomposes the `src` quaternion into two quaternions: the `twist` quaternion is the rotation around the `axis` vector, and the `swing` quaternion is the remaining rotation.
///
/// The returned order is `(swing, twist)`. The original quaternion can be reconstructed by `swing * twist`.
///
/// # Source
/// 1. https://stackoverflow.com/questions/3684269/component-of-a-quaternion-rotation-around-an-axis
/// 2. https://www.euclideanspace.com/maths/geometry/rotations/for/decomposition/
#[inline]
pub fn swing_twist_decomposition<F>(
    src: &UnitQuaternion<F>,
    axis: &UnitVector3<F>,
) -> (UnitQuaternion<F>, UnitQuaternion<F>)
where
    F: SimdRealField + Copy,
    F::Element: SimdRealField,
{
    let rotation_axis = Vector3::new(src.i, src.j, src.k);
    let dot = rotation_axis.dot(axis.as_ref());
    let projection = axis.into_inner() * dot;
    let twist = UnitQuaternion::new_normalize(Quaternion::new(
        src.w,
        projection.x,
        projection.y,
        projection.z,
    ));
    let swing = src * twist.conjugate();
    (swing, twist)
}
