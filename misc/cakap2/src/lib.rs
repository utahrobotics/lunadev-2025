use std::{
    collections::VecDeque,
    fmt::Display,
    num::NonZeroU64,
    sync::{atomic::AtomicU64, Arc},
    time::{Duration, Instant},
};

use fxhash::FxHashMap;
use indexmap::IndexSet;
use packet::{
    BorrowedBytes, HotPacket, HotPacketInner, OutgoingData, OutgoingDataInner, ReliableBuilder,
    ReliableIndex,
};

// mod collections;
pub mod packet;

pub struct Shared {
    reliable_index: AtomicU64,
    max_packet_size: usize,
}

struct Retransmit {
    send_at: Instant,
    data: BorrowedBytes,
}

pub struct PeerStateMachine {
    shared: Arc<Shared>,
    retransmission_duration: Duration,
    retransmission_map: FxHashMap<NonZeroU64, Retransmit>,
    retransmission_queue: VecDeque<NonZeroU64>,
    received_set: IndexSet<NonZeroU64>,
    max_received_set_size: usize,
}

impl Default for PeerStateMachine {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                reliable_index: AtomicU64::new(1),
                max_packet_size: 1400,
            }),
            retransmission_duration: Default::default(),
            retransmission_map: Default::default(),
            retransmission_queue: Default::default(),
            received_set: Default::default(),
            max_received_set_size: 256,
        }
    }
}

impl PeerStateMachine {
    pub fn get_reliable_builder(&self) -> ReliableBuilder {
        ReliableBuilder {
            shared: self.shared.clone(),
        }
    }

    pub fn poll<'a, 'b>(&'a mut self, event: Event<'b>, now: Instant) -> RecommendedAction<'a, 'b> {
        match event {
            Event::IncomingData(data) => {
                if data.len() < 8 {
                    return RecommendedAction::HandleError(Error::PacketTooSmall);
                }

                let index = u64::from_le_bytes(data[data.len() - 8..].try_into().unwrap());

                if let Some(index) = NonZeroU64::new(index) {
                    let msb = index.get() >> 63;
                    if msb == 0 {
                        // New packet from peer
                        if self.received_set.insert(index) {
                            let new_index = index | (1 << 63);

                            return RecommendedAction::HandleDataAndSend {
                                received: &data[0..data.len() - 8],
                                to_send: new_index.get().to_be_bytes(),
                            };
                        } else if self.received_set.len() > self.max_received_set_size {
                            self.received_set.shift_remove_index(0);
                        }
                    } else {
                        // Acknowledgement from peer
                        let true_index = index.get() & !(1 << 63);
                        let Some(true_index) = NonZeroU64::new(true_index) else {
                            return RecommendedAction::HandleError(Error::InvalidPacket);
                        };
                        self.retransmission_map.remove(&true_index);
                    }
                } else {
                    // Unreliable packet from peer
                    return RecommendedAction::HandleData(&data[0..data.len() - 8]);
                }
            }
            Event::DataToSend(outgoing_data) => match outgoing_data.inner {
                OutgoingDataInner::Reliable { data, index } => {
                    let option = self.retransmission_map.insert(
                        index,
                        Retransmit {
                            send_at: now + self.retransmission_duration,
                            data,
                        },
                    );
                    debug_assert!(option.is_none());
                    self.retransmission_queue.push_back(index);
                }
                OutgoingDataInner::CancelAllReliable => {
                    self.retransmission_map.clear();
                    self.retransmission_queue.clear();
                }
                OutgoingDataInner::CancelReliable(ReliableIndex(index)) => {
                    self.retransmission_map.remove(&index);
                }
                OutgoingDataInner::Unreliable(borrowed_bytes) => {
                    return RecommendedAction::SendData(HotPacket {
                        inner: HotPacketInner::Owned(borrowed_bytes),
                    })
                }
            },
            _ => {}
        }
        loop {
            let Some(&first_index) = self.retransmission_queue.front() else {
                break RecommendedAction::WaitForData;
            };
            let Some(retransmit) = self.retransmission_map.get_mut(&first_index) else {
                self.retransmission_queue.pop_front();
                continue;
            };
            if retransmit.send_at <= now {
                self.retransmission_queue.pop_front();
                self.retransmission_queue.push_back(first_index);
                retransmit.send_at = now + self.retransmission_duration;
                // To please the borrow checker
                let retransmit = self.retransmission_map.get(&first_index).unwrap();
                break RecommendedAction::SendData(HotPacket {
                    inner: HotPacketInner::Borrowed(&retransmit.data),
                });
            } else {
                break RecommendedAction::WaitForDuration(retransmit.send_at - now);
            }
        }
    }
}

#[derive(Debug)]
pub enum Error {
    /// A packet from the peer was too small to be processed.
    PacketTooSmall,
    /// A packet from the peer was too large to be processed.
    PacketTooLong,
    /// A packet from the peer was invalid.
    InvalidPacket,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PacketTooSmall => write!(f, "Packet from peer was too small to be processed"),
            Self::PacketTooLong => write!(f, "Packet from peer was too large to be processed"),
            Self::InvalidPacket => write!(f, "Packet from peer was invalid"),
        }
    }
}

impl std::error::Error for Error {}

pub enum RecommendedAction<'a, 'b> {
    /// Wait indefinitely until data from the peer is received, or there is data to send.
    WaitForData,
    /// Wait at most the given [`Duration`] for data from the peer, or data to be sent.
    ///
    /// If the given duration is 5 seconds, the event loop should still poll the state
    /// machine if there is data to be sent, or data was received from the peer. However,
    /// if 5 seconds have passed and neither event occurs, the state machine should still
    /// be polled anyway.
    WaitForDuration(Duration),
    /// Handle the given error (by logging or otherwise) and poll the state machine again
    /// with `NoEvent`.
    HandleError(Error),
    /// Handle the given data from the peer.
    HandleData(&'b [u8]),
    /// Handle `received` from the peer, and send `to_send` to the peer.
    HandleDataAndSend {
        received: &'b [u8],
        to_send: [u8; 8],
    },
    /// Send the given data to the peer.
    SendData(HotPacket<'a>),
}

pub enum Event<'a> {
    /// A whole packet of data, with no padding bytes or otherwise empty space.
    IncomingData(&'a [u8]),
    /// Data to be sent to the connected peer.
    DataToSend(OutgoingData),
    /// A [`HotPacket`] was confirmed to be sent.
    HotPacketSent,
    /// No data received, to be sent, or was sent. Usually used when some duration of time has passed,
    /// or after an error was handled.
    NoEvent,
}

impl<'a> Default for Event<'a> {
    fn default() -> Self {
        Self::NoEvent
    }
}

impl<'a> From<OutgoingData> for Event<'a> {
    fn from(value: OutgoingData) -> Self {
        Self::DataToSend(value)
    }
}
