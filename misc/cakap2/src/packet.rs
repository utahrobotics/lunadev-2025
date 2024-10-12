use std::{
    fmt::Debug,
    num::NonZeroU64,
    ops::{Deref, DerefMut},
    sync::{atomic::Ordering, Arc},
};

use crate::Shared;

struct SendVoid(*mut ());
unsafe impl Send for SendVoid {}
struct SendSlice(*mut [u8]);
unsafe impl Send for SendSlice {}

/// A data structure that allows for borrowing an array of bytes and returning it when it is no longer needed.
pub(crate) struct BorrowedBytes {
    buffer: SendVoid,
    return_fn: Box<dyn FnOnce(*mut ()) + Send>,
    pointer: SendSlice,
}

impl BorrowedBytes {
    /// Create a new `BorrowedBytes` from a buffer and a function that will be called when the buffer is no longer needed.
    pub(crate) fn new<F, T>(buffer: T, done: F) -> Self
    where
        T: AsMut<[u8]> + Send + 'static,
        F: FnOnce(T) + Send + 'static,
    {
        let buffer = Box::leak(Box::new(buffer));
        let pointer = buffer.as_mut();
        Self {
            pointer: SendSlice(pointer),
            return_fn: Box::new(|void| unsafe {
                let value = Box::from_raw(void.cast::<T>());
                let mut value = *value;
                value.as_mut().iter_mut().for_each(|x| *x = 0);

                done(value)
            }),
            buffer: SendVoid((buffer as *mut T).cast()),
        }
    }
}

impl Drop for BorrowedBytes {
    fn drop(&mut self) {
        if !self.buffer.0.is_null() {
            let buffer = std::mem::replace(&mut self.buffer.0, std::ptr::null_mut());
            let return_fn = std::mem::replace(&mut self.return_fn, Box::new(|_| {}));
            return_fn(buffer);
            // Not necessary, but good for peace of mind
            self.pointer = SendSlice(&mut []);
        }
    }
}

impl Deref for BorrowedBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.pointer.0 }
    }
}

impl DerefMut for BorrowedBytes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.pointer.0 }
    }
}

impl PartialEq for BorrowedBytes {
    fn eq(&self, other: &Self) -> bool {
        self.deref() == other.deref()
    }
}

impl Eq for BorrowedBytes {}

pub(crate) enum OutgoingDataInner {
    /// This packet of data will be transmitted repeatedly until the client acknowledges.
    Reliable {
        index: NonZeroU64,
        data: BorrowedBytes,
    },
    CancelReliable(ReliableIndex),
    CancelAllReliable,
    // /// The newest packet of data with some `id` will be transmitted repeatedly until
    // /// the client acknowledges.
    // ///
    // /// If some packet was sent as `EventuallyReliable` with `id` of 0, that
    // /// packet will be transmitted repeatedly. If a new packet with the same `id` is sent
    // /// before the client acknouwledges the old one, the new one will be sent instead.
    // EventuallyReliable { last_id: usize, data: BorrowedBytes },
    /// This packet of data will be transmitted only once.
    Unreliable(BorrowedBytes),
}

/// A packet intended to be sent to the peer.
/// 
/// Before sending, the packet should be digested by the state machine,
/// which will determine the next steps.
pub struct OutgoingData {
    pub(crate) inner: OutgoingDataInner,
}

impl OutgoingData {
    /// Cancel the reliable packet with the given index.
    /// 
    /// This prevents the packet from being retransmitted,
    /// but it does not guarantee that the packet will not be received
    /// if it is already in transit. Cancelling an already acknowledged
    /// packet will have no effect.
    pub fn cancel_reliable(index: ReliableIndex) -> Self {
        OutgoingData {
            inner: OutgoingDataInner::CancelReliable(index),
        }
    }
    /// Cancel all reliable packets.
    /// 
    /// This has the same guarantees as [`Self::cancel_reliable`].
    pub fn cancel_all_reliable() -> Self {
        OutgoingData {
            inner: OutgoingDataInner::CancelAllReliable,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ReliableIndex(pub(crate) NonZeroU64);

/// Used to create reliable and unreliable packets.
#[derive(Clone)]
pub struct PacketBuilder {
    pub(crate) shared: Arc<Shared>,
}

impl PacketBuilder {
    /// Sends the given bytes unreliably.
    ///
    /// The last 8 bytes of the given message will be overwritten with zeroes, so leave space for that.
    /// If the given bytes are shorter than 9, the bytes will be returned. This means packets cannot
    /// have a zero-sized payload.
    pub fn new_unreliable<T, F>(&self, mut bytes: T, done: F) -> Result<OutgoingData, T>
    where
        T: AsMut<[u8]> + Send + 'static,
        F: FnOnce(T) + Send + 'static,
    {
        let bytes_mut = bytes.as_mut();
        let len = bytes_mut.len();

        if len < 9 || len > self.shared.max_packet_size {
            return Err(bytes);
        }
        bytes_mut[len - 8..].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);

        Ok(OutgoingData {
            inner: OutgoingDataInner::Unreliable(BorrowedBytes::new(bytes, done)),
        })
    }

    /// Sends the given bytes reliably.
    ///
    /// The last 8 bytes of the given message will be overwritten with a reliable index, so leave space for that.
    /// If the given bytes are shorter than 9, the bytes will be returned. This means packets cannot
    /// have a zero-sized payload.
    ///
    /// # Safety
    /// Strictly speaking, unexpected behavior can occur if this method is called 2^63 - 1 times per struct due to overflow.
    /// However, this is hopefully not a practical concern.
    pub fn new_reliable<T, F>(
        &self,
        mut bytes: T,
        done: F,
    ) -> Result<(OutgoingData, ReliableIndex), T>
    where
        T: AsMut<[u8]> + Send + 'static,
        F: FnOnce(T) + Send + 'static,
    {
        let bytes_mut = bytes.as_mut();
        let len = bytes_mut.len();

        if len < 9 || len > self.shared.max_packet_size {
            return Err(bytes);
        }

        let reliable_index = self.shared.reliable_index.fetch_add(1, Ordering::Relaxed);

        bytes_mut[len - 8..].copy_from_slice(&reliable_index.to_be_bytes());
        let reliable_index = NonZeroU64::new(reliable_index).expect("Reliable Index has overflowed. Consider reconstructing the state machine earlier to avoid this");

        Ok((
            OutgoingData {
                inner: OutgoingDataInner::Reliable {
                    data: BorrowedBytes::new(bytes, done),
                    index: reliable_index,
                },
            },
            ReliableIndex(reliable_index),
        ))
    }
}

pub(crate) enum HotPacketInner<'a> {
    Borrowed(&'a BorrowedBytes),
    Owned(BorrowedBytes),
    Index([u8; 8]),
}

/// A packet of data that the state machine has determined needs to be sent immediately.
///
/// [`OutgoingData`] represents an intent to send data, that the state machine digests.
/// The state machine then produces [`HotPacket`]s at the appropriate time. When the packet
/// has been sent, the state machine should be notified.
pub struct HotPacket<'a> {
    pub(crate) inner: HotPacketInner<'a>,
}

impl<'a> Deref for HotPacket<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match &self.inner {
            HotPacketInner::Borrowed(buf) => buf,
            HotPacketInner::Owned(buf) => buf,
            HotPacketInner::Index(buf) => buf,
        }
    }
}

impl<'a> PartialEq for HotPacket<'a> {
    fn eq(&self, other: &Self) -> bool {
        let self_bytes = match &self.inner {
            HotPacketInner::Borrowed(buf) => *buf,
            HotPacketInner::Owned(buf) => buf.deref(),
            HotPacketInner::Index(buf) => buf,
        };
        let other_bytes = match &other.inner {
            HotPacketInner::Borrowed(buf) => *buf,
            HotPacketInner::Owned(buf) => buf.deref(),
            HotPacketInner::Index(buf) => buf,
        };
        self_bytes == other_bytes
    }
}

impl<'a> Eq for HotPacket<'a> {}

impl<'a> Debug for HotPacket<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotPacket")
            .field("data", &self.deref())
            .finish()
    }
}
