#![feature(impl_trait_in_fn_trait_return)]

use std::{
    collections::BinaryHeap, future::Future, hash::{DefaultHasher, Hash, Hasher}, net::{Ipv4Addr, SocketAddr, SocketAddrV4}, sync::{Arc, OnceLock}, time::Duration
};

use crossbeam::queue::ArrayQueue;
use fxhash::FxHashSet;
use tasker::{callbacks::callee::Subscriber, define_callbacks, task::AsyncTask};
use tokio::{
    net::UdpSocket,
    time::{sleep_until, Instant},
};

const MIN_PORT: u16 = 10000;
define_callbacks!(pub BytesCallbacks => Fn(record: &[u8]) + Send + Sync);

pub struct CakapSocket {
    reply_socket: Arc<UdpSocket>,
    noreply_socket: Arc<UdpSocket>,
    pub max_packet_size: usize,
    pub retransmission_duration: Duration,
    pub recycled_byte_vec_size: usize,
    bytes_callbacks: BytesCallbacks,
    recycled_byte_vecs: RecycledByteVecs,
    reliable_packet_sub: Arc<Subscriber<ReliablePacket>>,
}

impl CakapSocket {
    pub async fn bind(port: u16) -> std::io::Result<Self> {
        let reply_socket = Arc::new(UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).await?);
        let mut noreply_port = reply_socket.local_addr()?.port().wrapping_add(1);
        if noreply_port == 0 {
            noreply_port = MIN_PORT;
        }
        let noreply_socket = Arc::new(UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, noreply_port)).await?);
        Ok(Self {
            reply_socket,
            noreply_socket,
            max_packet_size: 1496,
            retransmission_duration: Duration::from_millis(50),
            recycled_byte_vec_size: 0,
            bytes_callbacks: BytesCallbacks::default(),
            recycled_byte_vecs: RecycledByteVecs {
                queue: Arc::new(OnceLock::new()),
            },
            reliable_packet_sub: Arc::new(Subscriber::new_unbounded()),
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

    pub fn get_stream(&self) -> CakapStream {
        CakapStream {
            noreply_socket: self.noreply_socket.clone(),
            recycled_byte_vecs: self.get_recycled_byte_vecs(),
            reliable_packet_pub: Arc::new(self.reliable_packet_sub.create_callback()),
        }
    }

    pub fn get_connect_callback(&self) -> impl Fn(SocketAddr) -> impl Future<Output=std::io::Result<()>> {
        let reply_socket = self.reply_socket.clone();
        let noreply_socket = self.noreply_socket.clone();

        move |mut addr| {
            let reply_socket = reply_socket.clone();
            let noreply_socket = noreply_socket.clone();
            async move {
                let _ = reply_socket.connect(addr).await?;
                let mut noreply_port = addr.port().wrapping_add(1);
                if noreply_port == 0 {
                    noreply_port = MIN_PORT;
                }
                addr.set_port(noreply_port);
                noreply_socket.connect(addr).await
            }
        }
    }
}

impl AsyncTask for CakapSocket {
    type Output = Result<(), (Self, std::io::Error)>;

    async fn run(mut self) -> Self::Output {
        let mut reply_buf = vec![0u8; self.max_packet_size];
        let mut noreply_buf = vec![0u8; self.max_packet_size];
        let mut retransmit_acks: FxHashSet<[u8; 8]> = FxHashSet::default();
        let mut retransmission_queue: BinaryHeap<PacketToRetransmit> = BinaryHeap::new();
        
        loop {
            tokio::select! {
                result = self.reply_socket.recv(&mut reply_buf) => {
                    let len = match result {
                        Ok(x) => x,
                        Err(e) => break Err((self, e)),
                    };
                    if len == 8 && retransmit_acks.remove(&reply_buf[..8]) {
                        continue;
                    }
                    self.bytes_callbacks.call(&reply_buf[..len]);
                }
                result = self.noreply_socket.recv(&mut noreply_buf) => {
                    let len = match result {
                        Ok(x) => x,
                        Err(e) => break Err((self, e)),
                    };
                    self.bytes_callbacks.call(&noreply_buf[..len]);
                }
                e = async {
                    loop {
                        if let Some(retransmit) = retransmission_queue.peek() {
                            if retransmit_acks.contains(&retransmit.ack) {
                                sleep_until(retransmit.instant).await;

                                if let Err(e) = self.reply_socket.send(&retransmit.payload).await {
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
                        if let Some(packet) = self.reliable_packet_sub.recv().await {
                            break packet;
                        } else {
                            tokio::time::sleep(self.retransmission_duration).await;
                        }
                    }
                } => {
                    match packet {
                        ReliablePacket::New { payload, ack } => {
                            retransmit_acks.insert(ack);
                            if let Err(e) = self.reply_socket.send(&payload).await {
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
                            if let Err(e) = self.reply_socket.send(&payload).await {
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
pub struct CakapStream {
    noreply_socket: Arc<UdpSocket>,
    recycled_byte_vecs: RecycledByteVecs,
    reliable_packet_pub: Arc<dyn Fn(ReliablePacket) + Send + Sync>,
}

impl CakapStream {
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
            let _ = self.noreply_socket.send(payload).await;
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
            let SendUnreliable::Vec { vec, .. } = &guard else { unreachable!() };
            let _ = self.noreply_socket.send(vec).await;
            std::mem::forget(guard);
        }
    }

    pub fn get_recycled_byte_vecs(&self) -> RecycledByteVecs {
        self.recycled_byte_vecs.clone()
    }

    pub fn send_reliable(&self, payload: Vec<u8>) {
        (self.reliable_packet_pub)(ReliablePacket::new(payload));
    }

    pub fn create_eventual_reliability_stream(&self) -> EventualReliabilityStream {
        EventualReliabilityStream {
            reliable_packet_pub: &*self.reliable_packet_pub,
            old_ack: None
        }
    }
}

enum SendUnreliable<'a, 'b> {
    Slice {
        stream: &'a CakapStream,
        slice: &'b [u8],
    },
    Vec {
        stream: &'a CakapStream,
        vec: Vec<u8>,
    }
}

impl<'a, 'b> Drop for SendUnreliable<'a, 'b> {
    fn drop(&mut self) {
        let (recycled_byte_vecs, stream, payload) = match self {
            SendUnreliable::Slice { stream, slice } => {
                let recycled_byte_vecs = stream.get_recycled_byte_vecs();
                let mut vec = recycled_byte_vecs.get_vec();
                vec.extend_from_slice(slice);
                (recycled_byte_vecs, *stream, vec)
            }
            SendUnreliable::Vec { stream, vec } => (stream.get_recycled_byte_vecs(), *stream, std::mem::take(vec)),
        };

        let udp = stream.noreply_socket.clone();
        (|| async move {
            let _ = udp.send(&payload).await;
            recycled_byte_vecs.recycle_vec(payload);
        })
        .spawn();
    }
}

pub struct EventualReliabilityStream<'a> {
    reliable_packet_pub: &'a (dyn Fn(ReliablePacket) + Send + Sync),
    old_ack: Option<[u8; 8]>
}

impl<'a> EventualReliabilityStream<'a> {
    pub fn send(&mut self, payload: Vec<u8>) {
        let new_ack;
        if let Some(old_ack) = self.old_ack {
            let packet = ReliablePacket::replace(payload, old_ack);
            new_ack = packet.get_ack();
            (self.reliable_packet_pub)(packet);
        } else {
            let packet = ReliablePacket::new(payload);
            new_ack = packet.get_ack();
            (self.reliable_packet_pub)(packet);
        }
        self.old_ack = Some(new_ack);
    }
}