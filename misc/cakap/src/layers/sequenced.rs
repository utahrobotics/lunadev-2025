use bytes::BytesMut;

use super::{reliable::{HasReliableGuard, ReliableToken}, Layer, UInt};

pub fn default_window_size() -> UInt {
    UInt::U16(128)
}

pub struct Sequenced<T> {
    window_size: UInt,
    counter: u64,
    pub forward: T,
}

impl<T> Sequenced<T> {
    pub fn new(window_size: UInt, forward: T) -> Self {
        Sequenced {
            window_size,
            counter: 0,
            forward,
        }
    }

    pub fn map<V>(self, new: V) -> Sequenced<V> {
        Sequenced {
            window_size: self.window_size,
            counter: self.counter,
            forward: new,
        }
    }
}

#[derive(Debug)]
pub enum SequencedRecvError<E> {
    PacketTooSmall,
    ForwardError(E),
}

impl<E> From<E> for SequencedRecvError<E> {
    fn from(e: E) -> Self {
        SequencedRecvError::ForwardError(e)
    }
}

impl<T> Layer for Sequenced<T>
where
    T: Layer<SendItem = BytesMut, RecvItem = BytesMut>,
{
    type SendError = T::SendError;
    type RecvError = SequencedRecvError<T::RecvError>;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, mut data: Self::SendItem) -> Result<(), Self::SendError> {
        self.window_size
            .with_u64(self.counter)
            .extend_bytes_mut(&mut data);
        self.forward.send(data).await
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        loop {
            let mut data = self.forward.recv().await?;

            if data.len() < self.window_size.size() as usize {
                return Err(SequencedRecvError::PacketTooSmall);
            }
            let slice_index = data.len() - self.window_size.size();
            let counter_slice = data.split_at(slice_index).1;
            let incoming_index = self
                .window_size
                .try_with_slice(counter_slice)
                .unwrap()
                .to_u64();
            let max_value = self.window_size.to_variant().max_value().to_u64();
            let window_size = self.window_size.to_u64();
            let upper_window_index = max_value - window_size;
            if incoming_index >= self.counter {
                if self.counter < window_size && incoming_index >= upper_window_index {
                    continue;
                }
            } else if self.counter < upper_window_index {
                continue;
            }
            self.counter = incoming_index.wrapping_add(1) % max_value;
            data.truncate(slice_index);

            break Ok(data);
        }
    }
}


impl<T: HasReliableGuard> HasReliableGuard for Sequenced<T> {
    #[inline(always)]
    async fn reliable_guard_send(
        &mut self,
        data: BytesMut,
        token: ReliableToken,
    ) {
        self.forward.reliable_guard_send(data, token).await
    }
}