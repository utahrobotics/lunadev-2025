use std::{
    array::TryFromSliceError,
    ops::{Deref, DerefMut},
};

use bytes::BytesMut;
use rand::Rng;

pub mod ecc;
pub mod fragment;
pub mod sequenced;
pub mod serde;
pub mod simulation;
pub mod udp;

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
    fn get_max_packet_size(&self) -> usize;
}

impl<'a, T: Layer> Layer for &'a mut T {
    type SendError = T::SendError;
    type RecvError = T::RecvError;

    type SendItem = T::SendItem;
    type RecvItem = T::RecvItem;

    #[inline(always)]
    fn send(
        &mut self,
        data: Self::SendItem,
    ) -> impl std::future::Future<Output = Result<(), Self::SendError>> {
        T::send(self, data)
    }

    #[inline(always)]
    fn recv(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Self::RecvItem, Self::RecvError>> {
        T::recv(self)
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        T::get_max_packet_size(self)
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum UInt {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
}

impl UInt {
    #[inline(always)]
    pub fn to_u64(self) -> u64 {
        self.into()
    }

    #[inline(always)]
    pub fn to_be_bytes(self) -> UIntBeBytes {
        self.into()
    }

    // #[inline(always)]
    // pub(crate) fn copy_to_slice(self, slice: &mut [u8]) -> Result<(), TryFromSliceError> {
    //     self.to_be_bytes().copy_to_slice(slice)
    // }

    #[inline(always)]
    pub(crate) fn extend_bytes_mut(self, bytes: &mut BytesMut) {
        self.to_be_bytes().extend_bytes_mut(bytes)
    }

    pub fn size(self) -> usize {
        match self {
            Self::U8(_) => 1,
            Self::U16(_) => 2,
            Self::U32(_) => 4,
            Self::U64(_) => 8,
        }
    }

    pub fn to_variant(self) -> UIntVariant {
        match self {
            Self::U8(_) => UIntVariant::U8,
            Self::U16(_) => UIntVariant::U16,
            Self::U32(_) => UIntVariant::U32,
            Self::U64(_) => UIntVariant::U64,
        }
    }

    pub fn with_u64(self, value: u64) -> Self {
        match self {
            Self::U8(_) => Self::U8(value as u8),
            Self::U16(_) => Self::U16(value as u16),
            Self::U32(_) => Self::U32(value as u32),
            Self::U64(_) => Self::U64(value),
        }
    }

    pub fn fit_u64(value: u64) -> Self {
        if value <= u8::MAX as u64 {
            Self::U8(value as u8)
        } else if value <= u16::MAX as u64 {
            Self::U16(value as u16)
        } else if value <= u32::MAX as u64 {
            Self::U32(value as u32)
        } else {
            Self::U64(value)
        }
    }

    #[inline(always)]
    pub(crate) fn try_with_slice(
        self,
        fragment_count_slice: &[u8],
    ) -> Result<Self, TryFromSliceError> {
        self.to_variant().try_with_slice(fragment_count_slice)
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum UIntBeBytes {
    U8([u8; 1]),
    U16([u8; 2]),
    U32([u8; 4]),
    U64([u8; 8]),
}

impl From<UInt> for u64 {
    fn from(value: UInt) -> Self {
        match value {
            UInt::U8(value) => value as u64,
            UInt::U16(value) => value as u64,
            UInt::U32(value) => value as u64,
            UInt::U64(value) => value,
        }
    }
}

impl From<UInt> for UIntBeBytes {
    fn from(value: UInt) -> Self {
        match value {
            UInt::U8(value) => Self::U8(value.to_be_bytes()),
            UInt::U16(value) => Self::U16(value.to_be_bytes()),
            UInt::U32(value) => Self::U32(value.to_be_bytes()),
            UInt::U64(value) => Self::U64(value.to_be_bytes()),
        }
    }
}

impl From<UIntBeBytes> for UInt {
    fn from(value: UIntBeBytes) -> Self {
        match value {
            UIntBeBytes::U8(bytes) => UInt::U8(u8::from_be_bytes(bytes)),
            UIntBeBytes::U16(bytes) => UInt::U16(u16::from_be_bytes(bytes)),
            UIntBeBytes::U32(bytes) => UInt::U32(u32::from_be_bytes(bytes)),
            UIntBeBytes::U64(bytes) => UInt::U64(u64::from_be_bytes(bytes)),
        }
    }
}

impl From<UIntBeBytes> for u64 {
    fn from(value: UIntBeBytes) -> Self {
        match value {
            UIntBeBytes::U8(bytes) => u8::from_be_bytes(bytes) as u64,
            UIntBeBytes::U16(bytes) => u16::from_be_bytes(bytes) as u64,
            UIntBeBytes::U32(bytes) => u32::from_be_bytes(bytes) as u64,
            UIntBeBytes::U64(bytes) => u64::from_be_bytes(bytes),
        }
    }
}

impl Deref for UIntBeBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::U8(bytes) => bytes,
            Self::U16(bytes) => bytes,
            Self::U32(bytes) => bytes,
            Self::U64(bytes) => bytes,
        }
    }
}

impl DerefMut for UIntBeBytes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::U8(bytes) => bytes,
            Self::U16(bytes) => bytes,
            Self::U32(bytes) => bytes,
            Self::U64(bytes) => bytes,
        }
    }
}

impl UIntBeBytes {
    pub fn copy_to_slice(self, slice: &mut [u8]) -> Result<(), TryFromSliceError> {
        match self {
            Self::U8(bytes) => {
                let arr: &mut [u8; 1] = slice.try_into()?;
                arr.copy_from_slice(&bytes);
            }
            Self::U16(bytes) => {
                let arr: &mut [u8; 2] = slice.try_into()?;
                arr.copy_from_slice(&bytes);
            }
            Self::U32(bytes) => {
                let arr: &mut [u8; 4] = slice.try_into()?;
                arr.copy_from_slice(&bytes);
            }
            Self::U64(bytes) => {
                let arr: &mut [u8; 8] = slice.try_into()?;
                arr.copy_from_slice(&bytes);
            }
        }
        Ok(())
    }

    pub fn size(self) -> usize {
        match self {
            Self::U8(_) => 1,
            Self::U16(_) => 2,
            Self::U32(_) => 4,
            Self::U64(_) => 8,
        }
    }

    pub fn to_variant(self) -> UIntVariant {
        match self {
            Self::U8(_) => UIntVariant::U8,
            Self::U16(_) => UIntVariant::U16,
            Self::U32(_) => UIntVariant::U32,
            Self::U64(_) => UIntVariant::U64,
        }
    }

    #[inline(always)]
    pub fn extend_bytes_mut(self, bytes: &mut BytesMut) {
        bytes.extend_from_slice(self.deref());
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum UIntVariant {
    U8,
    U16,
    U32,
    U64,
}

impl UIntVariant {
    pub(crate) fn random(&self, mut rng: impl Rng) -> UInt {
        match self {
            Self::U8 => UInt::U8(rng.gen()),
            Self::U16 => UInt::U16(rng.gen()),
            Self::U32 => UInt::U32(rng.gen()),
            Self::U64 => UInt::U64(rng.gen()),
        }
    }

    pub fn size(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::U64 => 8,
        }
    }

    pub fn with_u64(self, num: u64) -> UInt {
        match self {
            Self::U8 => UInt::U8(num as u8),
            Self::U16 => UInt::U16(num as u16),
            Self::U32 => UInt::U32(num as u32),
            Self::U64 => UInt::U64(num),
        }
    }

    pub fn try_with_slice(self, slice: &[u8]) -> Result<UInt, TryFromSliceError> {
        match slice.len() {
            1 => Ok(UIntBeBytes::U8(slice.try_into().unwrap()).into()),
            2 => Ok(UIntBeBytes::U16(slice.try_into().unwrap()).into()),
            4 => Ok(UIntBeBytes::U32(slice.try_into().unwrap()).into()),
            8 => Ok(UIntBeBytes::U64(slice.try_into().unwrap()).into()),
            // Hack since we cannot make an instance of TryFromSliceError
            _ => TryInto::<[u8; 1]>::try_into(slice).map(|_| unreachable!()),
        }
    }

    pub fn max_value(self) -> UInt {
        match self {
            Self::U8 => UInt::U8(u8::MAX),
            Self::U16 => UInt::U16(u16::MAX),
            Self::U32 => UInt::U32(u32::MAX),
            Self::U64 => UInt::U64(u64::MAX),
        }
    }
}