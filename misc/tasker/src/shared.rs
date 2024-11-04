use std::{ops::{Deref, DerefMut}, sync::Arc};

use crossbeam::{queue::SegQueue, sync::{Parker, Unparker}};
use parking_lot::{Condvar, Mutex};


struct MonoQueue<T> {
    data: Mutex<Option<T>>,
    condvar: Condvar
}

impl<T> Default for MonoQueue<T> {
    fn default() -> Self {
        Self {
            data: Mutex::new(None),
            condvar: Condvar::new()
        }
    }
}


impl<T> MonoQueue<T> {
    fn set(&self, data: T) {
        {
            let mut lock = self.data.lock();
            *lock = Some(data);
        }
        self.condvar.notify_all();
    }

    fn wait(&self) -> T {
        let mut lock = self.data.lock();
        self.condvar.wait(&mut lock);
        unsafe {
            lock.take().unwrap_unchecked()
        }
    }
}


struct DataInner<T> {
    data: T,
    unparker: Unparker
}

struct DataHandleInner<T> {
    new_callbacks: SegQueue<Box<dyn FnMut(&T)>>,
    new_lendees: SegQueue<Arc<MonoQueue<Arc<DataInner<T>>>>>,
}

pub struct DataHandle<T> {
    inner: Arc<DataHandleInner<T>>,
}

impl<T> DataHandle<T> {
    pub fn add_callback(&self, callback: impl FnMut(&T) + 'static) {
        self.inner.new_callbacks.push(Box::new(callback));
    }

    pub fn create_lendee(&self) -> SharedDataReceiver<T> {
        let queue: Arc<MonoQueue<Arc<DataInner<T>>>> = Default::default();
        self.inner.new_lendees.push(queue.clone());
        SharedDataReceiver {
            queue
        }
    }
}

pub struct OwnedData<T> {
    inner: Arc<DataInner<T>>,
    parker: Parker,
    callbacks: Vec<Box<dyn FnMut(&T)>>,
    data_handle: Arc<DataHandleInner<T>>,
    lendees: Vec<Arc<MonoQueue<Arc<DataInner<T>>>>>
}


impl<T> Deref for OwnedData<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner.data
    }
}


impl<T> DerefMut for OwnedData<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            &mut Arc::get_mut_unchecked(&mut self.inner).data
        }
    }
}


impl<T> OwnedData<T> {
    pub fn share(mut self) -> LoanedData<T> {
        self.callbacks.reserve(self.data_handle.new_callbacks.len());
        while let Some(callback) = self.data_handle.new_callbacks.pop() {
            self.callbacks.push(callback);
        }

        for callback in &mut self.callbacks {
            callback(&self.inner.data);
        }

        self.lendees.reserve(self.data_handle.new_lendees.len());
        while let Some(lendee) = self.data_handle.new_lendees.pop() {
            self.lendees.push(lendee);
        }

        for lendee in &self.lendees {
            lendee.set(self.inner.clone());
        }

        LoanedData {
            inner: self.inner,
            parker: self.parker,
            data_handle: self.data_handle,
            callbacks: self.callbacks,
            lendees: self.lendees,
        }
    }

    pub fn try_share(mut self) -> Result<Self, LoanedData<T>> {
        self.callbacks.reserve(self.data_handle.new_callbacks.len());
        while let Some(callback) = self.data_handle.new_callbacks.pop() {
            self.callbacks.push(callback);
        }

        for callback in &mut self.callbacks {
            callback(&self.inner.data);
        }

        self.lendees.reserve(self.data_handle.new_lendees.len());
        while let Some(lendee) = self.data_handle.new_lendees.pop() {
            self.lendees.push(lendee);
        }

        if self.lendees.is_empty() {
            Ok(self)
        } else {
            for lendee in &self.lendees {
                lendee.set(self.inner.clone());
            }
    
            Err(LoanedData {
                inner: self.inner,
                parker: self.parker,
                data_handle: self.data_handle,
                callbacks: self.callbacks,
                lendees: self.lendees,
            })
        }
    }
}

pub struct LoanedData<T> {
    inner: Arc<DataInner<T>>,
    parker: Parker,
    data_handle: Arc<DataHandleInner<T>>,
    callbacks: Vec<Box<dyn FnMut(&T)>>,
    lendees: Vec<Arc<MonoQueue<Arc<DataInner<T>>>>>
}


impl<T> LoanedData<T> {
    pub fn wait(self) -> OwnedData<T> {
        if Arc::strong_count(&self.inner) > 1 {
            self.parker.park();
            debug_assert_eq!(Arc::strong_count(&self.inner), 1);
        }
        OwnedData {
            inner: self.inner,
            parker: self.parker,
            data_handle: self.data_handle,
            callbacks: self.callbacks,
            lendees: self.lendees,
        }
    }

    pub fn replace(self, new_data: T) -> OwnedData<T> {
        let parker = Parker::new();
        let inner = Arc::new(DataInner {
            data: new_data,
            unparker: parker.unparker().clone(),
        });

        OwnedData {
            inner,
            parker,
            data_handle: self.data_handle,
            callbacks: self.callbacks,
            lendees: self.lendees,
        }
    }
}


pub struct SharedData<T> {
    inner: Arc<DataInner<T>>,
}

impl<T> Deref for SharedData<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner.data
    }
}

impl<T> Drop for SharedData<T> {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 2 {
            self.inner.unparker.unpark();
        }
    }
}

pub struct SharedDataReceiver<T> {
    queue: Arc<MonoQueue<Arc<DataInner<T>>>>
}

impl<T> SharedDataReceiver<T> {
    pub fn wait(&self) -> SharedData<T> {
        SharedData {
            inner: self.queue.wait()
        }
    }
}