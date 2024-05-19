use std::{future::Future, net::SocketAddr};

use laminar::Socket;

use super::Transport;

pub struct LaminarTransport {
    socket: Socket,
}

impl Transport for LaminarTransport {
    type PeerIdentifier = SocketAddr;

    type ConnectionError = laminar::ErrorKind;

    type ListenError = laminar::ErrorKind;

    type PeerTransport;

    fn connect_to_peer(
        &self,
        remote_id: Self::PeerIdentifier,
    ) -> impl Future<Output = Result<Self::PeerTransport, Self::ConnectionError>> {
        todo!()
    }

    fn listen_for_peers<B>(
        &self,
        on_connect: impl Fn(Self::PeerTransport) -> std::ops::ControlFlow<B>,
    ) -> impl Future<Output = Result<B, Self::ListenError>> {
        todo!()
    }
}

impl crate::Socket<LaminarTransport> {}
