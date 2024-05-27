use std::{
    cell::Cell,
    sync::{Arc, Weak},
};

use parking_lot::RwLock;

thread_local! {
    static DROP_CALLBACK: Cell<bool> = Cell::new(false);
}

pub fn drop_this_callback() {
    DROP_CALLBACK.with(|cell| cell.set(true));
}

pub fn retain_this_callback() {
    DROP_CALLBACK.with(|cell| cell.set(false));
}

pub fn was_callback_dropped() -> bool {
    DROP_CALLBACK.get()
}

pub mod prelude {
    pub use super::{drop_this_callback, retain_this_callback, was_callback_dropped};
}

#[macro_export]
macro_rules! define_callbacks {
    ($vis: vis $name: ident => Fn($($param: ident : $arg: ty),*)$($extra: tt)*) => {
        #[allow(dead_code)]
        use $crate::callbacks::caller::prelude::*;

        #[derive(Default)]
        $vis struct $name {
            storage: Vec<Box<dyn Fn($($arg,)*)$($extra)*>>,
        }

        impl $name {
            /// Takes ownership of all callbacks in other.
            #[allow(dead_code)]
            $vis fn append(&mut self, other: &mut Self) {
                self.storage.append(&mut other.storage);
            }

            /// Calls all callbacks.
            ///
            /// This *does not* drop any callbacks that return `true` from `was_callback_dropped` and is thus
            /// faster than `call`.
            #[allow(dead_code)]
            $vis fn call_immut(&self, $($param : $arg),*) {
                for func in self.storage.iter() {
                    func($($param),*);
                }
            }

            /// Calls all callbacks.
            ///
            /// This drops any callbacks that return `true` from `was_callback_dropped`, unlike `call_immut`.
            #[allow(dead_code)]
            $vis fn call(&mut self, $($param : $arg),*) {
                for i in (0..self.storage.len()).rev() {
                    retain_this_callback();
                    let func = self.storage.get_mut(i).unwrap();
                    func($(<$arg>::clone(&$param)),*);
                    if was_callback_dropped() {
                        let _ = self.storage.swap_remove(i);
                    }
                }
            }

            /// Adds a callback.
            #[allow(dead_code)]
            $vis fn add_callback(&mut self, callback: impl Fn($($arg),*)$($extra)* + 'static) {
                self.storage.push(Box::new(callback));
            }

            /// Get the number of callbacks.
            #[allow(dead_code)]
            $vis fn len(&self) -> usize {
                self.storage.len()
            }

            /// Returns true iff there are no callbacks.
            #[allow(dead_code)]
            $vis fn is_empty(&self) -> bool {
                self.storage.is_empty()
            }
        }
    };
    ($vis: vis $name: ident => FnMut($($param: ident : $arg: ty),*)$($extra: tt)*) => {
        #[allow(dead_code)]
        use $crate::callbacks::caller::prelude::*;

        #[derive(Default)]
        $vis struct $name {
            storage: Vec<Box<dyn FnMut($($arg,)*)$($extra)*>>,
        }

        impl $name {
            /// Takes ownership of all callbacks in other.
            #[allow(dead_code)]
            $vis fn append(&mut self, other: &mut Self) {
                self.storage.append(&mut other.storage);
            }

            /// Calls all callbacks.
            ///
            /// This drops any callbacks that return `true` from `was_callback_dropped`.
            #[allow(dead_code)]
            $vis fn call(&mut self, $($param : $arg),*) {
                for i in (0..self.storage.len()).rev() {
                    retain_this_callback();
                    let func = self.storage.get_mut(i).unwrap();
                    func($(<$arg>::clone(&$param)),*);
                    if was_callback_dropped() {
                        let _ = self.storage.swap_remove(i);
                    }
                }
            }

            /// Adds a callback.
            #[allow(dead_code)]
            $vis fn add_callback(&mut self, callback: impl FnMut($($arg),*)$($extra)* + 'static) {
                self.storage.push(Box::new(callback));
            }

            /// Get the number of callbacks.
            #[allow(dead_code)]
            $vis fn len(&self) -> usize {
                self.storage.len()
            }

            /// Returns true iff there are no callbacks.
            #[allow(dead_code)]
            $vis fn is_empty(&self) -> bool {
                self.storage.is_empty()
            }
        }
    };
}

#[macro_export]
macro_rules! define_shared_callbacks {
    ($vis: vis $name: ident => Fn($($param: ident : $arg: ty),*)$($extra: tt)*) => {
        #[allow(dead_code)]
        use $crate::callbacks::caller::prelude::*;

        #[derive(Default)]
        pub struct $name {
            storage: Arc<$crate::parking_lot::RwLock<Vec<Box<dyn Fn($($arg,)*)$($extra)*>>>>,
            modified: std::sync::atomic::AtomicBool
        }

        impl Clone for $name {
            fn clone(&self) -> Self {
                Self {
                    storage: self.storage.clone(),
                    modified: std::sync::atomic::AtomicBool::new(false)
                }
            }
        }

        impl $name {
            /// Calls all callbacks.
            ///
            /// This attempts to drop callbacks when possible without contention. The algorithm
            /// is heavily biased towards calling instead of dropping.
            #[allow(dead_code)]
            $vis fn call(&self, $($param : $arg),*) {
                if self.modified.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Some(mut storage) = self.storage.try_write() {
                        for i in (0..storage.len()).rev() {
                            retain_this_callback();
                            let func = storage.get_mut(i).unwrap();
                            func($(<$arg>::clone(&$param)),*);
                            if was_callback_dropped() {
                                let _ = storage.swap_remove(i);
                            }
                        }
                        self.modified.store(false, std::sync::atomic::Ordering::Relaxed);
                    } else {
                        for func in self.storage.read().iter() {
                            func($(<$arg>::clone(&$param)),*);
                        }
                    }
                } else {
                    for func in self.storage.read().iter() {
                        retain_this_callback();
                        func($(<$arg>::clone(&$param)),*);
                        if was_callback_dropped() {
                            self.modified.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
            }

            /// Adds a callback.
            #[allow(dead_code)]
            $vis fn add_callback(&self, callback: impl Fn($($arg),*)$($extra)* + 'static) {
                self.storage.write().push(Box::new(callback));
            }

            /// Gets a weak reference that can be used to add callbacks.
            #[allow(dead_code)]
            $vis fn get_ref(&self) -> CallbacksRef<dyn Fn($($arg),*)$($extra)*> {
                (&self.storage).into()
            }

            /// Get the number of callbacks.
            #[allow(dead_code)]
            $vis fn len(&self) -> usize {
                self.storage.read().len()
            }

            /// Returns true iff there are no callbacks.
            #[allow(dead_code)]
            $vis fn is_empty(&self) -> bool {
                self.storage.read().is_empty()
            }
        }
    };
    ($vis: vis $name: ident => FnMut($($param: ident : $arg: ty),*)$($extra: tt)*) => {
        #[allow(dead_code)]
        use $crate::callbacks::caller::prelude::*;

        #[derive(Default, Clone)]
        $vis struct $name {
            storage: Arc<$crate::parking_lot::RwLock<Vec<Box<dyn FnMut($($arg,)*)$($extra)*>>>>,
        }

        impl $name {
            /// Calls all callbacks.
            ///
            /// This drops any callbacks that return `true` from `was_callback_dropped`.
            #[allow(dead_code)]
            $vis fn call(&self, $($param : $arg),*) {
                let mut storage = self.storage.write();
                for i in (0..storage.len()).rev() {
                    retain_this_callback();
                    let func = storage.get_mut(i).unwrap();
                    func($(<$arg>::clone(&$param)),*);
                    if was_callback_dropped() {
                        let _ = storage.swap_remove(i);
                    }
                }
            }

            /// Adds a callback.
            #[allow(dead_code)]
            $vis fn add_callback(&self, callback: impl FnMut($($arg),*)$($extra)* + 'static) {
                self.storage.write().push(Box::new(callback));
            }

            /// Gets a weak reference that can be used to add callbacks.
            #[allow(dead_code)]
            $vis fn get_ref(&self) -> CallbacksRef<dyn FnMut($($arg),*)$($extra)*> {
                (&self.storage).into()
            }

            /// Get the number of callbacks.
            #[allow(dead_code)]
            $vis fn len(&self) -> usize {
                self.storage.read().len()
            }

            /// Returns true iff there are no callbacks.
            #[allow(dead_code)]
            $vis fn is_empty(&self) -> bool {
                self.storage.read().is_empty()
            }
        }
    };
}

pub struct CallbacksRef<T: ?Sized> {
    storage: Weak<RwLock<Vec<Box<T>>>>,
}

impl<T: ?Sized> From<&Arc<RwLock<Vec<Box<T>>>>> for CallbacksRef<T> {
    fn from(storage: &Arc<RwLock<Vec<Box<T>>>>) -> Self {
        Self {
            storage: Arc::downgrade(&storage),
        }
    }
}

impl<T: ?Sized> Clone for CallbacksRef<T> {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
        }
    }
}

impl<C: ?Sized> CallbacksRef<C> {
    /// Adds a callback.
    pub fn add_callback(&self, callback: Box<C>) {
        if let Some(storage) = self.storage.upgrade() {
            storage.write().push(callback.into());
        }
    }

    /// Get the number of callbacks.
    pub fn len(&self) -> usize {
        if let Some(storage) = self.storage.upgrade() {
            storage.read().len()
        } else {
            0
        }
    }

    /// Returns true iff there are no callbacks.
    pub fn is_empty(&self) -> bool {
        if let Some(storage) = self.storage.upgrade() {
            storage.read().is_empty()
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    define_callbacks!(TestCallbacks1 => Fn(i: i32));
    define_callbacks!(TestCallbacks2 => FnMut(i: &i32));
    define_shared_callbacks!(TestCallbacks3 => Fn(i: &i32));
    define_shared_callbacks!(TestCallbacks4 => FnMut(i: &i32));
}
