use bitcode::{Decode, Encode};

pub trait ChannelIdentifier<'a>: Sized {
    type Error;
    fn from_bytes(bytes: &'a [u8]) -> Result<Self, Self::Error>;

    fn to_bytes(self) -> Box<[u8]>;
}

impl<'a> ChannelIdentifier<'a> for String {
    type Error = std::string::FromUtf8Error;
    fn from_bytes(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        String::from_utf8(bytes.to_vec())
    }

    fn to_bytes(self) -> Box<[u8]> {
        self.into_bytes().into_boxed_slice()
    }
}

impl<'a> ChannelIdentifier<'a> for &'a str {
    type Error = std::str::Utf8Error;

    fn from_bytes(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        std::str::from_utf8(bytes)
    }

    fn to_bytes(self) -> Box<[u8]> {
        self.as_bytes().to_vec().into_boxed_slice()
    }
}

impl<'a> ChannelIdentifier<'a> for u32 {
    type Error = std::array::TryFromSliceError;
    fn from_bytes(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        let bytes: [u8; 4] = bytes.try_into()?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn to_bytes(self) -> Box<[u8]> {
        self.to_le_bytes().to_vec().into_boxed_slice()
    }
}

#[derive(Encode, Decode)]
pub struct CompleteChannelIdentifier {
    layer_ids: Box<[Box<[u8]>]>,
    channel_id: Box<[u8]>,
}

pub trait Channel {
    fn create_layer_ids() -> Box<[Box<[u8]>]>;
}
