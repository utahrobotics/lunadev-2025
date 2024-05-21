use std::{hash::BuildHasherDefault, sync::RwLock};

use bytes::BytesMut;
use fxhash::{FxBuildHasher, FxHashMap};
use reed_solomon_erasure::galois_8;

use super::{Layer, UInt, UIntVariant};


static REED_SOLOMON_ERASURES_8: RwLock<FxHashMap<(usize, usize), galois_8::ReedSolomon>> = RwLock::new(FxHashMap::with_hasher(BuildHasherDefault::new()));

#[derive(Debug)]
pub enum FragmentSendError<E> {
    PacketTooBig,
    ForwardError(E)
}

#[derive(Debug)]
pub enum FragmentRecvError<E> {
    PacketTooSmall,
    ForwardError(E)
}


pub struct Fragmenter<T> {
    pub max_fragment_size: usize,
    redundant_factor: f32,
    max_fragment_count: UInt,
    max_active_fragments: UInt,
    fragment_id_type: UIntVariant,
    pub forward: T,
}


impl<T> Layer for Fragmenter<T> where T: Layer<SendItem = BytesMut, RecvItem = BytesMut> {
    type SendError = FragmentSendError<T::SendError>;
    type RecvError = FragmentRecvError<T::RecvError>;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, mut data: Self::SendItem) -> Result<(), Self::SendError> {
        
        self.forward.send(data).await
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        loop {
            
        }
    }
}