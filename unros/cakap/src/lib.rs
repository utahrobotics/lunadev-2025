use std::ops::ControlFlow;

use channel::{Channel, ChannelIdentifier};
use transports::{PeerTransport, Transport};

pub mod channel;
pub mod layers;
pub mod transports;

pub struct Peer<T: PeerTransport> {
    transport: T,
}

impl<T: PeerTransport> Peer<T> {
    #[inline]
    pub async fn disconnect(self) -> Result<(), T::DisconnectionError> {
        self.transport.disconnect().await
    }

    #[inline]
    pub async fn wait_for_channel<'a, C, I>(
        &self,
        on_channel: impl FnMut(C, I) -> ControlFlow<C>,
    ) -> C
    where
        C: Channel,
        I: ChannelIdentifier<'a>,
    {
        self.transport.wait_for_channel(on_channel).await
    }

    #[inline]
    pub async fn negotiate_channel<'a, C, I>(&self, channel_id: I) -> Result<C, T::ChannelError>
    where
        C: Channel,
        I: ChannelIdentifier<'a>,
    {
        self.transport.negotiate_channel(channel_id).await
    }
}

pub struct Socket<T: Transport> {
    transport: T,
}

impl<T: Transport> Socket<T> {
    #[inline]
    pub fn new(transport: T) -> Self {
        Socket { transport }
    }

    #[inline]
    pub async fn connect_to_peer(
        &self,
        remote_id: T::PeerIdentifier,
    ) -> Result<Peer<T::PeerTransport>, T::ConnectionError> {
        self.transport
            .connect_to_peer(remote_id)
            .await
            .map(|transport| Peer { transport })
    }

    #[inline]
    pub async fn listen_for_peers<B>(
        &self,
        on_connect: impl Fn(Peer<T::PeerTransport>) -> ControlFlow<B>,
    ) -> Result<B, T::ListenError> {
        self.transport
            .listen_for_peers(|transport| on_connect(Peer { transport }))
            .await
    }
}
