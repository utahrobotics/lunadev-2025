use std::{
    collections::HashMap,
    marker::PhantomData,
    net::{Ipv4Addr, SocketAddrV4},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bitcode::{decode, encode, DecodeOwned, Encode};
pub use bytes;
use bytes::{Bytes, BytesMut};
use crossbeam::queue::SegQueue;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::UdpSocket,
};
use webrtc_sctp::{
    association::{Association, Config},
    chunk::chunk_payload_data::PayloadProtocolIdentifier,
    stream::{PollStream, ReliabilityType, Stream},
};
use webrtc_util::conn::conn_disconnected_packet::DisconnectedPacketConn;

pub struct Connection {
    association: Association,
    unmatched_streams: HashMap<u16, Arc<Stream>>,
}

impl Connection {
    pub async fn bind(local_socket: u16, max_packet_size: u32) -> std::io::Result<Self> {
        let sctp_udp =
            UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, local_socket)).await?;
        let sctp = Association::server(Config {
            net_conn: Arc::new(DisconnectedPacketConn::new(Arc::new(sctp_udp))),
            max_receive_buffer_size: max_packet_size,
            max_message_size: max_packet_size,
            name: "server".into(),
        })
        .await?;

        Ok(Self {
            association: sctp,
            unmatched_streams: HashMap::new(),
        })
    }

    pub async fn connect(
        client_addr: SocketAddrV4,
        local_socket: u16,
        max_packet_size: u32,
    ) -> std::io::Result<Self> {
        let sctp_udp =
            UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, local_socket)).await?;
        sctp_udp.connect(client_addr).await?;
        let sctp = Association::client(Config {
            net_conn: Arc::new(sctp_udp),
            max_receive_buffer_size: max_packet_size,
            max_message_size: max_packet_size,
            name: client_addr.to_string(),
        })
        .await?;

        Ok(Self {
            association: sctp,
            unmatched_streams: HashMap::new(),
        })
    }

    pub async fn open_unordered_unreliable_stream<T: ?Sized>(
        &self,
        id: u16,
    ) -> std::io::Result<CakapStream<T>> {
        let stream = self
            .association
            .open_stream(id, PayloadProtocolIdentifier::Binary)
            .await?;
        stream.set_reliability_params(true, ReliabilityType::Rexmit, 0);
        Ok(CakapStream {
            stream,
            max_packet_size: self.association.max_message_size() as usize,
            buffer_queue: SegQueue::default(),
            _phantom: PhantomData,
        })
    }

    pub async fn open_ordered_unreliable_stream<T: ?Sized>(
        &self,
        id: u16,
    ) -> std::io::Result<CakapStream<T>> {
        let stream = self
            .association
            .open_stream(id, PayloadProtocolIdentifier::Binary)
            .await?;
        stream.set_reliability_params(false, ReliabilityType::Rexmit, 0);
        Ok(CakapStream {
            stream,
            max_packet_size: self.association.max_message_size() as usize,
            buffer_queue: SegQueue::default(),
            _phantom: PhantomData,
        })
    }

    pub async fn open_unordered_reliable_stream<T: ?Sized>(
        &self,
        id: u16,
    ) -> std::io::Result<CakapStream<T>> {
        let stream = self
            .association
            .open_stream(id, PayloadProtocolIdentifier::Binary)
            .await?;
        stream.set_reliability_params(true, ReliabilityType::Reliable, 0);
        Ok(CakapStream {
            stream,
            max_packet_size: self.association.max_message_size() as usize,
            buffer_queue: SegQueue::default(),
            _phantom: PhantomData,
        })
    }

    pub async fn open_ordered_reliable_stream<T: ?Sized>(
        &self,
        id: u16,
    ) -> std::io::Result<CakapStream<T>> {
        let stream = self
            .association
            .open_stream(id, PayloadProtocolIdentifier::Binary)
            .await?;
        stream.set_reliability_params(false, ReliabilityType::Reliable, 0);
        Ok(CakapStream {
            stream,
            max_packet_size: self.association.max_message_size() as usize,
            buffer_queue: SegQueue::default(),
            _phantom: PhantomData,
        })
    }

    pub async fn open_byte_stream(&self, id: u16) -> std::io::Result<RWStream> {
        let stream = self
            .association
            .open_stream(id, PayloadProtocolIdentifier::Binary)
            .await?;
        stream.set_reliability_params(false, ReliabilityType::Reliable, 0);
        Ok(RWStream(PollStream::new(stream)))
    }

    async fn accept_sctp_stream(&mut self, id: u16) -> Option<Arc<Stream>> {
        if let Some(stream) = self.unmatched_streams.remove(&id) {
            return Some(stream);
        }
        loop {
            let stream = self.association.accept_stream().await?;
            if stream.stream_identifier() == id {
                break Some(stream);
            } else {
                self.unmatched_streams
                    .insert(stream.stream_identifier(), stream);
            }
        }
    }

    pub async fn accept_stream<T: ?Sized>(&mut self, id: u16) -> Option<CakapStream<T>> {
        let stream = self.accept_sctp_stream(id).await?;
        Some(CakapStream {
            stream,
            max_packet_size: self.association.max_message_size() as usize,
            buffer_queue: SegQueue::default(),
            _phantom: PhantomData,
        })
    }

    pub async fn accept_rw_stream(&mut self, id: u16) -> Option<RWStream> {
        let stream = self.accept_sctp_stream(id).await?;
        Some(RWStream(PollStream::new(stream)))
    }
}

pub struct CakapStream<T: ?Sized> {
    stream: Arc<Stream>,
    max_packet_size: usize,
    buffer_queue: SegQueue<Box<[u8]>>,
    _phantom: PhantomData<T>,
}

impl<T: Encode + DecodeOwned> CakapStream<T> {
    pub async fn send(&self, message: &T) -> anyhow::Result<()> {
        let payload = encode(message);
        if payload.len() > self.max_packet_size {
            return Err(anyhow::anyhow!("Payload too large"));
        }
        self.stream.write(&Bytes::from(payload)).await?;
        Ok(())
    }

    pub async fn recv(&self) -> anyhow::Result<T> {
        let mut buf = self
            .buffer_queue
            .pop()
            .unwrap_or_else(|| vec![0; self.max_packet_size].into_boxed_slice());
        let n = self.stream.read(&mut buf).await?;
        let res = decode(&buf[..n]).map_err(Into::into);
        self.buffer_queue.push(buf);
        res
    }
}

impl CakapStream<[u8]> {
    pub async fn send(&self, message: &[u8]) -> anyhow::Result<()> {
        if message.len() > self.max_packet_size {
            return Err(anyhow::anyhow!("Payload too large"));
        }
        self.stream.write(&Bytes::copy_from_slice(message)).await?;
        Ok(())
    }

    pub async fn recv(&self, bytes: &mut BytesMut) -> anyhow::Result<usize> {
        let mut buf = self
            .buffer_queue
            .pop()
            .unwrap_or_else(|| vec![0; self.max_packet_size].into_boxed_slice());
        let n = self.stream.read(&mut buf).await?;
        bytes.extend_from_slice(&buf[..n]);
        self.buffer_queue.push(buf);
        Ok(n)
    }
}

pub struct RWStream(PollStream);

impl AsyncRead for RWStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for RWStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}
