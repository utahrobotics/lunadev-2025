use std::net::SocketAddr;

use bytes::BytesMut;
use pnet::transport::transport_channel;
use pnet::transport::TransportChannelType;
use pnet::transport::TransportProtocol;
use pnet::{
    packet::{
        ip::IpNextHeaderProtocols,
        udp::MutableUdpPacket,
        Packet,
    },
    transport::{udp_packet_iter, TransportSender},
};
use tokio::sync::mpsc::Receiver as AsyncReceiver;

use super::Layer;

pub struct UdpTransport {
    sender: TransportSender,
    receiver: AsyncReceiver<std::io::Result<BytesMut>>,
    packet_buffer: Vec<u8>,
    destination: SocketAddr,
    source: SocketAddr,
    pub checksum_enabled: bool,
}

impl UdpTransport {
    pub fn connect(source: SocketAddr, destination: SocketAddr, buffer_size: usize) -> std::io::Result<Self> {
        let protocol = TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Udp));

        let (sender, mut rx) = transport_channel(buffer_size, protocol)?;
        let (incoming_sender, receiver) = tokio::sync::mpsc::channel(std::thread::available_parallelism().map(|x| x.get()).unwrap_or(8));

        std::thread::spawn(move || {
            let mut iter = udp_packet_iter(&mut rx);
            loop {
                match iter.next() {
                    Ok((packet, addr)) => {
                        if addr != destination.ip() {
                            continue;
                        }
                        let mut bytes = BytesMut::with_capacity(packet.packet().len());
                        bytes.extend_from_slice(packet.packet());
                        if incoming_sender.blocking_send(Ok(bytes)).is_err() {
                            break;
                        }
                    }
                    Err(e) => if incoming_sender.blocking_send(Err(e)).is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self {
            sender,
            receiver,
            packet_buffer: Vec::new(),
            destination,
            source,
            checksum_enabled: true,
        })
    }
}

impl Layer for UdpTransport {
    type SendError = std::io::Error;
    type RecvError = std::io::Error;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        assert!(MutableUdpPacket::minimum_packet_size() <= u8::MAX as usize);
        self.packet_buffer.clear();
        self.packet_buffer.extend_from_slice(&data);
        for _ in
            0..MutableUdpPacket::minimum_packet_size().saturating_sub(self.packet_buffer.len() + 1)
        {
            self.packet_buffer.push(0);
        }
        self.packet_buffer
            .push(data.len().min(u8::MAX as usize) as u8);
        for _ in 0..5 {
            let mut udp_packet = MutableUdpPacket::new(&mut self.packet_buffer).unwrap();
            if !self.checksum_enabled {
                udp_packet.set_checksum(0);
            }
            udp_packet.set_source(self.source.port());
            udp_packet.set_destination(self.destination.port());
            let expected_n = udp_packet.packet().len();
            let n = self.sender.send_to(udp_packet, self.destination.ip())?;
            if n == expected_n {
                return Ok(());
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to send complete packet",
        ))
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        let mut bytes = self.receiver.recv().await.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Failed to receive packet",
            )
        })??;

        if bytes.len() <= MutableUdpPacket::minimum_packet_size() {
            bytes.truncate(bytes.last().copied().unwrap_or_default() as usize);
        } else {
            bytes.truncate(bytes.len().saturating_sub(1));
        }

        Ok(bytes)
    }
}
