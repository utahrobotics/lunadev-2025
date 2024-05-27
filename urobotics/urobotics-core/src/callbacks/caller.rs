use std::{cell::Cell, marker::Tuple, sync::Arc};

use crossbeam::queue::SegQueue;

thread_local! {
    static DROP_CALLBACK: Cell<bool> = Cell::new(false);
}

pub fn drop_this_callback() {
    DROP_CALLBACK.with(|cell| cell.set(true));
}

pub fn retain_this_callback() {
    DROP_CALLBACK.with(|cell| cell.set(false));
}

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

// Callbacks FnMut

impl<Args: Clone + Tuple> FnOnce<Args> for Callbacks<dyn FnMut<Args, Output = ()>> {
    type Output = ();

    extern "rust-call" fn call_once(mut self, args: Args) -> Self::Output {
        self.call_mut(args);
    }
}


impl<Args: Clone + Tuple> FnMut<Args> for Callbacks<dyn FnMut<Args, Output = ()>> {
    extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output {
        for i in (0..self.storage.len()).rev() {
            DROP_CALLBACK.set(false);
            self.storage.get_mut(i).unwrap().call_mut(args.clone());
            if DROP_CALLBACK.get() {
                self.storage.swap_remove(i);
            }
        }
    }
}

impl<Args: Tuple> Callbacks<dyn FnMut<Args, Output = ()>> {
    pub fn add_callback(&mut self, callback: impl FnMut<Args, Output = ()> + 'static) {
        self.storage.push(Box::new(callback));
    }
}

// Callbacks Fn

impl<Args: Tuple> Callbacks<dyn Fn<Args, Output = ()>> {
    pub fn add_callback(&mut self, callback: impl Fn<Args, Output = ()> + 'static) {
        self.storage.push(Box::new(callback));
    }
}


impl<Args: Clone + Tuple> FnOnce<Args> for Callbacks<dyn Fn<Args, Output = ()>> {
    type Output = ();

    extern "rust-call" fn call_once(mut self, args: Args) -> Self::Output {
        self.call_mut(args);
    }
}


impl<Args: Clone + Tuple> FnMut<Args> for Callbacks<dyn Fn<Args, Output = ()>> {
    extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output {
        for i in (0..self.storage.len()).rev() {
            DROP_CALLBACK.set(false);
            self.storage.get_mut(i).unwrap().call_mut(args.clone());
            if DROP_CALLBACK.get() {
                self.storage.swap_remove(i);
            }
        }
    }
}


impl<Args: Clone + Tuple> Fn<Args> for Callbacks<dyn Fn<Args, Output = ()>> {
    extern "rust-call" fn call(&self, args: Args) -> Self::Output {
        for i in (0..self.storage.len()).rev() {
            (self.storage.get(i).unwrap()).call(args.clone());
        }
    }
}


pub struct SharedCallbacks<C: ?Sized> {
    pub(crate) storage: Arc<SegQueue<Box<C>>>,
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

// SharedCallbacks FnMut

impl<Args: Clone + Tuple> FnOnce<Args> for SharedCallbacks<dyn FnMut<Args, Output = ()>> {
    type Output = ();

    extern "rust-call" fn call_once(mut self, args: Args) -> Self::Output {
        self.call_mut(args);
    }
}

impl<Args: Clone + Tuple> FnMut<Args> for SharedCallbacks<dyn FnMut<Args, Output = ()>> {
    extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output {
        for _ in 0..self.storage.len() {
            DROP_CALLBACK.set(false);
            let mut callback = self.storage.pop().unwrap();
            callback.call_mut(args.clone());
            if !DROP_CALLBACK.get() {
                self.storage.push(callback);
            }
        }
    }
}

impl<Args: Tuple> SharedCallbacks<dyn FnMut<Args, Output = ()>> {
    pub fn add_callback(&self, callback: impl FnMut<Args, Output = ()> + 'static) {
        self.storage.push(Box::new(callback));
    }
}

// SharedCallbacks Fn

// impl<Args: Clone + Tuple> FnOnce<Args> for SharedCallbacks<dyn Fn<Args, Output = ()>> {
//     type Output = ();

//     extern "rust-call" fn call_once(self, args: Args) -> Self::Output {
//         self.call(args);
//     }
// }

// impl<Args: Clone + Tuple> FnMut<Args> for SharedCallbacks<dyn Fn<Args, Output = ()>> {
//     extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output {
//         self.call(args);
//     }
// }
// impl<Args: Clone + Tuple> Fn<Args> for SharedCallbacks<dyn Fn<Args, Output = ()>> {
//     extern "rust-call" fn call(&self, args: Args) -> Self::Output {
//         for _ in 0..self.storage.len() {
//             DROP_CALLBACK.set(false);
//             let callback = self.storage.pop().unwrap();
//             callback.call(args.clone());
//             if !DROP_CALLBACK.get() {
//                 self.storage.push(callback);
//             }
//         }
//     }
// }

impl<'b, T: 'b> FnOnce<(&T,)> for SharedCallbacks<dyn for<'a> Fn<(&'a T,), Output = ()>> {
    type Output = ();

    extern "rust-call" fn call_once(self, args: (&T,)) -> Self::Output {
        self.call(args);
    }
}

impl<'b, T: 'b> FnMut<(&T,)> for SharedCallbacks<dyn for<'a> Fn<(&'a T,), Output = ()>> {
    extern "rust-call" fn call_mut(&mut self, args: (&T,)) -> Self::Output {
        self.call(args);
    }
}
impl<'b, T: 'b> Fn<(&T,)> for SharedCallbacks<dyn for<'a> Fn<(&'a T,), Output = ()>> {
    extern "rust-call" fn call(&self, args: (&T,)) -> Self::Output {
        for _ in 0..self.storage.len() {
            DROP_CALLBACK.set(false);
            let callback = self.storage.pop().unwrap();
            callback.call(args.clone());
            if !DROP_CALLBACK.get() {
                self.storage.push(callback);
            }
        }
    }
}

impl<Args: Tuple> SharedCallbacks<dyn Fn<Args, Output = ()>> {
    pub fn add_callback(&self, callback: impl Fn<Args, Output = ()> + 'static) {
        self.storage.push(Box::new(callback));
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

// CallbacksRef FnMut

impl<Args: Tuple> CallbacksRef<dyn FnMut<Args, Output = ()>> {
    pub fn add_callback(&self, callback: impl FnMut<Args, Output = ()> + 'static) {
        self.storage.push(Box::new(callback));
    }
}

// CallbacksRef Fn

impl<Args: Tuple> CallbacksRef<dyn Fn<Args, Output = ()>> {
    pub fn add_callback(&self, callback: impl Fn<Args, Output = ()> + 'static) {
        self.storage.push(Box::new(callback));
    }
}