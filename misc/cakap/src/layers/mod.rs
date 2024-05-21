pub mod fragment;
pub mod sequenced;
pub mod serde;
pub mod simulation;
pub mod udp;
pub mod ecc;

pub trait Layer {
    type SendError;
    type RecvError;

    type SendItem;
    type RecvItem;

    fn send(
        &mut self,
        data: Self::SendItem,
    ) -> impl std::future::Future<Output = Result<(), Self::SendError>>;
    fn recv(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Self::RecvItem, Self::RecvError>>;
}

pub enum UInt {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
}

pub enum UIntVariant {
    U8,
    U16,
    U32,
    U64,
}
