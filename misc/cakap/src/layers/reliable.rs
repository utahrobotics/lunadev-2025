use std::{ops::DerefMut, sync::Arc};

use bytes::BytesMut;
use tokio::sync::Mutex;

use super::{Layer, UIntVariant};


enum ReliableStrategy {
    GenerateUnique {
        id_type: UIntVariant,
    },
    UseLastBytes {
        size: usize
    },
    NonUnique,
}


pub struct Reliable<T> {
    pub forward: T,
    strategy: ReliableStrategy,
}


pub struct ReliableGuard<T> {
    forward: Arc<Mutex<T>>,
}


pub struct ReliableGuardBuilder {
}


impl ReliableGuardBuilder {
    pub fn new<T>(forward: T) -> ReliableGuard<T> {
        let forward = Arc::new(Mutex::new(forward));
        let forward2 = forward.clone();
        tokio::spawn(async move {

        });
        ReliableGuard {
            forward,
        }
    }
}


pub struct ReliableToken(());


pub trait HasReliableGuard {
    fn reliable_guard_send(
        &mut self,
        data: BytesMut,
        token: ReliableToken,
    ) -> impl std::future::Future<Output=()>;
}


impl<T> HasReliableGuard for ReliableGuard<T>
where
    T: Layer<SendItem=BytesMut>,
{
    async fn reliable_guard_send(
        &mut self,
        data: BytesMut,
        _token: ReliableToken,
    ) {
        let _ = self.forward.lock().await.send(data).await;
    }
}


impl<T> Layer for ReliableGuard<T>
where
    T: Layer,
{
    type SendError = T::SendError;
    type RecvError = T::RecvError;

    type SendItem = T::SendItem;
    type RecvItem = T::RecvItem;

    async fn send(
        &mut self,
        data: Self::SendItem,
    ) -> Result<(), Self::SendError> {
        self.forward.lock().await.deref_mut().send(data).await
    }

    async fn recv(
        &mut self,
    ) -> Result<Self::RecvItem, Self::RecvError> {
        self.forward.lock().await.deref_mut().recv().await
    }
}