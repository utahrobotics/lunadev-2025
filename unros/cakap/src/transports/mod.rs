use std::{future::Future, ops::ControlFlow};

use crate::channel::{Channel, ChannelIdentifier};

#[cfg(feature = "laminar")]
pub mod laminar;

pub trait Transport {
    type PeerIdentifier;
    type ConnectionError;
    type ListenError;
    type PeerTransport: PeerTransport;

    fn connect_to_peer(
        &self,
        remote_id: Self::PeerIdentifier,
    ) -> impl Future<Output = Result<Self::PeerTransport, Self::ConnectionError>>;

    fn listen_for_peers<B>(
        &self,
        on_connect: impl Fn(Self::PeerTransport) -> ControlFlow<B>,
    ) -> impl Future<Output = Result<B, Self::ListenError>>;
}

pub trait PeerTransport {
    type DisconnectionError;
    type ChannelError;

    fn disconnect(self) -> impl Future<Output = Result<(), Self::DisconnectionError>>;

    fn wait_for_channel<'a, C, I>(
        &self,
        on_channel: impl FnMut(C, I) -> ControlFlow<C>,
    ) -> impl Future<Output = C>
    where
        C: Channel,
        I: ChannelIdentifier<'a>;

    fn negotiate_channel<'a, C, I>(
        &self,
        channel_id: I,
    ) -> impl Future<Output = Result<C, Self::ChannelError>>
    where
        C: Channel,
        I: ChannelIdentifier<'a>;
}
