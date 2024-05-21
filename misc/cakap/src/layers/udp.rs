use std::net::SocketAddr;

use bytes::BytesMut;
use pnet::{packet::{udp::{MutableUdpPacket, UdpPacket}, Packet}, transport::{udp_packet_iter, TransportReceiver, TransportSender}};
use tokio::sync::mpsc::Receiver as AsyncReceiver;

use super::Layer;

pub struct UdpTransport {
    sender: TransportSender,
    receiver: AsyncReceiver<BytesMut>,
    packet_buffer: Vec<u8>,
    dest: SocketAddr,
    src: SocketAddr,
    checksum_enabled: bool,
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
        for _ in 0..MutableUdpPacket::minimum_packet_size().saturating_sub(self.packet_buffer.len() + 1) {
            self.packet_buffer.push(0);
        }
        self.packet_buffer.push(data.len().min(u8::MAX as usize) as u8);
        for _ in 0..5 {
            let mut udp_packet = MutableUdpPacket::new(&mut self.packet_buffer).unwrap();
            if !self.checksum_enabled {
                udp_packet.set_checksum(0);
            }
            udp_packet.set_source(self.src.port());
            udp_packet.set_destination(self.dest.port());
            let expected_n = udp_packet.packet().len();
            let n = self.sender.send_to(udp_packet, self.dest.ip())?;
            if n == expected_n {
                return Ok(());
            }
        }
        Err(std::io::Error::new(std::io::ErrorKind::Other, "Failed to send complete packet"))
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        let mut bytes = self.receiver.recv().await.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Failed to receive packet"))?;

        if bytes.len() <= MutableUdpPacket::minimum_packet_size() {
            bytes.truncate(bytes.last().copied().unwrap_or_default() as usize);
        } else {
            bytes.truncate(bytes.len().saturating_sub(1));
        }

        Ok(bytes)
    }
}
