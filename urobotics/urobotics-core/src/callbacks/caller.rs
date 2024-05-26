use std::{marker::PhantomData, sync::Arc};

use crossbeam::queue::SegQueue;

/// A storage for callbacks that requires mutable access to add callbacks.
pub struct MutCallbackStorage<T> {
    callbacks: Vec<BoxedCallback<T>>,
}

/// A storage for callbacks that only requires immutable access to add callbacks.
pub struct MutCallbackSharedStorage<T> {
    callbacks: Arc<SegQueue<BoxedCallback<T>>>,
}

impl<C> Clone for MutCallbackSharedStorage<C> {
    fn clone(&self) -> Self {
        Self {
            callbacks: self.callbacks.clone(),
        }
    }
}


/// A storage for a single mutable callback.
pub enum SingleCallbackStorage<T> {
    Callback(Box<dyn Callback<T>>),
    None,
}

impl<C> Default for MutCallbackStorage<C> {
    fn default() -> Self {
        Self {
            callbacks: Vec::new(),
        }
    }
}

impl<C> Default for MutCallbackSharedStorage<C> {
    fn default() -> Self {
        Self {
            callbacks: Arc::default(),
        }
    }
}

impl<C> Default for SingleCallbackStorage<C> {
    fn default() -> Self {
        Self::None
    }
}

pub type BoxedCallback<T> = Box<dyn Callback<T>>;

pub struct Callbacks<T, S = MutCallbackStorage<BoxedCallback<T>>> {
    storage: S,
    phantom: PhantomData<T>,
}

pub type SharedCallbacks<T> =
    Callbacks<T, MutCallbackSharedStorage<BoxedCallback<T>>>;
pub type SingleCallback<T> =
    Callbacks<T, SingleCallbackStorage<BoxedCallback<T>>>;


impl<T> Callbacks<T, MutCallbackStorage<T>> {
    pub fn add_callback(&mut self, callback: impl Callback<T>) {
        self.storage.callbacks.push(Box::new(callback));
    }
}

impl<T, S: Default> Default for Callbacks<T, S> {
    fn default() -> Self {
        Self {
            storage: S::default(),
            phantom: PhantomData,
        }
    }
}

impl<T> Callbacks<T, MutCallbackSharedStorage<T>> {
    pub fn add_callback(&self, callback: impl Callback<T>) {
        self.storage.callbacks.push(Box::new(callback));
    }

    pub fn get_ref(&self) -> CallbacksRef<T, MutCallbackSharedStorage<T>> {
        CallbacksRef {
            storage: self.storage.clone(),
            phantom: PhantomData,
        }
    }
}

impl<T> Callbacks<T, SingleCallbackStorage<T>> {
    pub fn add_callback(&mut self, callback: impl Callback<T>) {
        self.storage = SingleCallbackStorage::Callback(Box::new(callback));
    }
}


impl<T: Clone> Callbacks<T, MutCallbackStorage<T>> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.callbacks.len()).rev() {
            if CallbackReturn::Drop == (self.storage.callbacks.get_mut(i).unwrap())(value.clone())
            {
                self.storage.callbacks.swap_remove(i);
            }
        }
    }
}

impl<T: Clone> Callbacks<T, MutCallbackSharedStorage<T>> {
    pub fn call(&mut self, value: T) {
        for _ in 0..self.storage.callbacks.len() {
            let mut callback = self.storage.callbacks.pop().unwrap();
            if CallbackReturn::Persist == callback(value.clone()) {
                self.storage.callbacks.push(callback);
            }
        }
    }
}


impl<T> Callbacks<T, SingleCallbackStorage<T>> {
    pub fn call(&mut self, value: T) {
        match &mut self.storage {
            SingleCallbackStorage::Callback(callback) => {
                if CallbackReturn::Drop == callback(value) {
                    self.storage = SingleCallbackStorage::None;
                }
            }
            SingleCallbackStorage::None => {}
        }
    }
}


pub struct CallbacksRef<T, S> {
    storage: S,
    phantom: PhantomData<T>,
}

impl<T> CallbacksRef<T, MutCallbackSharedStorage<T>> {
    pub fn add_callback(&self, callback: impl for<'a> Callback<T>) {
        self.storage.callbacks.push(Box::new(callback));
    }
}


impl<T, S: Clone> Clone for CallbacksRef<T, S> {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            phantom: PhantomData,
        }
    }
}

pub type SharedCallbacksRef<T> =
    CallbacksRef<T, MutCallbackSharedStorage<BoxedCallback<T>>>;


pub trait Callback<T>: FnMut(T) -> CallbackReturn + Send + Sync + 'static {
}

impl<T, F: FnMut(T) -> CallbackReturn + Send + Sync + 'static> Callback<T> for F {
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallbackReturn {
    Persist,
    Drop
}
