use std::{cell::Cell, marker::PhantomData, ops::ControlFlow, sync::Arc};

use parking_lot::{Mutex, RwLock};

thread_local! {
    static DROP_CALLBACK: Cell<bool> = Cell::new(false);
}


pub trait Callback<A>: Send + Sync {
    fn call_immut(&self, arg: A) -> ControlFlow<()>;
    fn call_mut(&mut self, arg: A) -> ControlFlow<()>;
}

pub fn immut_callback<'a, A: 'a>(f: impl Fn(A) + Send + Sync) -> Box<dyn Callback<A>> {
    Box::new(FnCallback(Box::new(f), PhantomData))
}


struct FnCallback<'a, A: 'a>(Box<dyn Fn(A) + Send + Sync>, PhantomData<&'a ()>);

impl<'a, A> Callback<A> for FnCallback<'a, A> {
    fn call_immut(&self, arg: A) -> ControlFlow<()> {
        self.0(arg);
        ControlFlow::Continue(())
    }

    fn call_mut(&mut self, arg: A) -> ControlFlow<()> {
        self.0(arg);
        ControlFlow::Continue(())
    }
}


// enum CallbackDispatch<A> {
//     Mut(Mutex<Box<dyn FnMut(A) + Send + Sync>>),
//     Immut(Box<dyn Fn(A) + Send + Sync>),
// }

// impl<A> CallbackDispatch<A> {
//     fn call(&self, arg: A) {
//         match self {
//             Self::Mut(f) => f.lock()(arg),
//             Self::Immut(f) => f(arg),
//         }
//     }

//     fn call_mut(&mut self, arg: A) {
//         match self {
//             Self::Mut(f) => f.get_mut()(arg),
//             Self::Immut(f) => f(arg),
//         }
//     }
// }

enum CallbackStorage<C:? Sized> {
    Vec(Vec<Box<C>>),
    RwLock(Arc<RwLock<Vec<Box<C>>>>),
}


// pub struct Callbacks<A: Arg> {
//     storage: CallbackStorage<A::Item>,
// }


// impl<A: Arg> Callbacks<A> {
//     pub fn call_immut(&self, arg: A::Item) {
//         match &self.storage {
//             CallbackStorage::Vec(vec) => Self::call_vec(vec, arg),
//             CallbackStorage::RwLock(shared) => {
//                 if let Some(mut vec) = shared.try_write() {
//                     Self::call_vec_mut(&mut vec, arg);
//                     return;
//                 }
//                 let vec = shared.read();
//                 Self::call_vec(&vec, arg);
//             }
//         }
//     }

//     pub fn call(&mut self, arg: A::Item) {
//         match &mut self.storage {
//             CallbackStorage::Vec(vec) => Self::call_vec_mut(vec, arg),
//             CallbackStorage::RwLock(shared) => {
//                 if let Some(mut vec) = shared.try_write() {
//                     Self::call_vec_mut(&mut vec, arg);
//                     return;
//                 }
//                 let vec = shared.read();
//                 Self::call_vec(&vec, arg);
//             }
//         }
//     }

//     fn call_vec(vec: &Vec<CallbackDispatch<A::Item>>, arg: A::Item) {
//         vec.iter().for_each(|c| c.call(arg.clone()));
//     }

//     fn call_vec_mut(vec: &mut Vec<CallbackDispatch<A::Item>>, arg: A::Item) {
//         for i in (0..vec.len()).rev() {
//             DROP_CALLBACK.set(false);
//             vec[i].call_mut(arg.clone());
//             if DROP_CALLBACK.get() {
//                 vec.swap_remove(i);
//             }
//         }
//         vec.iter().for_each(|c| c.call(arg.clone()));
//     }

//     pub fn try_unshare(&mut self) -> bool {
//         match std::mem::replace(&mut self.storage, CallbackStorage::Vec(vec![])) {
//             CallbackStorage::Vec(v) => {
//                 self.storage = CallbackStorage::Vec(v);
//                 true
//             }
//             CallbackStorage::RwLock(shared) => match Arc::try_unwrap(shared) {
//                 Ok(inner) => {
//                     self.storage = CallbackStorage::Vec(inner.into_inner());
//                     true
//                 }
//                 Err(shared) => {
//                     self.storage = CallbackStorage::RwLock(shared);
//                     false
//                 }
//             }
//         }
//     }

//     pub fn share(&mut self) -> CallbacksRef<A> {
//         match &mut self.storage {
//             CallbackStorage::Vec(vec) => {
//                 let shared = Arc::new(RwLock::new(std::mem::replace(vec, vec![])));
//                 self.storage = CallbackStorage::RwLock(shared.clone());
//                 CallbacksRef { storage: shared }
//             },
//             CallbackStorage::RwLock(shared) => CallbacksRef { storage: shared.clone() },
//         }
//     }

//     pub fn add_callback(&mut self, callback: impl Fn(A::Item) + Send + Sync + 'static) {
//         let callback = CallbackDispatch::Immut(Box::new(callback));
//         match &mut self.storage {
//             CallbackStorage::Vec(vec) => vec.push(callback),
//             CallbackStorage::RwLock(shared) => shared.write().push(callback),
//         }
//     }

//     pub fn add_mut_callback(&mut self, callback: impl FnMut(A::Item) + Send + Sync + 'static) {
//         let callback = CallbackDispatch::Mut(Mutex::new(Box::new(callback)));
//         match &mut self.storage {
//             CallbackStorage::Vec(vec) => vec.push(callback),
//             CallbackStorage::RwLock(shared) => shared.write().push(callback),
//         }
//     }
// }


// pub struct CallbacksRef<A: Arg> {
//     storage: Arc<RwLock<Vec<CallbackDispatch<A::Item>>>>
// }

// mod logging {
//     use super::*;

//     #[derive(Default)]
//     struct LogPub {
//         callbacks: Callbacks<dyn for<'a, 'b> Arg<Item=&'a log::Record<'b>>>,
//     }

//     impl log::Log for LogPub {
//         fn enabled(&self, _metadata: &log::Metadata) -> bool {
//             true
//         }

//         fn log(&self, record: &log::Record) {
//             self.callbacks.call(record);
//         }

//         fn flush(&self) {}
//     }
// }