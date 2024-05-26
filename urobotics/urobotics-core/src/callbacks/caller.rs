use std::{cell::Cell, sync::Arc};

use crossbeam::queue::SegQueue;


// pub type BoxedCallback<T: ?Sized> = Box<dyn Callback<T>>;

pub struct Callbacks<C: ?Sized> {
    storage: Vec<Box<C>>,
}

impl<T: ?Sized> Callbacks<T> {
    pub fn append(&mut self, other: &mut Self) {
        self.storage.append(&mut other.storage);
    }
}

impl<T: ?Sized> Default for Callbacks<T> {
    fn default() -> Self {
        Self {
            storage: Vec::default(),
        }
    }
}

// impl<T: ?Sized> Callbacks<T, MutCallbackSharedStorage<T>> {
//     pub fn add_callback(&self, callback: Box<T>) {
//         self.storage.callbacks.push(callback);
//     }

//     pub fn get_ref(&self) -> CallbacksRef<T, MutCallbackSharedStorage<T>> {
//         CallbacksRef {
//             storage: self.storage.clone(),
//             phantom: PhantomData,
//         }
//     }
// }

thread_local! {
    static DROP_CALLBACK: Cell<bool> = Cell::new(false);
}

pub fn drop_this_callback() {
    DROP_CALLBACK.with(|cell| cell.set(true));
}

pub fn retain_this_callback() {
    DROP_CALLBACK.with(|cell| cell.set(false));
}

impl<T> Callbacks<dyn FnMut(T)> {
    pub fn add_callback(&mut self, callback: impl FnMut(T) + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> Callbacks<dyn FnMut(T) + Send> {
    pub fn add_callback(&mut self, callback: impl FnMut(T) + Send + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> Callbacks<dyn FnMut(T) + Sync> {
    pub fn add_callback(&mut self, callback: impl FnMut(T) + Sync + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> Callbacks<dyn FnMut(T) + Send + Sync> {
    pub fn add_callback(&mut self, callback: impl FnMut(T) + Send + Sync + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T: Clone> Callbacks<dyn FnMut(T)> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.len()).rev() {
            DROP_CALLBACK.set(false);
            (self.storage.get_mut(i).unwrap())(value.clone());
            if DROP_CALLBACK.get() {
                self.storage.swap_remove(i);
            }
        }
    }
}

impl<T: Clone> Callbacks<dyn FnMut(T) + Send> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.len()).rev() {
            DROP_CALLBACK.set(false);
            (self.storage.get_mut(i).unwrap())(value.clone());
            if DROP_CALLBACK.get() {
                self.storage.swap_remove(i);
            }
        }
    }
}

impl<T: Clone> Callbacks<dyn FnMut(T) + Sync> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.len()).rev() {
            DROP_CALLBACK.set(false);
            (self.storage.get_mut(i).unwrap())(value.clone());
            if DROP_CALLBACK.get() {
                self.storage.swap_remove(i);
            }
        }
    }
}
impl<T: Clone> Callbacks<dyn FnMut(T) + Send + Sync> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.len()).rev() {
            DROP_CALLBACK.set(false);
            (self.storage.get_mut(i).unwrap())(value.clone());
            if DROP_CALLBACK.get() {
                self.storage.swap_remove(i);
            }
        }
    }
}

impl<T> Callbacks<dyn Fn(T)> {
    pub fn add_callback(&mut self, callback: impl Fn(T) + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> Callbacks<dyn Fn(T) + Send> {
    pub fn add_callback(&mut self, callback: impl Fn(T) + Send + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> Callbacks<dyn Fn(T) + Sync> {
    pub fn add_callback(&mut self, callback: impl Fn(T) + Sync + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> Callbacks<dyn Fn(T) + Send + Sync> {
    pub fn add_callback(&mut self, callback: impl Fn(T) + Send + Sync + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T: Clone> Callbacks<dyn Fn(T)> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.len()).rev() {
            DROP_CALLBACK.set(false);
            (self.storage.get_mut(i).unwrap())(value.clone());
            if DROP_CALLBACK.get() {
                self.storage.swap_remove(i);
            }
        }
    }

    pub fn call_immut(&self, value: T) {
        for i in (0..self.storage.len()).rev() {
            (self.storage.get(i).unwrap())(value.clone());
        }
    }
}

impl<T: Clone> Callbacks<dyn Fn(T) + Send> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.len()).rev() {
            DROP_CALLBACK.set(false);
            (self.storage.get_mut(i).unwrap())(value.clone());
            if DROP_CALLBACK.get() {
                self.storage.swap_remove(i);
            }
        }
    }

    pub fn call_immut(&self, value: T) {
        for i in (0..self.storage.len()).rev() {
            (self.storage.get(i).unwrap())(value.clone());
        }
    }
}

impl<T: Clone> Callbacks<dyn Fn(T) + Sync> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.len()).rev() {
            DROP_CALLBACK.set(false);
            (self.storage.get_mut(i).unwrap())(value.clone());
            if DROP_CALLBACK.get() {
                self.storage.swap_remove(i);
            }
        }
    }

    pub fn call_immut(&self, value: T) {
        for i in (0..self.storage.len()).rev() {
            (self.storage.get(i).unwrap())(value.clone());
        }
    }
}

impl<T: Clone> Callbacks<dyn Fn(T) + Send + Sync> {
    pub fn call(&mut self, value: T) {
        for i in (0..self.storage.len()).rev() {
            DROP_CALLBACK.set(false);
            (self.storage.get_mut(i).unwrap())(value.clone());
            if DROP_CALLBACK.get() {
                self.storage.swap_remove(i);
            }
        }
    }

    pub fn call_immut(&self, value: T) {
        for i in (0..self.storage.len()).rev() {
            (self.storage.get(i).unwrap())(value.clone());
        }
    }
}

pub struct SharedCallbacks<C: ?Sized> {
    storage: Arc<SegQueue<Box<C>>>,
}


impl<T: ?Sized> Default for SharedCallbacks<T> {
    fn default() -> Self {
        Self {
            storage: Arc::default(),
        }
    }
}

impl<T: ?Sized> SharedCallbacks<T> {
    pub fn get_ref(&self) -> CallbacksRef<T> {
        CallbacksRef {
            storage: self.storage.clone(),
        }
    }
}

impl<T> SharedCallbacks<dyn FnMut(T)> {
    pub fn add_callback(&self, callback: impl FnMut(T) + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> SharedCallbacks<dyn FnMut(T) + Send> {
    pub fn add_callback(&self, callback: impl FnMut(T) + Send + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> SharedCallbacks<dyn FnMut(T) + Sync> {
    pub fn add_callback(&self, callback: impl FnMut(T) + Sync + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> SharedCallbacks<dyn FnMut(T) + Send + Sync> {
    pub fn add_callback(&self, callback: impl FnMut(T) + Send + Sync + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T: Clone> SharedCallbacks<dyn FnMut(T)> {
    pub fn call(&self, value: T) {
        for _ in 0..self.storage.len() {
            DROP_CALLBACK.set(false);
            let mut callback = self.storage.pop().unwrap();
            callback(value.clone());
            if !DROP_CALLBACK.get() {
                self.storage.push(callback);
            }
        }
    }
}

impl<T: Clone> SharedCallbacks<dyn FnMut(T) + Send> {
    pub fn call(&self, value: T) {
        for _ in 0..self.storage.len() {
            DROP_CALLBACK.set(false);
            let mut callback = self.storage.pop().unwrap();
            callback(value.clone());
            if !DROP_CALLBACK.get() {
                self.storage.push(callback);
            }
        }
    }
}

impl<T: Clone> SharedCallbacks<dyn FnMut(T) + Sync> {
    pub fn call(&self, value: T) {
        for _ in 0..self.storage.len() {
            DROP_CALLBACK.set(false);
            let mut callback = self.storage.pop().unwrap();
            callback(value.clone());
            if !DROP_CALLBACK.get() {
                self.storage.push(callback);
            }
        }
    }
}
impl<T: Clone> SharedCallbacks<dyn FnMut(T) + Send + Sync> {
    pub fn call(&self, value: T) {
        for _ in 0..self.storage.len() {
            DROP_CALLBACK.set(false);
            let mut callback = self.storage.pop().unwrap();
            callback(value.clone());
            if !DROP_CALLBACK.get() {
                self.storage.push(callback);
            }
        }
    }
}

impl<T> SharedCallbacks<dyn Fn(T)> {
    pub fn add_callback(&self, callback: impl Fn(T) + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> SharedCallbacks<dyn Fn(T) + Send> {
    pub fn add_callback(&self, callback: impl Fn(T) + Send + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> SharedCallbacks<dyn Fn(T) + Sync> {
    pub fn add_callback(&self, callback: impl Fn(T) + Sync + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T> SharedCallbacks<dyn Fn(T) + Send + Sync> {
    pub fn add_callback(&self, callback: impl Fn(T) + Send + Sync + 'static) {
        self.storage.push(Box::new(callback));
    }
}

impl<T: Clone> SharedCallbacks<dyn Fn(T)> {
    pub fn call(&self, value: T) {
        for _ in 0..self.storage.len() {
            DROP_CALLBACK.set(false);
            let callback = self.storage.pop().unwrap();
            callback(value.clone());
            if !DROP_CALLBACK.get() {
                self.storage.push(callback);
            }
        }
    }
}

impl<T: Clone> SharedCallbacks<dyn Fn(T) + Send> {
    pub fn call1(&self, value: T) {
        for _ in 0..self.storage.len() {
            DROP_CALLBACK.set(false);
            let callback = self.storage.pop().unwrap();
            callback(value.clone());
            if !DROP_CALLBACK.get() {
                self.storage.push(callback);
            }
        }
    }
}

impl<T: Clone> SharedCallbacks<dyn Fn(T) + Sync> {
    pub fn call(&self, value: T) {
        for _ in 0..self.storage.len() {
            DROP_CALLBACK.set(false);
            let callback = self.storage.pop().unwrap();
            callback(value.clone());
            if !DROP_CALLBACK.get() {
                self.storage.push(callback);
            }
        }
    }
}

impl<T: Clone> SharedCallbacks<dyn Fn(T) + Send + Sync> {
    pub fn call(&self, value: T) {
        for _ in 0..self.storage.len() {
            DROP_CALLBACK.set(false);
            let callback = self.storage.pop().unwrap();
            callback(value.clone());
            if !DROP_CALLBACK.get() {
                self.storage.push(callback);
            }
        }
    }
}


pub struct CallbacksRef<T: ?Sized> {
    storage: Arc<SegQueue<Box<T>>>,
}

impl<T: ?Sized> Clone for CallbacksRef<T> {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
        }
    }
}


impl<T> CallbacksRef<dyn FnMut(T)> {
    pub fn add_callback(&self, callback: impl FnMut(T) + 'static) {
        self.storage.push(Box::new(callback));
    }
}


impl<T> CallbacksRef<dyn FnMut(T) + Send> {
    pub fn add_callback(&self, callback: impl FnMut(T) + Send + 'static) {
        self.storage.push(Box::new(callback));
    }
}


impl<T> CallbacksRef<dyn FnMut(T) + Sync> {
    pub fn add_callback(&self, callback: impl FnMut(T) + Sync + 'static) {
        self.storage.push(Box::new(callback));
    }
}


impl<T> CallbacksRef<dyn FnMut(T) + Send + Sync> {
    pub fn add_callback(&self, callback: impl FnMut(T) + Send + Sync + 'static) {
        self.storage.push(Box::new(callback));
    }
}
