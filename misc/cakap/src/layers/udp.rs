use std::sync::Arc;

use bytes::BytesMut;
use tokio::net::UdpSocket;

use super::Layer;

pub struct UdpTransport {
    socket: Arc<UdpSocket>,
    pub maximum_packet_size: usize,
}

impl From<Arc<UdpSocket>> for UdpTransport {
    fn from(socket: Arc<UdpSocket>) -> Self {
        UdpTransport {
            socket,
            maximum_packet_size: 1450,
        }
    }
}

impl From<UdpSocket> for UdpTransport {
    fn from(socket: UdpSocket) -> Self {
        UdpTransport::from(Arc::new(socket))
    }
}

impl UdpTransport {
    pub fn set_checksum_enabled(&self, value: bool) -> std::io::Result<()> {
        let value = if value { 0 } else { 1 };
        let err;
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::io::AsRawSocket;
            use windows_sys::Win32::Networking::WinSock::{IPPROTO_UDP, UDP_NOCHECKSUM};

            let sock_n = self.socket.as_raw_socket();
            err = unsafe {
                libc::setsockopt(sock_n as usize, IPPROTO_UDP, UDP_NOCHECKSUM, &value, 4)
            };
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let sock_n = self.socket.as_raw_fd();
            let ptr: *const _ = &value;
            err = unsafe {
                libc::setsockopt(sock_n, libc::SOL_SOCKET, libc::SO_NO_CHECK, ptr.cast(), 4)
            };
        }
        if err == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

impl Layer for UdpTransport {
    type SendError = std::io::Error;
    type RecvError = std::io::Error;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        if data.len() > self.maximum_packet_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Packet too big",
            ));
        }
        self.socket.send(&data).await.map(|_| ())
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        let mut bytes = BytesMut::with_capacity(self.maximum_packet_size);
        self.socket.recv_buf(&mut bytes).await?;
        Ok(bytes)
    }

    fn get_max_packet_size(&self) -> usize {
        self.maximum_packet_size
    }
}
