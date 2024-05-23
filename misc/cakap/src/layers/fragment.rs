use std::{hash::BuildHasherDefault, ops::DerefMut};

use bytemuck::{cast_slice, cast_slice_mut};
use bytes::{BufMut, BytesMut};
use fxhash::FxHashMap;
use indexmap::{IndexMap, IndexSet};
use parking_lot::{RwLock, RwLockWriteGuard};
use reed_solomon_erasure::{galois_16, galois_8, Field};

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
    BadFragmentCount,
    BadFragmentData,
    TooManyFragments,
    ReconstructionError(reed_solomon_erasure::Error),
    ForwardError(E),
}

impl<E> From<E> for FragmentRecvError<E> {
    fn from(e: E) -> Self {
        FragmentRecvError::ForwardError(e)
    }
}

enum Fragments {
    Sorted {
        fragments: Vec<BytesMut>,
        fragment_size: u64,
        found_count: u64,
        target_count: u64,
        succeeded: bool,
        padding: u64,
    },
    Unsorted {
        fragments: FxHashMap<u64, BytesMut>,
        fragment_size: u64,
    },
}

pub struct Fragmenter<T> {
    max_fragment_payload_size: UInt,
    redundant_factor: f32,
    max_fragment_count: UInt,
    max_active_fragments: UInt,
    fragment_id_type: UIntVariant,
    send_active_fragments: IndexSet<u64>,
    recv_active_fragments: IndexMap<u64, Fragments>,
    pub forward: T,
}

#[derive(Debug, Clone, Copy)]
pub struct FragmenterBuilder {
    pub max_fragment_size: u64,
    pub redundant_factor: f32,
    pub max_fragment_count: UInt,
    pub max_active_fragment_sets: UInt,
    pub fragment_id_type: UIntVariant,
}

impl Default for FragmenterBuilder {
    fn default() -> Self {
        Self {
            max_fragment_size: 1450,
            redundant_factor: 0.2,
            max_fragment_count: UInt::U16(u16::MAX),
            max_active_fragment_sets: UInt::U16(256),
            fragment_id_type: UIntVariant::U32,
        }
    }
}

impl FragmenterBuilder {
    #[inline(always)]
    pub fn redundant_factor(self, redundant_factor: f32) -> Self {
        Self {
            redundant_factor,
            ..self
        }
    }
    pub fn build<T>(self, forward: T) -> Fragmenter<T> {
        let partial_size = self.max_fragment_size
            - 1
            - self.max_fragment_count.size() as u64 * 2
            - self.fragment_id_type.size() as u64;
        let mut max_fragment_payload_size = UInt::fit_u64(partial_size - 8);
        match max_fragment_payload_size {
            UInt::U8(n) => if n > u8::MAX - 7 {
                max_fragment_payload_size = UInt::U16(n as u16 + 6);
            } else {
                max_fragment_payload_size = UInt::U8(n + 7);
            }
            UInt::U16(n) => if n > u16::MAX - 6 {
                max_fragment_payload_size = UInt::U32(n as u32 + 4);
            } else {
                max_fragment_payload_size = UInt::U16(n + 6);
            }
            UInt::U32(n) => if n > u32::MAX - 4 {
                max_fragment_payload_size = UInt::U64(n as u64);
            } else {
                max_fragment_payload_size = UInt::U32(n + 4);
            }
            UInt::U64(_) => {}
        }
        Fragmenter {
            max_fragment_payload_size,
            redundant_factor: self.redundant_factor,
            max_fragment_count: self.max_fragment_count,
            max_active_fragments: self.max_active_fragment_sets,
            fragment_id_type: self.fragment_id_type,
            send_active_fragments: IndexSet::with_capacity(
                self.max_active_fragment_sets.to_u64().try_into().unwrap(),
            ),
            recv_active_fragments: IndexMap::with_capacity(
                self.max_active_fragment_sets.to_u64().try_into().unwrap(),
            ),
            forward,
        }
    }
}

impl<T> Fragmenter<T> {
    #[inline(always)]
    pub fn builder() -> FragmenterBuilder {
        FragmenterBuilder::default()
    }
}

impl<T> Layer for Fragmenter<T>
where
    T: Layer<SendItem = BytesMut, RecvItem = BytesMut>,
{
    type SendError = FragmentSendError<T::SendError>;
    type RecvError = FragmentRecvError<T::RecvError>;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, mut data: Self::SendItem) -> Result<(), Self::SendError> {
        if data.is_empty() {
            self.forward.send(data).await?;
            return Ok(());
        }

        let mut padding = 0u64;
        if data.len() % 2 == 1 {
            data.put_u8(0);
            padding += 1;
        }

        // Calculate the number of fragments needed to store all the data
        let data_fragment_count =
            (data.len() as u64).div_ceil(self.max_fragment_payload_size.to_u64());

        // The redundant fragments are for error correction
        let redundant_fragment_count =
            (data_fragment_count as f32 * self.redundant_factor).round() as u64;

        let fragment_count = data_fragment_count + redundant_fragment_count;

        let max_fragment_count = self.max_fragment_count.to_u64();
        if fragment_count > max_fragment_count {
            return Err(FragmentSendError::PacketTooBig);
        }

        // The actual size of a fragment's payload can be smaller than the max fragment payload size,
        // reducing the amount of padding needed in the last fragment
        let fragment_payload_size = data.len().div_ceil(data_fragment_count.try_into().unwrap());

        let last_padding = (fragment_payload_size * usize::try_from(data_fragment_count).unwrap()
            - data.len()) as u64;
        padding += last_padding;
        // Pad the data with zeros to make it a multiple of the fragment payload size
        data.put_bytes(0, last_padding.try_into().unwrap());

        // Find a fragment ID that is not already in use
        // We assume that fragment IDs older than max_active_fragments are no longer in use
        let max_active_fragments = self.max_active_fragments.to_u64();
        if self.send_active_fragments.len() as u64 >= max_active_fragments {
            self.send_active_fragments.shift_remove_index(0);
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

        let mut redundant_shards =
            vec![vec![0u8; fragment_payload_size]; redundant_fragment_count.try_into().unwrap()];
        if redundant_fragment_count > 0 {
            let shards: Vec<_> = data.chunks_exact(fragment_payload_size).collect();
            debug_assert_eq!(shards.len() as u64, data_fragment_count);
            let rs_key = (
                data_fragment_count.try_into().unwrap(),
                redundant_fragment_count.try_into().unwrap(),
            );

            if fragment_count < galois_8::Field::ORDER as u64 {
                let mut reader;
                let encoder = 'get: {
                    reader = REED_SOLOMON_ERASURES_8.read();
                    if let Some(encoder) = reader.get(&rs_key) {
                        break 'get encoder;
                    }
                    drop(reader);
                    let mut writer = REED_SOLOMON_ERASURES_8.write();
                    writer.insert(
                        rs_key,
                        galois_8::ReedSolomon::new(rs_key.0, rs_key.1).unwrap(),
                    );
                    reader = RwLockWriteGuard::downgrade(writer);
                    reader.get(&rs_key).unwrap()
                };
                encoder.encode_sep(&shards, &mut redundant_shards).unwrap();
            } else {
                let mut redundant_shards: Vec<&mut [[u8; 2]]> = redundant_shards
                    .iter_mut()
                    .map(|x| cast_slice_mut(x))
                    .collect();
                let shards: Vec<&[[u8; 2]]> = shards.iter().map(|x| cast_slice(x)).collect();
                let mut reader;
                let encoder = 'get: {
                    reader = REED_SOLOMON_ERASURES_16.read();
                    if let Some(encoder) = reader.get(&rs_key) {
                        break 'get encoder;
                    }
                    drop(reader);
                    let mut writer = REED_SOLOMON_ERASURES_16.write();
                    writer.insert(
                        rs_key,
                        galois_16::ReedSolomon::new(rs_key.0, rs_key.1).unwrap(),
                    );
                    reader = RwLockWriteGuard::downgrade(writer);
                    reader.get(&rs_key).unwrap()
                };
                encoder.encode_sep(&shards, &mut redundant_shards).unwrap();
            }
        }

        // A Header depends on the type of packet
        // A complete packet (255) has the padding size, fragments count, fragment index, and fragment ID
        // A partial packet (0) has a fragment index, and fragment ID
        // The last byte in each header is the type of the packet

        // The number of complete packets is the number of redundant fragments plus one
        // This makes it impossible to have the minimum number of packets for reconstruction
        // but not the fragment count (the receiver will not be able to know if it has the
        // minimum number of packets without the fragment count)
        let complete_packet_count = redundant_fragment_count + 1;

        // The first complete_packet_count fragments are complete packets
        for (i, fragment_data) in data
            .chunks(fragment_payload_size)
            .chain(redundant_shards.iter().map(|x| x.as_slice()))
            .enumerate()
        {
            let header_size = if (i as u64) < complete_packet_count {
                1 + self.max_fragment_count.size() * 2
                    + self.fragment_id_type.size()
                    + self.max_fragment_payload_size.size()
            } else {
                1 + self.max_fragment_count.size() + self.fragment_id_type.size()
            };
            let mut fragment_packet = BytesMut::with_capacity(fragment_payload_size + header_size);
            fragment_packet.extend_from_slice(fragment_data);

            if (i as u64) < complete_packet_count {
                self.max_fragment_payload_size
                    .with_u64(padding)
                    .extend_bytes_mut(&mut fragment_packet);
                self.max_fragment_count
                    .with_u64(fragment_count)
                    .extend_bytes_mut(&mut fragment_packet);
            }

            self.max_fragment_count
                .with_u64(i as u64)
                .extend_bytes_mut(&mut fragment_packet);
            self.fragment_id_type
                .with_u64(fragment_id)
                .extend_bytes_mut(&mut fragment_packet);

            if (i as u64) < complete_packet_count {
                fragment_packet.put_u8(255);
            } else {
                fragment_packet.put_u8(0);
            }

            self.forward.send(fragment_packet).await?;
        }

        Ok(())
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        let (mut fragments, fragment_size, target_count, padding, succeeded) = loop {
            let mut data = self.forward.recv().await?;

            let Some(packet_type) = data.last().copied() else {
                return Ok(data);
            };
            data.truncate(data.len() - 1);

            if packet_type >= 127 {
                if data.len()
                    < 1 + self.max_fragment_count.size() * 2
                        + self.fragment_id_type.size()
                        + self.max_fragment_payload_size.size()
                {
                    return Err(FragmentRecvError::PacketTooSmall);
                }
            } else if data.len() < 1 + self.max_fragment_count.size() + self.fragment_id_type.size()
            {
                return Err(FragmentRecvError::PacketTooSmall);
            }

            let fragment_id_slice = data.split_off(data.len() - self.fragment_id_type.size());
            let fragment_id = self
                .fragment_id_type
                .try_with_slice(&fragment_id_slice)
                .unwrap()
                .to_u64();
            let fragment_index_slice = data.split_off(data.len() - self.max_fragment_count.size());
            let fragment_index = self
                .max_fragment_count
                .try_with_slice(&fragment_index_slice)
                .unwrap()
                .to_u64();

            let fragment_set = if let Some(x) = self.recv_active_fragments.get_mut(&fragment_id) {
                x
            } else if data.is_empty() {
                return Err(FragmentRecvError::BadFragmentData);
            } else {
                if self.recv_active_fragments.len() as u64 >= self.max_active_fragments.to_u64() {
                    self.recv_active_fragments.shift_remove_index(0);
                }
                let fragment_size = if packet_type >= 127 {
                    (data.len()
                        - self.max_fragment_count.size()
                        - self.max_fragment_payload_size.size()) as u64
                } else {
                    data.len() as u64
                };
                self.recv_active_fragments
                    .entry(fragment_id)
                    .or_insert(Fragments::Unsorted {
                        fragments: FxHashMap::default(),
                        fragment_size,
                    })
            };

            if packet_type >= 127 {
                let fragment_count_slice =
                    data.split_off(data.len() - self.max_fragment_count.size());
                let fragment_count = self
                    .max_fragment_count
                    .try_with_slice(&fragment_count_slice)
                    .unwrap()
                    .to_u64();
                if fragment_count == 0 {
                    self.recv_active_fragments.shift_remove(&fragment_id);
                    return Err(FragmentRecvError::BadFragmentCount);
                }
                let padding_slice =
                    data.split_off(data.len() - self.max_fragment_payload_size.size());
                let padding = self
                    .max_fragment_payload_size
                    .try_with_slice(&padding_slice)
                    .unwrap()
                    .to_u64();
                match fragment_set {
                    Fragments::Sorted { fragments, .. } => {
                        if fragments.len() as u64 != fragment_count {
                            self.recv_active_fragments.shift_remove(&fragment_id);
                            return Err(FragmentRecvError::BadFragmentCount);
                        }
                    }
                    Fragments::Unsorted {
                        fragment_size,
                        fragments,
                    } => {
                        let fragment_size = *fragment_size;
                        let found_count = fragments.len() as u64;
                        let mut new_fragments =
                            Vec::with_capacity(fragment_count.try_into().unwrap());
                        for _ in 0..fragment_count {
                            new_fragments.push(BytesMut::new());
                        }
                        let Fragments::Unsorted { fragments, .. } = std::mem::replace(
                            fragment_set,
                            Fragments::Sorted {
                                fragments: new_fragments,
                                fragment_size,
                                found_count,
                                padding,
                                target_count: (fragment_count as f32
                                    / (1.0 + self.redundant_factor))
                                    .round() as u64,
                                succeeded: false,
                            },
                        ) else {
                            unreachable!();
                        };
                        let Fragments::Sorted {
                            fragments: new_fragments,
                            ..
                        } = fragment_set
                        else {
                            unreachable!();
                        };
                        for (i, fragment) in fragments {
                            if i >= fragment_count {
                                self.recv_active_fragments.shift_remove(&fragment_id);
                                return Err(FragmentRecvError::BadFragmentCount);
                            }
                            new_fragments[i as usize] = fragment;
                        }
                    }
                }
            }

            match fragment_set {
                Fragments::Sorted {
                    fragments,
                    fragment_size,
                    found_count,
                    target_count,
                    succeeded,
                    padding,
                } => {
                    if *fragment_size != data.len() as u64 {
                        self.recv_active_fragments.shift_remove(&fragment_id);
                        return Err(FragmentRecvError::BadFragmentData);
                    }
                    if fragment_index as usize >= fragments.len() {
                        self.recv_active_fragments.shift_remove(&fragment_id);
                        return Err(FragmentRecvError::BadFragmentIndex);
                    }
                    if fragments[fragment_index as usize].is_empty() {
                        fragments[fragment_index as usize] = data;
                    } else {
                        self.recv_active_fragments.shift_remove(&fragment_id);
                        return Err(FragmentRecvError::DuplicateFragment);
                    }

                    *found_count += 1;
                    if found_count >= target_count && !*succeeded {
                        break (
                            std::mem::take(fragments),
                            *fragment_size,
                            *target_count,
                            *padding,
                            succeeded,
                        );
                    }
                }
                Fragments::Unsorted {
                    fragments,
                    fragment_size,
                } => {
                    if *fragment_size != data.len() as u64 {
                        self.recv_active_fragments.shift_remove(&fragment_id);
                        return Err(FragmentRecvError::BadFragmentData);
                    }
                    if fragments.insert(fragment_index, data).is_some() {
                        self.recv_active_fragments.shift_remove(&fragment_id);
                        return Err(FragmentRecvError::DuplicateFragment);
                    }
                    if fragments.len() as u64
                        >= (self.max_fragment_count.to_u64() as f32 * self.redundant_factor).round()
                            as u64
                            + 1
                    {
                        return Err(FragmentRecvError::TooManyFragments);
                    }
                }
            }
        };

        let redundant_fragment_count =
            (target_count as f32 * self.redundant_factor).round() as usize;

        let rs_key = (
            usize::try_from(target_count).unwrap(),
            redundant_fragment_count,
        );
        if fragments.len() < galois_8::Field::ORDER {
            let mut fragments: Vec<_> = fragments
                .iter_mut()
                .map(|b| {
                    if b.is_empty() {
                        b.put_bytes(0, fragment_size.try_into().unwrap());
                        (b.deref_mut(), false)
                    } else {
                        debug_assert_eq!(b.len() as u64, fragment_size);
                        (b.deref_mut(), true)
                    }
                })
                .collect();
            let mut reader;
            let encoder = 'get: {
                reader = REED_SOLOMON_ERASURES_8.read();
                if let Some(encoder) = reader.get(&rs_key) {
                    break 'get encoder;
                }
                drop(reader);
                let mut writer = REED_SOLOMON_ERASURES_8.write();
                writer.insert(
                    rs_key,
                    galois_8::ReedSolomon::new(rs_key.0, rs_key.1).unwrap(),
                );
                reader = RwLockWriteGuard::downgrade(writer);
                reader.get(&rs_key).unwrap()
            };
            encoder
                .reconstruct_data(&mut fragments)
                .map_err(FragmentRecvError::ReconstructionError)?;
        } else {
            let mut fragments: Vec<(&mut [[u8; 2]], bool)> = fragments
                .iter_mut()
                .map(|b| {
                    if b.is_empty() {
                        b.put_bytes(0, fragment_size.try_into().unwrap());
                        (cast_slice_mut(b.deref_mut()), false)
                    } else {
                        debug_assert_eq!(b.len() as u64, fragment_size);
                        (cast_slice_mut(b.deref_mut()), true)
                    }
                })
                .collect();

            let mut reader;
            let encoder = 'get: {
                reader = REED_SOLOMON_ERASURES_16.read();
                if let Some(encoder) = reader.get(&rs_key) {
                    break 'get encoder;
                }
                drop(reader);
                let mut writer = REED_SOLOMON_ERASURES_16.write();
                writer.insert(
                    rs_key,
                    galois_16::ReedSolomon::new(rs_key.0, rs_key.1).unwrap(),
                );
                reader = RwLockWriteGuard::downgrade(writer);
                reader.get(&rs_key).unwrap()
            };
            encoder
                .reconstruct_data(&mut fragments)
                .map_err(FragmentRecvError::ReconstructionError)?;
        }

        *succeeded = true;
        let mut iter = fragments.into_iter().take(target_count as usize);
        let mut data = iter.next().unwrap();
        for fragment in iter {
            data.extend_from_slice(&fragment);
        }
        data.truncate((data.len() as u64 - padding) as usize);
        Ok(data)
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        self.forward.get_max_packet_size()
    }
}
