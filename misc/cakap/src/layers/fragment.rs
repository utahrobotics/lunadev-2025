use std::{collections::HashMap, hash::BuildHasherDefault, sync::RwLock};

use bytes::BytesMut;
use fxhash::FxHashMap;
use indexmap::{IndexMap, IndexSet};
use rand::Rng;
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
        let mut fragment_count = data.len().div_ceil(self.max_fragment_payload_size);
        let redundant_fragment_count =
            (fragment_count as f32 * self.redundant_factor).round() as usize;
        fragment_count += redundant_fragment_count;

        let mut header_size;
        let max_fragment_count = match self.max_fragment_count {
            UInt::U8(n) => {
                header_size = 1;
                n as u64
            }
            UInt::U16(n) => {
                header_size = 2;
                n as u64
            }
            UInt::U32(n) => {
                header_size = 4;
                n as u64
            }
            UInt::U64(n) => {
                header_size = 8;
                n
            }
        };
        if fragment_count as u64 > max_fragment_count {
            return Err(FragmentSendError::PacketTooBig);
        }

        let actual_fragment_payload_size = data
            .len()
            .div_ceil(fragment_count - redundant_fragment_count);

        let max_active_fragments = match self.max_active_fragments {
            UInt::U8(n) => n as u64,
            UInt::U16(n) => n as u64,
            UInt::U32(n) => n as u64,
            UInt::U64(n) => n as u64,
        };
        if self.send_active_fragments.len() as u64 == max_active_fragments {
            self.send_active_fragments.shift_remove(&0);
        }
        let mut fragment_id;
        {
            let mut rng = rand::thread_rng();
            loop {
                match self.fragment_id_type {
                    UIntVariant::U8 => {
                        fragment_id = rng.gen::<u8>() as u64;
                        header_size += 1;
                    }
                    UIntVariant::U16 => {
                        fragment_id = rng.gen::<u16>() as u64;
                        header_size += 2;
                    }
                    UIntVariant::U32 => {
                        fragment_id = rng.gen::<u32>() as u64;
                        header_size += 4;
                    }
                    UIntVariant::U64 => {
                        fragment_id = rng.gen::<u64>() as u64;
                        header_size += 8;
                    }
                }

                if !self.send_active_fragments.contains(&fragment_id) {
                    self.send_active_fragments.insert(fragment_id);
                    break;
                }
            }
        }

        for fragment_data in data.chunks(actual_fragment_payload_size) {
            let mut fragment_packet = BytesMut::zeroed(actual_fragment_payload_size + header_size);
            let end_point;
            if fragment_data.len() < actual_fragment_payload_size {
                end_point = fragment_data.len();
            } else {
                end_point = actual_fragment_payload_size;
            }
            fragment_packet[0..end_point].copy_from_slice(fragment_data);

            let final_fragment_portion;
            match self.max_fragment_count {
                UInt::U8(n) => {
                    fragment_packet[actual_fragment_payload_size..actual_fragment_payload_size + 1]
                        .copy_from_slice(&n.to_be_bytes());
                    final_fragment_portion =
                        &mut fragment_packet[actual_fragment_payload_size + 1..];
                }
                UInt::U16(n) => {
                    fragment_packet[actual_fragment_payload_size..actual_fragment_payload_size + 2]
                        .copy_from_slice(&n.to_be_bytes());
                    final_fragment_portion =
                        &mut fragment_packet[actual_fragment_payload_size + 4..];
                }
                UInt::U32(n) => {
                    fragment_packet[actual_fragment_payload_size..actual_fragment_payload_size + 4]
                        .copy_from_slice(&n.to_be_bytes());
                    final_fragment_portion =
                        &mut fragment_packet[actual_fragment_payload_size + 4..];
                }
                UInt::U64(n) => {
                    fragment_packet[actual_fragment_payload_size..actual_fragment_payload_size + 8]
                        .copy_from_slice(&n.to_be_bytes());
                    final_fragment_portion =
                        &mut fragment_packet[actual_fragment_payload_size + 8..];
                }
            }

            match self.fragment_id_type {
                UIntVariant::U8 => {
                    final_fragment_portion.copy_from_slice(&(fragment_id as u8).to_be_bytes())
                }
                UIntVariant::U16 => {
                    final_fragment_portion.copy_from_slice(&(fragment_id as u16).to_be_bytes())
                }
                UIntVariant::U32 => {
                    final_fragment_portion.copy_from_slice(&(fragment_id as u32).to_be_bytes())
                }
                UIntVariant::U64 => {
                    final_fragment_portion.copy_from_slice(&(fragment_id as u64).to_be_bytes())
                }
            }
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
            match self.max_fragment_count {
                UInt::U8(n) => {
                    fragment_index_start = fragment_id_start.saturating_sub(1);
                    let fragment_index_slice = &data[fragment_index_start..fragment_id_start];
                    let fragment_index_array: [u8; 1] = fragment_index_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_index = u8::from_be_bytes(fragment_index_array) as u64;
                    if fragment_index >= n as u64 {
                        return Err(FragmentRecvError::BadFragmentIndex);
                    }
                }
                UInt::U16(n) => {
                    fragment_index_start = fragment_id_start.saturating_sub(2);
                    let fragment_index_slice = &data[fragment_index_start..fragment_id_start];
                    let fragment_index_array: [u8; 2] = fragment_index_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_index = u16::from_be_bytes(fragment_index_array) as u64;
                    if fragment_index >= n as u64 {
                        return Err(FragmentRecvError::BadFragmentIndex);
                    }
                }
                UInt::U32(n) => {
                    fragment_index_start = fragment_id_start.saturating_sub(4);
                    let fragment_index_slice = &data[fragment_index_start..fragment_id_start];
                    let fragment_index_array: [u8; 4] = fragment_index_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_index = u32::from_be_bytes(fragment_index_array) as u64;
                    if fragment_index >= n as u64 {
                        return Err(FragmentRecvError::BadFragmentIndex);
                    }
                }
                UInt::U64(n) => {
                    fragment_index_start = fragment_id_start.saturating_sub(8);
                    let fragment_index_slice = &data[fragment_index_start..fragment_id_start];
                    let fragment_index_array: [u8; 8] = fragment_index_slice
                        .try_into()
                        .map_err(|_| FragmentRecvError::PacketTooSmall)?;
                    fragment_index = u64::from_be_bytes(fragment_index_array) as u64;
                    if fragment_index >= n as u64 {
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
                buffers.push(data);
                self.recv_active_fragments.insert(fragment_id, buffers);
            }
        }
    }
}
