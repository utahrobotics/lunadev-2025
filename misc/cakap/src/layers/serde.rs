use std::{marker::PhantomData, ops::Deref};

use bytes::BytesMut;

use super::Layer;

pub struct Bitcoder<T, L> {
    pub forward: L,
    phantom: PhantomData<T>,
}

pub enum BitcoderRecvError<E> {
    DecodeError(bitcode::Error),
    ForwardError(E),
}

impl<E> From<E> for BitcoderRecvError<E> {
    fn from(e: E) -> Self {
        BitcoderRecvError::ForwardError(e)
    }
}

impl<
        T: bitcode::Encode + bitcode::DecodeOwned,
        L,
        B1: From<BytesMut>,
        B2: Deref<Target = [u8]>,
    > Layer for Bitcoder<T, L>
where
    L: Layer<SendItem = B1, RecvItem = B2>,
{
    type SendError = L::SendError;
    type RecvError = BitcoderRecvError<L::RecvError>;

    type SendItem = T;
    type RecvItem = T;

    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        let src_bytes = bitcode::encode(&data);
        let mut bytes = BytesMut::with_capacity(src_bytes.len());
        bytes.extend_from_slice(&src_bytes);
        self.forward.send(bytes.into()).await
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        let source = self.forward.recv().await?;
        let data = bitcode::decode(&source).map_err(BitcoderRecvError::DecodeError)?;
        Ok(data)
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        self.forward.get_max_packet_size()
    }
}
