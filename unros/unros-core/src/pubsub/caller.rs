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
    callbacks: Arc<SegQueue<C>>,
}

impl<C> Clone for MutCallbackSharedStorage<C> {
    fn clone(&self) -> Self {
        Self {
            callbacks: self.callbacks.clone(),
        }
    }
}

/// A storage for immutable callbacks.
pub struct ImmutCallbackStorage<C> {
    callbacks: Arc<RwLock<Vec<C>>>,
}

impl<C> Clone for ImmutCallbackStorage<C> {
    fn clone(&self) -> Self {
        Self {
            callbacks: self.callbacks.clone(),
        }
    }
}

/// A storage for a single mutable callback.
pub enum SingleCallbackStorage<C> {
    Callback(C),
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

impl<C> Default for ImmutCallbackStorage<C> {
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

pub type DefaultCallback<T> = Box<dyn FnMut(T) -> ControlFlow<()> + Send + Sync>;

pub struct Callbacks<T, S = MutCallbackStorage<DefaultCallback<T>>, C = DefaultCallback<T>> {
    storage: S,
    phantom: PhantomData<(T, C)>,
}

pub type SharedCallbacks<T> = Callbacks<T, MutCallbackSharedStorage<DefaultCallback<T>>, DefaultCallback<T>>;
pub type ImmutCallbacks<T> = Callbacks<T, ImmutCallbackStorage<Box<dyn Fn(T) -> ControlFlow<()> + Send + Sync>>, Box<dyn Fn(T) -> ControlFlow<()> + Send + Sync>>;
pub type SingleCallback<T> = Callbacks<T, SingleCallbackStorage<DefaultCallback<T>>, DefaultCallback<T>>;

pub struct CallbackHandle<C> {
    callback: Arc<SyncUnsafeCell<C>>,
}

impl<T, C> Callbacks<T, MutCallbackStorage<C>, C> {
    pub fn add_callback(&mut self, callback: C) {
        self.storage.callbacks.push(callback.into());
    }
}

impl<T, S: Default, C> Default for Callbacks<T, S, C> {
    fn default() -> Self {
        Self {
            storage: S::default(),
            phantom: PhantomData,
        }
    }
}

impl<T: 'static> Callbacks<T, MutCallbackStorage<DefaultCallback<T>>, DefaultCallback<T>> {
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

impl<T, C> Callbacks<T, MutCallbackSharedStorage<C>, C> {
    pub fn add_callback(&self, callback: C) {
        self.storage.callbacks.push(callback.into());
    }

    pub fn get_ref(&self) -> CallbacksRef<T, MutCallbackSharedStorage<C>, C> {
        CallbacksRef {
            storage: self.storage.clone(),
            phantom: PhantomData,
        }
    }
}

impl<T: 'static> Callbacks<T, MutCallbackSharedStorage<DefaultCallback<T>>, DefaultCallback<T>> {
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

impl<T, C> Callbacks<T, ImmutCallbackStorage<C>, C> {
    pub fn add_callback(&self, callback: C) {
        self.storage.callbacks.clear_poison();
        self.storage
            .callbacks
            .write()
            .unwrap()
            .push(callback.into());
    }

    pub fn get_ref(&self) -> CallbacksRef<T, ImmutCallbackStorage<C>, C> {
        CallbacksRef {
            storage: self.storage.clone(),
            phantom: PhantomData,
        }
    }
}

impl<T: 'static> Callbacks<T, ImmutCallbackStorage<DefaultCallback<T>>, DefaultCallback<T>> {
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

impl<T, C> Callbacks<T, SingleCallbackStorage<C>, C> {
    pub fn add_callback(&mut self, callback: C) {
        self.storage = SingleCallbackStorage::Callback(callback.into());
    }
}

impl<T: 'static> Callbacks<T, SingleCallbackStorage<DefaultCallback<T>>, DefaultCallback<T>> {
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

impl<T: Clone, C: FnMut(T) -> ControlFlow<()>> Callbacks<T, MutCallbackStorage<C>, C> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.callbacks.len()).rev() {
            if ControlFlow::Break(()) == (self.storage.callbacks.get_mut(i).unwrap())(value.clone())
            {
                self.storage.callbacks.swap_remove(i);
            }
        }
    }
}

impl<T: Clone, C: FnMut(T) -> ControlFlow<()>> Callbacks<T, MutCallbackSharedStorage<C>, C> {
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

impl<T: Clone, C: Fn(T) -> ControlFlow<()>> Callbacks<T, ImmutCallbackStorage<C>, C> {
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

impl<T, C: FnMut(T) -> ControlFlow<()>> Callbacks<T, SingleCallbackStorage<C>, C> {
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

impl<T, C: Fn(T) -> ControlFlow<()>> Callbacks<T, SingleCallbackStorage<C>, C> {
    pub fn call_immut(&self, value: T) {
        match &self.storage {
            SingleCallbackStorage::Callback(callback) => {
                callback(value);
            }
            SingleCallbackStorage::None => {}
        }
    }
}

pub struct CallbacksRef<T, S, C = DefaultCallback<T>> {
    storage: S,
    phantom: PhantomData<(T, C)>,
}

impl<T, C> CallbacksRef<T, MutCallbackSharedStorage<C>, C> {
    pub fn add_callback(&self, callback: C) {
        self.storage.callbacks.push(callback.into());
    }
}

impl<T: 'static> CallbacksRef<T, MutCallbackSharedStorage<DefaultCallback<T>>, DefaultCallback<T>> {
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

impl<T, C> CallbacksRef<T, ImmutCallbackStorage<C>, C> {
    pub fn add_callback(&self, callback: C) {
        self.storage.callbacks.clear_poison();
        self.storage
            .callbacks
            .write()
            .unwrap()
            .push(callback.into());
    }
}

impl<T: 'static> CallbacksRef<T, ImmutCallbackStorage<DefaultCallback<T>>, DefaultCallback<T>> {
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

impl<T, S: Clone, C> Clone for CallbacksRef<T, S, C> {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            phantom: PhantomData,
        }
    }
}

pub type SharedCallbacksRef<T> = CallbacksRef<T, MutCallbackSharedStorage<DefaultCallback<T>>, DefaultCallback<T>>;
pub type ImmutCallbacksRef<T> = CallbacksRef<T, ImmutCallbackStorage<Box<dyn Fn(T) -> ControlFlow<()> + Send + Sync>>, Box<dyn Fn(T) -> ControlFlow<()> + Send + Sync>>;