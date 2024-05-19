use std::{
    cell::SyncUnsafeCell,
    marker::PhantomData,
    ops::ControlFlow,
    sync::{Arc, RwLock},
};

use crossbeam::queue::SegQueue;

/// A storage for callbacks that requires mutable access to add callbacks.
pub struct MutCallbackStorage<C> {
    callbacks: Vec<C>,
}

/// A storage for callbacks that only requires immutable access to add callbacks.
pub struct MutCallbackSharedStorage<C> {
    callbacks: SegQueue<C>,
}

/// A storage for immutable callbacks.
pub struct ImmutCallbackStorage<C> {
    callbacks: RwLock<Vec<C>>,
}

/// A storage for a single mutable callback.
pub enum SingleCallbackStorage<C> {
    Callback(C),
    None,
}

pub type DefaultCallback<T> = Box<dyn FnMut(T) -> ControlFlow<()> + Send + Sync>;

pub struct Callbacks<T, C = DefaultCallback<T>, S = MutCallbackStorage<C>> {
    storage: S,
    phantom: PhantomData<(T, C)>,
}

pub struct CallbackHandle<C> {
    callback: Arc<SyncUnsafeCell<C>>,
}

impl<T, C> Callbacks<T, C, MutCallbackStorage<C>> {
    pub fn add_callback(&mut self, callback: impl Into<C>) {
        self.storage.callbacks.push(callback.into());
    }
}

impl<T: 'static> Callbacks<T, DefaultCallback<T>, MutCallbackStorage<DefaultCallback<T>>> {
    pub fn add_callback_with_handle(
        &mut self,
        callback: impl Into<DefaultCallback<T>>,
    ) -> CallbackHandle<DefaultCallback<T>> {
        let handle = CallbackHandle {
            callback: Arc::new(SyncUnsafeCell::new(callback.into())),
        };
        let weak = Arc::downgrade(&handle.callback);
        self.storage.callbacks.push(Box::new(move |item| unsafe {
            if let Some(callback) = weak.upgrade() {
                (*callback.get())(item);
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        }));
        handle
    }
}

impl<T, C> Callbacks<T, C, MutCallbackSharedStorage<C>> {
    pub fn add_callback(&self, callback: impl Into<C>) {
        self.storage.callbacks.push(callback.into());
    }
}

impl<T: 'static> Callbacks<T, DefaultCallback<T>, MutCallbackSharedStorage<DefaultCallback<T>>> {
    pub fn add_callback_with_handle(
        &mut self,
        callback: impl Into<DefaultCallback<T>>,
    ) -> CallbackHandle<DefaultCallback<T>> {
        let handle = CallbackHandle {
            callback: Arc::new(SyncUnsafeCell::new(callback.into())),
        };
        let weak = Arc::downgrade(&handle.callback);
        self.storage.callbacks.push(Box::new(move |item| unsafe {
            if let Some(callback) = weak.upgrade() {
                (*callback.get())(item);
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        }));
        handle
    }
}

impl<T, C> Callbacks<T, C, ImmutCallbackStorage<C>> {
    pub fn add_callback(&self, callback: impl Into<C>) {
        self.storage.callbacks.clear_poison();
        self.storage
            .callbacks
            .write()
            .unwrap()
            .push(callback.into());
    }
}

impl<T: 'static> Callbacks<T, DefaultCallback<T>, ImmutCallbackStorage<DefaultCallback<T>>> {
    pub fn add_callback_with_handle(
        &mut self,
        callback: impl Into<DefaultCallback<T>>,
    ) -> CallbackHandle<DefaultCallback<T>> {
        let handle = CallbackHandle {
            callback: Arc::new(SyncUnsafeCell::new(callback.into())),
        };
        let weak = Arc::downgrade(&handle.callback);
        self.storage.callbacks.clear_poison();
        self.storage
            .callbacks
            .write()
            .unwrap()
            .push(Box::new(move |item| unsafe {
                if let Some(callback) = weak.upgrade() {
                    (*callback.get())(item);
                    ControlFlow::Continue(())
                } else {
                    ControlFlow::Break(())
                }
            }));
        handle
    }
}

impl<T, C> Callbacks<T, C, SingleCallbackStorage<C>> {
    pub fn add_callback(&mut self, callback: impl Into<C>) {
        self.storage = SingleCallbackStorage::Callback(callback.into());
    }
}

impl<T: 'static> Callbacks<T, DefaultCallback<T>, SingleCallbackStorage<DefaultCallback<T>>> {
    pub fn add_callback_with_handle(
        &mut self,
        callback: impl Into<DefaultCallback<T>>,
    ) -> CallbackHandle<DefaultCallback<T>> {
        let handle = CallbackHandle {
            callback: Arc::new(SyncUnsafeCell::new(callback.into())),
        };
        let weak = Arc::downgrade(&handle.callback);
        self.storage = SingleCallbackStorage::Callback(Box::new(move |item| unsafe {
            if let Some(callback) = weak.upgrade() {
                (*callback.get())(item);
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        }));
        handle
    }
}

impl<T: Clone, C: FnMut(T) -> ControlFlow<()>> Callbacks<T, C, MutCallbackStorage<C>> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.callbacks.len()).rev() {
            if ControlFlow::Break(()) == (self.storage.callbacks.get_mut(i).unwrap())(value.clone())
            {
                self.storage.callbacks.swap_remove(i);
            }
        }
    }
}

impl<T: Clone, C: FnMut(T) -> ControlFlow<()>> Callbacks<T, C, MutCallbackSharedStorage<C>> {
    pub fn call(&mut self, value: T) {
        for _ in 0..self.storage.callbacks.len() {
            let mut callback = self.storage.callbacks.pop().unwrap();
            if ControlFlow::Continue(()) == callback(value.clone())
            {
                self.storage.callbacks.push(callback);
            }
        }
    }
}

impl<T: Clone, C: Fn(T) -> ControlFlow<()>> Callbacks<T, C, ImmutCallbackStorage<C>> {
    pub fn call(&self, value: T) {
        self.storage.callbacks.clear_poison();
        if let Ok(mut callbacks) = self.storage.callbacks.try_write() {
            for i in (0..callbacks.len()).rev() {
                if ControlFlow::Break(()) == (callbacks.get_mut(i).unwrap())(value.clone())
                {
                   callbacks.swap_remove(i);
                }
            }
        } else {
            self.storage
                .callbacks
                .read()
                .unwrap()
                .iter()
                .for_each(|callback| {
                    callback(value.clone());
                });
        }
    }
}

impl<T, C: FnMut(T) -> ControlFlow<()>> Callbacks<T, C, SingleCallbackStorage<C>> {
    pub fn call(&mut self, value: T) {
        match &mut self.storage {
            SingleCallbackStorage::Callback(callback) => {
                if ControlFlow::Break(()) == callback(value)
                {
                    self.storage = SingleCallbackStorage::None;
                }
            }
            SingleCallbackStorage::None => {}
        }
    }
}

impl<T, C: Fn(T) -> ControlFlow<()>> Callbacks<T, C, SingleCallbackStorage<C>> {
    pub fn call_immut(&self, value: T) {
        match &self.storage {
            SingleCallbackStorage::Callback(callback) => {
                callback(value);
            }
            SingleCallbackStorage::None => {}
        }
    }
}
