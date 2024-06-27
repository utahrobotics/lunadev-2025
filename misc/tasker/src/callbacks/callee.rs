use std::sync::Arc;

use crossbeam::queue::{ArrayQueue, SegQueue};
use tokio::sync::Notify;

use super::caller::try_drop_this_callback;

enum Queue<T> {
    Bounded(ArrayQueue<T>),
    Unbounded(SegQueue<T>),
}

impl<T> Queue<T> {
    #[inline]
    fn push(&self, value: T) -> Result<(), T> {
        match self {
            Self::Bounded(queue) => queue.push(value),
            Self::Unbounded(queue) => {
                queue.push(value);
                Ok(())
            }
        }
    }

    #[inline]
    fn force_push(&self, value: T) -> Option<T> {
        match self {
            Self::Bounded(queue) => queue.force_push(value),
            Self::Unbounded(queue) => {
                queue.push(value);
                None
            }
        }
    }

    #[inline]
    fn pop(&self) -> Option<T> {
        match self {
            Self::Bounded(queue) => queue.pop(),
            Self::Unbounded(queue) => queue.pop(),
        }
    }
}

struct SubscriberInner<T> {
    queue: Queue<T>,
    notify: Notify,
}

pub struct Subscriber<T> {
    inner: Arc<SubscriberInner<T>>,
}

impl<T> Subscriber<T> {
    pub fn new(max_size: usize) -> Self {
        Self {
            inner: Arc::new(SubscriberInner {
                queue: Queue::Bounded(ArrayQueue::new(max_size)),
                notify: Notify::new(),
            }),
        }
    }
    pub fn new_unbounded() -> Self {
        Self {
            inner: Arc::new(SubscriberInner {
                queue: Queue::Unbounded(SegQueue::new()),
                notify: Notify::new(),
            }),
        }
    }

    #[inline]
    pub fn try_recv(&self) -> Option<T> {
        self.inner.queue.pop()
    }

    #[inline]
    pub fn is_closed(&self) -> bool {
        Arc::weak_count(&self.inner) == 0
    }

    pub async fn recv(&self) -> Option<T> {
        loop {
            if let Some(value) = self.inner.queue.pop() {
                return Some(value);
            }

            if self.is_closed() {
                return None;
            }

            self.inner.notify.notified().await;
        }
    }

    pub async fn recv_or_never(&self) -> T {
        if let Some(value) = self.recv().await {
            value
        } else {
            std::future::pending().await
        }
    }

    pub fn create_conservative_callback(&self) -> impl Fn(T) + Send + Sync
    where
        T: Send,
    {
        let inner = Arc::downgrade(&self.inner.clone());
        move |value| {
            let Some(inner) = inner.upgrade() else {
                try_drop_this_callback();
                return;
            };
            if inner.queue.push(value).is_ok() {
                inner.notify.notify_one();
            }
        }
    }

    pub fn create_callback(&self) -> impl Fn(T) + Send + Sync
    where
        T: Send,
    {
        let inner = Arc::downgrade(&self.inner.clone());
        move |value| {
            let Some(inner) = inner.upgrade() else {
                try_drop_this_callback();
                return;
            };
            if inner.queue.force_push(value).is_none() {
                inner.notify.notify_one();
            }
        }
    }
}
