use std::{
    collections::BinaryHeap,
    future::Future,
    hash::{DefaultHasher, Hash, Hasher},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};

use crossbeam::{atomic::AtomicCell, queue::ArrayQueue};
use fxhash::FxHashSet;
use log::error;
use tasker::{callbacks::callee::Subscriber, define_callbacks, fn_alias, task::AsyncTask};
use tokio::{
    net::UdpSocket,
    sync::mpsc,
    time::{sleep_until, Instant},
};

const MIN_PORT: u16 = 10000;
define_callbacks!(pub BytesCallbacks => Fn(bytes: &[u8]) + Send + Sync);
fn_alias!(pub type BytesCallbacksRef = CallbacksRef(&[u8]) + Send + Sync);

struct CakapSocketShared {
    noreply_socket: UdpSocket,
    reliable_packet_sub: Subscriber<ReliablePacket>,
    send_to_addr: AtomicCell<Option<SocketAddr>>,
    max_packet_size: AtomicUsize,
}

pub struct CakapSocket {
    reply_socket: UdpSocket,
    ack_socket: UdpSocket,
    pub retransmission_duration: Duration,
    pub recycled_byte_vec_size: usize,
    bytes_callbacks: BytesCallbacks,
    recycled_byte_vecs: RecycledByteVecs,
    shared: Arc<CakapSocketShared>,
    existing_stream_tx: mpsc::Sender<()>,
    existing_stream_rx: mpsc::Receiver<()>,
}

impl CakapSocket {
    pub async fn bind(port: u16) -> std::io::Result<Self> {
        let reply_socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).await?;
        let mut next_port = reply_socket.local_addr()?.port().wrapping_add(1);
        if next_port == 0 {
            next_port = MIN_PORT;
        }
        let noreply_socket =
            UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, next_port)).await?;
        next_port = next_port.wrapping_add(1);
        if next_port == 0 {
            next_port = MIN_PORT;
        }
        let ack_socket =
            UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, next_port)).await?;
        let (existing_stream_tx, existing_stream_rx) = mpsc::channel(1);
        Ok(Self {
            reply_socket,
            ack_socket,
            retransmission_duration: Duration::from_millis(50),
            recycled_byte_vec_size: 0,
            bytes_callbacks: BytesCallbacks::default(),
            recycled_byte_vecs: RecycledByteVecs {
                queue: Arc::new(OnceLock::new()),
            },
            shared: Arc::new(CakapSocketShared {
                noreply_socket,
                reliable_packet_sub: Subscriber::new_unbounded(),
                send_to_addr: AtomicCell::new(None),
                max_packet_size: AtomicUsize::new(1400),
            }),
            existing_stream_tx,
            existing_stream_rx,
        })
    }

    pub fn get_recycled_byte_vecs(&self) -> RecycledByteVecs {
        self.recycled_byte_vecs.clone()
    }

    pub fn spawn_looping(self) {
        self.spawn_with(|result| {
            let (socket, e) = result.unwrap_err();
            log::error!("Error in CakapSocket: {}", e);
            socket.spawn_looping();
        });
    }

    pub fn get_stream(&self) -> CakapSender {
        CakapSender {
            shared: self.shared.clone(),
            recycled_byte_vecs: self.get_recycled_byte_vecs(),
            _existing_stream_tx: self.existing_stream_tx.clone(),
        }
    }

    pub fn get_bytes_callback_ref(&self) -> BytesCallbacksRef {
        self.bytes_callbacks.get_ref()
    }

    pub fn set_max_packet_size(&self, size: usize) {
        self.shared.max_packet_size.store(size, Ordering::Relaxed);
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.reply_socket.local_addr()
    }
}

impl AsyncTask for CakapSocket {
    type Output = Result<(), (Self, std::io::Error)>;

    async fn run(mut self) -> Self::Output {
        let max_packet_size = self.shared.max_packet_size.load(Ordering::Relaxed);
        let mut reply_buf = vec![0u8; max_packet_size];
        let mut noreply_buf = vec![0u8; max_packet_size];
        let mut ack_buf = [0u8; 8];
        let mut retransmit_acks: FxHashSet<[u8; 8]> = FxHashSet::default();
        let mut retransmission_queue: BinaryHeap<PacketToRetransmit> = BinaryHeap::new();
        let get_send_to_addr = || self.shared.send_to_addr.load();

        loop {
            tokio::select! {
                result = self.reply_socket.recv_from(&mut reply_buf) => {
                    let (len, addr) = match result {
                        Ok(x) => x,
                        Err(e) => break Err((self, e)),
                    };
                    self.shared.send_to_addr.store(Some(addr));
                    let payload = &reply_buf[..len];
                    self.bytes_callbacks.call(payload);

                    let ack = ReliablePacket::create_ack(payload);
                    if let Err(e) = self.reply_socket.send_to(&ack, addr).await {
                        break Err((self, e));
                    }
                }
                result = self.ack_socket.recv_from(&mut ack_buf) => {
                    let (len, addr) = match result {
                        Ok(x) => x,
                        Err(e) => break Err((self, e)),
                    };
                    self.shared.send_to_addr.store(Some(addr));
                    if len != 8 {
                        error!("Received ack of invalid len: {len}");
                        continue;
                    }
                    retransmit_acks.remove(&reply_buf[..8]);
                }
                result = self.shared.noreply_socket.recv_from(&mut noreply_buf) => {
                    let (len, addr) = match result {
                        Ok(x) => x,
                        Err(e) => break Err((self, e)),
                    };
                    self.shared.send_to_addr.store(Some(addr));
                    self.bytes_callbacks.call(&noreply_buf[..len]);
                }
                res = self.existing_stream_rx.recv() => {
                    debug_assert!(res.is_none());
                    break Ok(());
                }
                e = async {
                    loop {
                        if let Some(retransmit) = retransmission_queue.peek() {
                            if retransmit_acks.contains(&retransmit.ack) {
                                sleep_until(retransmit.instant).await;
                                let Some(addr) = get_send_to_addr() else {
                                    error!("No address to send to");
                                    continue;
                                };

                                if let Err(e) = self.reply_socket.send_to(&retransmit.payload, addr).await {
                                    break e;
                                }
                            } else {
                                let PacketToRetransmit { payload, .. } = retransmission_queue.pop().unwrap();
                                self.recycled_byte_vecs.recycle_vec(payload);
                            }
                        } else {
                            std::future::pending().await
                        }
                    }
                } => break Err((self, e)),
                packet = async {
                    loop {
                        if let Some(packet) = self.shared.reliable_packet_sub.recv().await {
                            break packet;
                        } else {
                            tokio::time::sleep(self.retransmission_duration).await;
                        }
                    }
                } => {
                    match packet {
                        ReliablePacket::New { payload, ack } => {
                            retransmit_acks.insert(ack);
                            let Some(addr) = get_send_to_addr() else {
                                error!("No address to send to");
                                continue;
                            };

                            if let Err(e) = self.reply_socket.send_to(&payload, addr).await {
                                break Err((self, e));
                            }
                            retransmission_queue.push(PacketToRetransmit {
                                instant: Instant::now() + self.retransmission_duration,
                                payload,
                                ack
                            });
                        }
                        ReliablePacket::Replace { payload, new_ack, old_ack } => {
                            retransmit_acks.remove(&old_ack);
                            retransmit_acks.insert(new_ack);
                            let Some(addr) = get_send_to_addr() else {
                                error!("No address to send to");
                                continue;
                            };
                            if let Err(e) = self.reply_socket.send_to(&payload, addr).await {
                                break Err((self, e));
                            }
                            retransmission_queue.push(PacketToRetransmit {
                                instant: Instant::now() + self.retransmission_duration,
                                payload,
                                ack: new_ack
                            });
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct RecycledByteVecs {
    queue: Arc<OnceLock<ArrayQueue<Vec<u8>>>>,
}

impl RecycledByteVecs {
    pub fn get_vec(&self) -> Vec<u8> {
        self.queue
            .get()
            .map(|queue| queue.pop().unwrap_or_default())
            .unwrap_or_default()
    }

    pub fn recycle_vec(&self, mut vec: Vec<u8>) {
        vec.clear();
        self.queue.get().map(|queue| queue.push(vec));
    }
}

struct PacketToRetransmit {
    instant: Instant,
    payload: Vec<u8>,
    ack: [u8; 8],
}

impl Ord for PacketToRetransmit {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.instant.cmp(&self.instant)
    }
}

impl PartialOrd for PacketToRetransmit {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for PacketToRetransmit {
    fn eq(&self, other: &Self) -> bool {
        self.instant == other.instant
    }
}

impl Eq for PacketToRetransmit {}

enum ReliablePacket {
    New {
        payload: Vec<u8>,
        ack: [u8; 8],
    },
    Replace {
        payload: Vec<u8>,
        new_ack: [u8; 8],
        old_ack: [u8; 8],
    },
}

impl ReliablePacket {
    fn create_ack(data: &[u8]) -> [u8; 8] {
        if data.len() <= 8 {
            let mut ack = [0u8; 8];
            ack[..data.len()].copy_from_slice(data);
            return ack;
        }
        let mut hasher = DefaultHasher::default();
        data.hash(&mut hasher);
        hasher.finish().to_ne_bytes()
    }

    fn new(payload: Vec<u8>) -> Self {
        Self::New {
            ack: Self::create_ack(&payload),
            payload,
        }
    }

    fn replace(payload: Vec<u8>, old_ack: [u8; 8]) -> Self {
        Self::Replace {
            new_ack: Self::create_ack(&payload),
            payload,
            old_ack,
        }
    }

    fn get_ack(&self) -> [u8; 8] {
        match self {
            Self::New { ack, .. } => *ack,
            Self::Replace { new_ack, .. } => *new_ack,
        }
    }
}

#[derive(Clone)]
pub struct CakapSender {
    shared: Arc<CakapSocketShared>,
    recycled_byte_vecs: RecycledByteVecs,
    _existing_stream_tx: mpsc::Sender<()>,
}

impl std::fmt::Debug for CakapSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CakapSender")
            .field(
                "max_packet_size",
                &self.shared.max_packet_size.load(Ordering::Relaxed),
            )
            .field("send_to_addr", &self.shared.send_to_addr.load())
            .field(
                "recycled_byte_vecs",
                &self.recycled_byte_vecs.queue.get().map(|x| x.len()),
            )
            .finish()
    }
}

impl CakapSender {
    /// Send a payload with no guarantees of delivery and ordering.
    ///
    /// The returned future may be awaited on to efficiently send the payload using the current
    /// async runtime. If the future is dropped, the payload will be send to a separate task
    /// to be sent. This involves copying the payload and may incur a heap allocation if there is
    /// not a recycled byte vec available.
    ///
    /// This function does not need to be called from within an async context. One will be made
    /// if necessary.
    pub fn send_unreliable<'a>(&'a self, payload: &'a [u8]) -> impl Future<Output = ()> + 'a {
        let guard = SendUnreliable::Slice {
            stream: self,
            slice: &payload,
        };
        async {
            let Some(mut addr) = self.shared.send_to_addr.load() else {
                error!("No address to send to");
                std::mem::forget(guard);
                return;
            };
            if payload.len() > self.shared.max_packet_size.load(Ordering::Relaxed) {
                error!("Payload too large to send");
                std::mem::forget(guard);
                return;
            }
            let new_port = addr.port().wrapping_add(1);
            if new_port == 0 {
                addr.set_port(MIN_PORT);
            } else {
                addr.set_port(new_port);
            }
            let _ = self.shared.noreply_socket.send_to(payload, addr).await;
            std::mem::forget(guard);
        }
    }

    /// Send a payload with no guarantees of delivery and ordering.
    ///
    /// The returned future may be awaited on to efficiently send the payload using the current
    /// async runtime. If the future is dropped, the payload will be send to a separate task
    /// to be sent.
    ///
    /// This function does not need to be called from within an async context. One will be made
    /// if necessary.
    pub fn send_unreliable_vec<'a>(&'a self, payload: Vec<u8>) -> impl Future<Output = ()> + 'a {
        let guard = SendUnreliable::Vec {
            stream: self,
            vec: payload,
        };
        async {
            let Some(mut addr) = self.shared.send_to_addr.load() else {
                error!("No address to send to");
                std::mem::forget(guard);
                return;
            };
            let SendUnreliable::Vec { vec, .. } = &guard else {
                unreachable!()
            };
            if vec.len() > self.shared.max_packet_size.load(Ordering::Relaxed) {
                error!("Payload too large to send");
                std::mem::forget(guard);
                return;
            }

            let new_port = addr.port().wrapping_add(1);
            if new_port == 0 {
                addr.set_port(MIN_PORT);
            } else {
                addr.set_port(new_port);
            }

            let _ = self.shared.noreply_socket.send_to(vec, addr).await;
            std::mem::forget(guard);
        }
    }

    pub fn get_recycled_byte_vecs(&self) -> &RecycledByteVecs {
        &self.recycled_byte_vecs
    }

    pub fn send_reliable(&self, payload: Vec<u8>) {
        if payload.len() > self.shared.max_packet_size.load(Ordering::Relaxed) {
            error!("Payload too large to send");
            return;
        }
        self.shared
            .reliable_packet_sub
            .put(ReliablePacket::new(payload));
    }

    pub fn create_eventual_reliability_stream(&self) -> EventualReliabilityStream {
        EventualReliabilityStream {
            stream: &self,
            old_ack: None,
        }
    }

    /// Sets the address to send to for *all* CakapStreams created from the same CakapSocket.
    pub fn set_send_addr(&self, addr: SocketAddr) {
        self.shared.send_to_addr.store(Some(addr));
    }
}

enum SendUnreliable<'a, 'b> {
    Slice {
        stream: &'a CakapSender,
        slice: &'b [u8],
    },
    Vec {
        stream: &'a CakapSender,
        vec: Vec<u8>,
    },
}

impl<'a, 'b> Drop for SendUnreliable<'a, 'b> {
    fn drop(&mut self) {
        let (recycled_byte_vecs, stream, payload) = match self {
            SendUnreliable::Slice { stream, slice } => {
                let recycled_byte_vecs = stream.get_recycled_byte_vecs().clone();
                let mut vec = recycled_byte_vecs.get_vec();
                vec.extend_from_slice(slice);
                (recycled_byte_vecs, *stream, vec)
            }
            SendUnreliable::Vec { stream, vec } => (
                stream.get_recycled_byte_vecs().clone(),
                *stream,
                std::mem::take(vec),
            ),
        };

        let shared = stream.shared.clone();
        (|| async move {
            let Some(mut addr) = shared.send_to_addr.load() else {
                error!("No address to send to");
                return;
            };

            let new_port = addr.port().wrapping_add(1);
            if new_port == 0 {
                addr.set_port(MIN_PORT);
            } else {
                addr.set_port(new_port);
            }

            let _ = shared.noreply_socket.send_to(&payload, addr).await;
            recycled_byte_vecs.recycle_vec(payload);
        })
        .spawn();
    }
}

pub struct EventualReliabilityStream<'a> {
    stream: &'a CakapSender,
    old_ack: Option<[u8; 8]>,
}

impl<'a> EventualReliabilityStream<'a> {
    pub fn send(&mut self, payload: Vec<u8>) {
        if payload.len() > self.stream.shared.max_packet_size.load(Ordering::Relaxed) {
            error!("Payload too large to send");
            return;
        }
        let new_ack;
        if let Some(old_ack) = self.old_ack {
            let packet = ReliablePacket::replace(payload, old_ack);
            new_ack = packet.get_ack();
            self.stream.shared.reliable_packet_sub.put(packet);
        } else {
            let packet = ReliablePacket::new(payload);
            new_ack = packet.get_ack();
            self.stream.shared.reliable_packet_sub.put(packet);
        }
        self.old_ack = Some(new_ack);
    }
}
