use std::{marker::PhantomData, mem::MaybeUninit, ops::{Deref, DerefMut}, sync::Arc};

use crossbeam::queue::SegQueue;
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
    fn try_clear(&self) {
        if let Some(mut guard) = self.data.try_lock() {
            *guard = None;
        }
    }

    fn set(&self, data: T) {
        {
            let mut lock = self.data.lock();
            *lock = Some(data);
        }
        self.condvar.notify_one();
    }

    fn get(&self) -> T {
        let mut lock = self.data.lock();
        if lock.is_none() {
            self.condvar.wait(&mut lock);
        }
        unsafe {
            lock.take().unwrap_unchecked()
        }
    }

    fn try_get(&self) -> Option<T> {
        let mut lock = self.data.lock();
        lock.take()
    }
}


struct DataInner<T> {
    data: MaybeUninit<T>,
    released_mut: Mutex<()>,
    released_condvar: Condvar,
}

struct DataHandleInner<T> {
    new_callbacks: SegQueue<Box<dyn FnMut(&T) + Send>>,
    new_lendees: SegQueue<Arc<MonoQueue<Arc<DataInner<T>>>>>,
}

/// A handle to a shared data object.
/// 
/// You can add callbacks to be called when the data is updated,
/// and create lendees to receive the data.
#[repr(transparent)]
#[derive(Clone)]
pub struct DataHandle<T> {
    inner: Arc<DataHandleInner<T>>,
}

impl<T> DataHandle<T> {
    /// Adds a callback to be called when [`OwnedData::share`] is called.
    pub fn add_callback(&self, callback: impl FnMut(&T) + Send + 'static) {
        self.inner.new_callbacks.push(Box::new(callback));
    }

    /// Creates a lendee to receive the data when [`OwnedData::share`] is called.
    pub fn create_lendee(&self) -> SharedDataReceiver<T> {
        let queue: Arc<MonoQueue<Arc<DataInner<T>>>> = Default::default();
        self.inner.new_lendees.push(queue.clone());
        SharedDataReceiver {
            queue
        }
    }
}

/// An uninitialized heap allocation for `T`.
pub struct UninitOwnedData<T> {
    inner: Arc<DataInner<T>>,
    callbacks: Vec<Box<dyn FnMut(&T) + Send>>,
    data_handle: Arc<DataHandleInner<T>>,
    lendees: Vec<Arc<MonoQueue<Arc<DataInner<T>>>>>,
    phantom: PhantomData<fn() -> T>
}

impl<T> UninitOwnedData<T> {
    /// Adds a callback to be called when [`share`] is called.
    pub fn add_callback(&mut self, callback: impl FnMut(&T) + Send + 'static) {
        self.callbacks.push(Box::new(callback));
    }

    /// Creates a lendee to receive the data when [`share`] is called.
    pub fn create_lendee(&mut self) -> SharedDataReceiver<T> {
        let queue: Arc<MonoQueue<Arc<DataInner<T>>>> = Default::default();
        self.lendees.push(queue.clone());
        SharedDataReceiver {
            queue
        }
    }

    /// Returns a reference to the handle.
    pub fn get_data_handle(&self) -> &DataHandle<T> {
        unsafe {
            std::mem::transmute(&self.data_handle)
        }
    }

    /// Initializes the data.
    pub fn init(mut self, data: T) -> OwnedData<T> {
        unsafe {
            Arc::get_mut_unchecked(&mut self.inner).data.write(data);
        }
        OwnedData {
            inner: self.inner,
            callbacks: self.callbacks,
            data_handle: self.data_handle,
            lendees: self.lendees,
        }
    }
}

impl<T> Default for UninitOwnedData<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new(DataInner {
                data: MaybeUninit::uninit(),
                released_condvar: Condvar::new(),
                released_mut: Mutex::new(()),
            }),
            callbacks: Vec::new(),
            data_handle: Arc::new(DataHandleInner {
                new_callbacks: SegQueue::new(),
                new_lendees: SegQueue::new(),
            }),
            lendees: Vec::new(),
            phantom: PhantomData
        }
    }
}

/// A smart pointer to `T` that can invoke callbacks
/// and temporarily lend the data to other threads.
pub struct OwnedData<T> {
    inner: Arc<DataInner<T>>,
    callbacks: Vec<Box<dyn FnMut(&T) + Send>>,
    data_handle: Arc<DataHandleInner<T>>,
    lendees: Vec<Arc<MonoQueue<Arc<DataInner<T>>>>>
}


impl<T> From<T> for OwnedData<T> {
    fn from(value: T) -> Self {
        Self {
            inner: Arc::new(DataInner {
                data: MaybeUninit::new(value),
                released_condvar: Condvar::new(),
                released_mut: Mutex::new(()),
            }),
            callbacks: Vec::new(),
            data_handle: Arc::new(DataHandleInner {
                new_callbacks: SegQueue::new(),
                new_lendees: SegQueue::new(),
            }),
            lendees: Vec::new(),
        }
    }
}


impl<T> Deref for OwnedData<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe {
            self.inner.data.assume_init_ref()
        }
    }
}


impl<T> DerefMut for OwnedData<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            Arc::get_mut_unchecked(&mut self.inner).data.assume_init_mut()
        }
    }
}


impl<T> OwnedData<T> {
    /// Unwraps the data.
    pub fn unwrap(self) -> T {
        unsafe {
            Arc::try_unwrap(self.inner).unwrap_unchecked().data.assume_init()
        }
    }

    /// Unitinializes self and returns the data.
    pub fn uninit(self) -> (T, UninitOwnedData<T>) {
        unsafe {
            let data = self.inner.data.assume_init_read();
            (data, UninitOwnedData {
                inner: self.inner,
                callbacks: self.callbacks,
                data_handle: self.data_handle,
                lendees: self.lendees,
                phantom: PhantomData
            })
        }
    }

    /// Adds a callback to be called when [`share`] is called.
    pub fn add_callback(&mut self, callback: impl FnMut(&T) + Send + 'static) {
        self.callbacks.push(Box::new(callback));
    }

    /// Creates a lendee to receive the data when [`share`] is called.
    pub fn create_lendee(&mut self) -> SharedDataReceiver<T> {
        let queue: Arc<MonoQueue<Arc<DataInner<T>>>> = Default::default();
        self.lendees.push(queue.clone());
        SharedDataReceiver {
            queue
        }
    }

    /// Returns a reference to the handle.
    pub fn get_data_handle(&self) -> &DataHandle<T> {
        unsafe {
            std::mem::transmute(&self.data_handle)
        }
    }

    /// Invokes callbacks and lends the data to other threads.
    /// 
    /// Always assumes that ownership was shared.
    pub fn pessimistic_share(mut self) -> LoanedData<T> {
        self.callbacks.reserve(self.data_handle.new_callbacks.len());
        while let Some(callback) = self.data_handle.new_callbacks.pop() {
            self.callbacks.push(callback);
        }

        for callback in &mut self.callbacks {
            callback(unsafe { self.inner.data.assume_init_ref() });
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
            data_handle: self.data_handle,
            callbacks: self.callbacks,
            lendees: self.lendees,
        }
    }

    /// Invokes callbacks and lends the data to other threads.
    /// 
    /// If no threads were registered to receive the data, ownership is returned.
    /// Otherwise, a `LoanedData` object is returned which only allows immutable access.
    pub fn share(mut self) -> Result<Self, LoanedData<T>> {
        self.callbacks.reserve(self.data_handle.new_callbacks.len());
        while let Some(callback) = self.data_handle.new_callbacks.pop() {
            self.callbacks.push(callback);
        }

        for callback in &mut self.callbacks {
            callback(unsafe { self.inner.data.assume_init_ref() });
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
                data_handle: self.data_handle,
                callbacks: self.callbacks,
                lendees: self.lendees,
            })
        }
    }
}

/// A smart pointer to `T` that only allows immutable access.
pub struct LoanedData<T> {
    inner: Arc<DataInner<T>>,
    data_handle: Arc<DataHandleInner<T>>,
    callbacks: Vec<Box<dyn FnMut(&T) + Send>>,
    lendees: Vec<Arc<MonoQueue<Arc<DataInner<T>>>>>
}


impl<T> LoanedData<T> {
    /// Adds a callback to be called when [`share`] is called.
    pub fn add_callback(&mut self, callback: impl FnMut(&T) + Send + 'static) {
        self.callbacks.push(Box::new(callback));
    }

    /// Creates a lendee to receive the data when [`share`] is called.
    pub fn create_lendee(&mut self) -> SharedDataReceiver<T> {
        let queue: Arc<MonoQueue<Arc<DataInner<T>>>> = Default::default();
        self.lendees.push(queue.clone());
        SharedDataReceiver {
            queue
        }
    }

    /// Returns a reference to the handle.
    pub fn get_data_handle(&self) -> &DataHandle<T> {
        unsafe {
            std::mem::transmute(&self.data_handle)
        }
    }

    /// Waits for other threads to drop their ownership of the data.
    pub fn recall(self) -> OwnedData<T> {
        for lendee in &self.lendees {
            lendee.try_clear();
        }
        {
            let mut lock = self.inner.released_mut.lock();
            if Arc::strong_count(&self.inner) > 1 {
                self.inner.released_condvar.wait(&mut lock);
                debug_assert_eq!(Arc::strong_count(&self.inner), 1);
            }
        }
        OwnedData {
            inner: self.inner,
            data_handle: self.data_handle,
            callbacks: self.callbacks,
            lendees: self.lendees,
        }
    }

    /// Checks if other threads have dropped their ownership of the data, replacing
    /// data in-place if possible. Otherwise, ownership is replaced with `new_data`.
    pub fn replace(self, new_data: T) -> OwnedData<T> {
        for lendee in &self.lendees {
            lendee.try_clear();
        }
        if Arc::strong_count(&self.inner) == 1 {
            let mut owned = OwnedData {
                inner: self.inner,
                data_handle: self.data_handle,
                callbacks: self.callbacks,
                lendees: self.lendees,
            };
            *owned = new_data;
            return owned;
        }

        let inner = Arc::new(DataInner {
            data: MaybeUninit::new(new_data),
            released_condvar: Condvar::new(),
            released_mut: Mutex::new(()),
        });

        OwnedData {
            inner,
            data_handle: self.data_handle,
            callbacks: self.callbacks,
            lendees: self.lendees,
        }
    }

    /// Creates a new uninit object that maintains the callbacks and lendees.
    /// 
    /// This is different from [`OwnedData::uninit`], which reuses the heap allocation. This
    /// makes a new heap allocation.
    pub fn deinit(self) -> UninitOwnedData<T> {
        UninitOwnedData {
            inner: Arc::new(DataInner {
                data: MaybeUninit::uninit(),
                released_condvar: Condvar::new(),
                released_mut: Mutex::new(()),
            }),
            callbacks: self.callbacks,
            data_handle: self.data_handle,
            lendees: self.lendees,
            phantom: PhantomData
        }
    }
}

impl<T> Deref for LoanedData<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.inner.data.assume_init_ref() }
    }
}

/// Temporary shared ownership of `T`.
/// 
/// In most cases, a thread is waiting on all `SharedData` objects to be dropped,
/// so dropping this object as early as possible is recommended.
pub struct SharedData<T> {
    inner: Arc<DataInner<T>>,
}

impl<T> Deref for SharedData<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.inner.data.assume_init_ref() }
    }
}

impl<T> Drop for SharedData<T> {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 2 {
            let _guard = self.inner.released_mut.lock();
            self.inner.released_condvar.notify_one();
        }
    }
}

/// A receiver for shared data.
pub struct SharedDataReceiver<T> {
    queue: Arc<MonoQueue<Arc<DataInner<T>>>>
}

impl<T> SharedDataReceiver<T> {
    /// Waits until the provider shares the data.
    /// 
    /// If the provider has already shared the data, the data is returned immediately.
    /// If the provider provides, then recalls, before this method is called, this method will
    /// wait until the provider shares the data again.
    pub fn get(&self) -> SharedData<T> {
        SharedData {
            inner: self.queue.get()
        }
    }

    /// Tries to get the data without waiting.
    pub fn try_get(&self) -> Option<SharedData<T>> {
        Some(SharedData {
            inner: self.queue.try_get()?
        })
    }
}

/// A convenience type for handling both owned and loaned data under the same type.
pub enum MaybeOwned<T> {
    Owned(OwnedData<T>),
    Loaned(LoanedData<T>),
}

impl<T> MaybeOwned<T> {
    /// Tries to unwrap the data if owned.
    pub fn try_unwrap(self) -> Result<T, Self> {
        match self {
            Self::Owned(owned) => Ok(owned.unwrap()),
            Self::Loaned(loaned) => Err(Self::Loaned(loaned))
        }
    }

    /// Returns a mutable reference to the data if owned.
    pub fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Owned(owned) => Some(owned),
            _ => None
        }
    }
    
    /// Adds a callback to be called when [`share`] is called.
    pub fn add_callback(&mut self, callback: impl FnMut(&T) + Send + 'static) {
        match self {
            Self::Owned(owned) => owned.add_callback(callback),
            Self::Loaned(loaned) => loaned.add_callback(callback),
        }
    }

    /// Creates a lendee to receive the data when [`share`] is called.
    pub fn create_lendee(&mut self) -> SharedDataReceiver<T> {
        match self {
            Self::Owned(owned) => owned.create_lendee(),
            Self::Loaned(loaned) => loaned.create_lendee(),
        }
    }

    /// Returns a reference to the handle.
    pub fn get_data_handle(&self) -> &DataHandle<T> {
        match self {
            Self::Owned(owned) => owned.get_data_handle(),
            Self::Loaned(loaned) => loaned.get_data_handle(),
        }
    }
    
    /// Invokes callbacks and lends the data to other threads.
    /// 
    /// If no threads were registered to receive the data, ownership is returned.
    /// Otherwise, a `LoanedData` object is returned which only allows immutable access.
    pub fn share(&mut self) {
        match self {
            Self::Owned(owned) => {
                unsafe {
                    let owned_owned = std::ptr::read(owned);
                    match owned_owned.share() {
                        Ok(x) => {
                            std::ptr::write(owned, x);
                        }
                        Err(x) => {
                            std::ptr::write(self, Self::Loaned(x));
                        }
                    }
                }
            }
            Self::Loaned(_) => {}
        }
    }

    /// Checks if other threads have dropped their ownership of the data, replacing
    /// data in-place if possible. Otherwise, ownership is replaced with `new_data`.
    pub fn replace(&mut self, new_data: T) {
        match self {
            Self::Owned(owned) => {
                *owned.deref_mut() = new_data;
            },
            Self::Loaned(loaned) => {
                unsafe {
                    let owned_loaded = std::ptr::read(loaned);
                    let owned = owned_loaded.replace(new_data);
                    std::ptr::write(self, Self::Owned(owned));
                }
            }
        }
    }

    /// Waits for other threads to drop their ownership of the data.
    pub fn recall(&mut self) {
        match self {
            Self::Owned(_) => {}
            Self::Loaned(loaned) => {
                unsafe {
                    let owned_loaded = std::ptr::read(loaned);
                    let owned = owned_loaded.recall();
                    std::ptr::write(self, Self::Owned(owned));
                }
            }
        }
    }
}

impl<T> Deref for MaybeOwned<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(owned) => owned,
            Self::Loaned(loaned) => loaned,
        }
    }
}


impl<T> From<T> for MaybeOwned<T> {
    fn from(value: T) -> Self {
        Self::Owned(value.into())
    }
}


impl<T> From<OwnedData<T>> for MaybeOwned<T> {
    fn from(value: OwnedData<T>) -> Self {
        Self::Owned(value)
    }
}

impl<T> From<LoanedData<T>> for MaybeOwned<T> {
    fn from(value: LoanedData<T>) -> Self {
        Self::Loaned(value)
    }
}


#[cfg(test)]
mod tests {
    use super::MaybeOwned;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn test01() {
        let mut data: MaybeOwned<i32> = 5.into();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter2 = counter.clone();
        data.add_callback(move |&x| {
            assert_eq!(x, 5);
            counter2.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        data.share();
        data.recall();
        data.share();
        data.recall();
        assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 2);
    }
}