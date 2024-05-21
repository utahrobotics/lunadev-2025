use bytes::BytesMut;

use super::Layer;

#[derive(Clone, Copy, Debug)]
pub enum SequenceOptions {
    U8 {
        window_size: u8
    },
    U16 {
        window_size: u16
    },
    U32 {
        window_size: u32
    },
    U64 {
        window_size: u64
    },
}


impl Default for SequenceOptions {
    fn default() -> Self {
        SequenceOptions::U16 { window_size: 128 }
    }
}


pub struct Sequenced<T> {
    options: SequenceOptions,
    counter: u64,
    pub forward: T
}


impl<T> Sequenced<T> {
    pub fn new(options: SequenceOptions, forward: T) -> Self {
        Sequenced {
            options,
            counter: 0,
            forward
        }
    }
}

#[derive(Debug)]
pub enum SequencedRecvError<E> {
    PacketTooSmall,
    ForwardError(E)
}


impl<E> From<E> for SequencedRecvError<E> {
    fn from(e: E) -> Self {
        SequencedRecvError::ForwardError(e)
    }
}


impl<T> Layer for Sequenced<T> where T: Layer<SendItem = BytesMut, RecvItem = BytesMut> {
    type SendError = T::SendError;
    type RecvError = SequencedRecvError<T::RecvError>;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, mut data: Self::SendItem) -> Result<(), Self::SendError> {
        match self.options {
            SequenceOptions::U8 { .. } => {
                data.extend_from_slice(&(self.counter as u8).to_be_bytes());
                self.counter = self.counter.wrapping_add(1) % u8::MAX as u64;
            }
            SequenceOptions::U16 { .. } => {
                data.extend_from_slice(&(self.counter as u16).to_be_bytes());
                self.counter = self.counter.wrapping_add(1) % u16::MAX as u64;
            }
            SequenceOptions::U32 { .. } => {
                data.extend_from_slice(&(self.counter as u32).to_be_bytes());
                self.counter = self.counter.wrapping_add(1) % u32::MAX as u64;
            }
            SequenceOptions::U64 { .. } => {
                data.extend_from_slice(&(self.counter as u64).to_be_bytes());
                self.counter = self.counter.wrapping_add(1) % u64::MAX as u64;
            }
        }
        self.forward.send(data).await
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        loop {
            let mut data = self.forward.recv().await?;

            macro_rules! sequence {
                ($num: ty, $len: literal, $window_size: ident) => {{
                    let slice_index = data.len().saturating_sub($len);
                    let counter_slice = data.split_at(slice_index).1;
                    let incoming_index: [u8; $len] = counter_slice.try_into().map_err(|_| SequencedRecvError::PacketTooSmall)?;
                    let incoming_index = <$num>::from_be_bytes(incoming_index) as u64;
                    let upper_window_index = (<$num>::MAX - $window_size) as u64;

                    if incoming_index >= self.counter {
                        if self.counter < $window_size as u64 && incoming_index >= upper_window_index {
                            continue;
                        }
                    } else if self.counter < upper_window_index {
                        continue;
                    }
                    self.counter = incoming_index.wrapping_add(1) % <$num>::MAX as u64;
                    data.truncate(slice_index);
                }}
            }

            match self.options {
                SequenceOptions::U8 { window_size } => sequence!(u8, 1, window_size),
                SequenceOptions::U16 { window_size } => sequence!(u16, 2, window_size),
                SequenceOptions::U32 { window_size } => sequence!(u32, 4, window_size),
                SequenceOptions::U64 { window_size } => sequence!(u64, 8, window_size),
            }
    
            break Ok(data);
        }
    }
}
