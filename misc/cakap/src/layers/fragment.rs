use std::{collections::HashMap, hash::BuildHasherDefault, sync::RwLock};

use bytes::BytesMut;
use fxhash::FxHashMap;
use indexmap::{IndexMap, IndexSet};
use reed_solomon_erasure::{galois_16, galois_8};

use super::{Layer, UInt, UIntVariant};

static REED_SOLOMON_ERASURES_8: RwLock<FxHashMap<(usize, usize), galois_8::ReedSolomon>> =
    RwLock::new(FxHashMap::with_hasher(BuildHasherDefault::new()));
static REED_SOLOMON_ERASURES_16: RwLock<FxHashMap<(usize, usize), galois_16::ReedSolomon>> =
    RwLock::new(FxHashMap::with_hasher(BuildHasherDefault::new()));

#[derive(Debug)]
pub enum FragmentSendError<E> {
    PacketTooBig,
    ForwardError(E),
}

impl<E> From<E> for FragmentSendError<E> {
    fn from(e: E) -> Self {
        FragmentSendError::ForwardError(e)
    }
}

#[derive(Debug)]
pub enum FragmentRecvError<E> {
    PacketTooSmall,
    DuplicateFragment,
    BadFragmentIndex,
    ForwardError(E),
}

impl<E> From<E> for FragmentRecvError<E> {
    fn from(e: E) -> Self {
        FragmentRecvError::ForwardError(e)
    }
}

pub struct Fragmenter<T> {
    pub max_fragment_payload_size: usize,
    header_size: usize,
    redundant_factor: f32,
    max_fragment_count: UInt,
    max_active_fragments: UInt,
    fragment_id_type: UIntVariant,
    send_active_fragments: IndexSet<u64>,
    recv_active_fragments: IndexMap<u64, Vec<BytesMut>>,
    pub forward: T,
}

impl<T> Layer for Fragmenter<T>
where
    T: Layer<SendItem = BytesMut, RecvItem = BytesMut>,
{
    type SendError = FragmentSendError<T::SendError>;
    type RecvError = FragmentRecvError<T::RecvError>;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        let mut fragment_count = data.len().div_ceil(self.max_fragment_payload_size) as u64;
        let redundant_fragment_count =
            (fragment_count as f32 * self.redundant_factor).round() as u64;
        fragment_count += redundant_fragment_count;

        // A Header is comprised of the fragment index, number of fragments, and fragment id
        // let header_size = self.max_fragment_count.size() * 2 + self.fragment_id_type.size();
        let max_fragment_count = self.max_fragment_count.to_u64();
        if fragment_count > max_fragment_count {
            return Err(FragmentSendError::PacketTooBig);
        }

        let actual_fragment_payload_size = data
            .len()
            .div_ceil((fragment_count - redundant_fragment_count) as usize);

        let max_active_fragments = self.max_active_fragments.to_u64();
        if self.send_active_fragments.len() as u64 >= max_active_fragments {
            self.send_active_fragments.shift_remove(&0);
        }
        let mut fragment_id;
        {
            let mut rng = rand::thread_rng();
            loop {
                fragment_id = self.fragment_id_type.random(&mut rng).to_u64();

                if self.send_active_fragments.insert(fragment_id) {
                    break;
                }
            }
        }

        for fragment_data in data.chunks(actual_fragment_payload_size) {
            let mut fragment_packet =
                BytesMut::zeroed(actual_fragment_payload_size + self.header_size);
            fragment_packet[0..fragment_data.len()].copy_from_slice(fragment_data);

            self.max_fragment_count
                .copy_to_slice(
                    &mut fragment_packet[actual_fragment_payload_size
                        ..actual_fragment_payload_size + self.max_fragment_count.size()],
                )
                .unwrap();
            let final_fragment_portion = &mut fragment_packet
                [actual_fragment_payload_size + self.max_fragment_count.size()..];

            self.fragment_id_type
                .with_u64(fragment_id)
                .copy_to_slice(final_fragment_portion)
                .unwrap();

            self.forward.send(fragment_packet).await?;
        }

        Ok(())
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        loop {
            let data = self.forward.recv().await?;
            let fragment_id;
            let fragment_id_start;
            match self.fragment_id_type {
                UIntVariant::U8 => {
                    fragment_id_start = data.len().saturating_sub(1);
                    let fragment_id_slice = &data[fragment_id_start..];
                    let fragment_id_array: [u8; 1] = fragment_id_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_id = u8::from_be_bytes(fragment_id_array) as u64;
                }
                UIntVariant::U16 => {
                    fragment_id_start = data.len().saturating_sub(2);
                    let fragment_id_slice = &data[fragment_id_start..];
                    let fragment_id_array: [u8; 2] = fragment_id_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_id = u16::from_be_bytes(fragment_id_array) as u64;
                }
                UIntVariant::U32 => {
                    fragment_id_start = data.len().saturating_sub(4);
                    let fragment_id_slice = &data[fragment_id_start..];
                    let fragment_id_array: [u8; 4] = fragment_id_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_id = u32::from_be_bytes(fragment_id_array) as u64;
                }
                UIntVariant::U64 => {
                    fragment_id_start = data.len().saturating_sub(8);
                    let fragment_id_slice = &data[fragment_id_start..];
                    let fragment_id_array: [u8; 8] = fragment_id_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_id = u64::from_be_bytes(fragment_id_array) as u64;
                }
            }
            let fragment_index;
            let fragment_index_start;
            let max_fragment_count;
            match self.max_fragment_count {
                UInt::U8(n) => {
                    max_fragment_count = n as u64;
                    fragment_index_start = fragment_id_start.saturating_sub(1);
                    let fragment_index_slice = &data[fragment_index_start..fragment_id_start];
                    let fragment_index_array: [u8; 1] = fragment_index_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_index = u8::from_be_bytes(fragment_index_array) as u64;
                    if fragment_index >= max_fragment_count {
                        return Err(FragmentRecvError::BadFragmentIndex);
                    }
                }
                UInt::U16(n) => {
                    max_fragment_count = n as u64;
                    fragment_index_start = fragment_id_start.saturating_sub(2);
                    let fragment_index_slice = &data[fragment_index_start..fragment_id_start];
                    let fragment_index_array: [u8; 2] = fragment_index_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_index = u16::from_be_bytes(fragment_index_array) as u64;
                    if fragment_index >= max_fragment_count {
                        return Err(FragmentRecvError::BadFragmentIndex);
                    }
                }
                UInt::U32(n) => {
                    max_fragment_count = n as u64;
                    fragment_index_start = fragment_id_start.saturating_sub(4);
                    let fragment_index_slice = &data[fragment_index_start..fragment_id_start];
                    let fragment_index_array: [u8; 4] = fragment_index_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_index = u32::from_be_bytes(fragment_index_array) as u64;
                    if fragment_index >= max_fragment_count {
                        return Err(FragmentRecvError::BadFragmentIndex);
                    }
                }
                UInt::U64(n) => {
                    max_fragment_count = n as u64;
                    fragment_index_start = fragment_id_start.saturating_sub(8);
                    let fragment_index_slice = &data[fragment_index_start..fragment_id_start];
                    let fragment_index_array: [u8; 8] = fragment_index_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_index = u64::from_be_bytes(fragment_index_array) as u64;
                    if fragment_index >= max_fragment_count {
                        return Err(FragmentRecvError::BadFragmentIndex);
                    }
                }
            }
            if let Some(buffers) = self.recv_active_fragments.get_mut(&fragment_id) {
                if !buffers[fragment_index as usize].is_empty() {
                    return Err(FragmentRecvError::DuplicateFragment);
                }
                buffers[fragment_index as usize] = data;
            } else {
                let mut buffers = Vec::new();
                for _ in 0..max_fragment_count {
                    buffers.push(BytesMut::new());
                }
                buffers[fragment_index as usize] = data;
                self.recv_active_fragments.insert(fragment_id, buffers);
            }
        }
    }
}
