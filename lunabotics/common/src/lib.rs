use std::cell::RefCell;

use bitcode::{Buffer, Decode, Encode};


thread_local! {
    static BITCODE_BUFFER: RefCell<Buffer> = RefCell::new(Buffer::default());
}


#[derive(Debug, Encode, Decode)]
pub enum FromLunabase {
    Ping,
    ContinueMission,
    TriggerSetup
}

impl TryFrom<&[u8]> for FromLunabase {
    type Error = bitcode::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        BITCODE_BUFFER.with_borrow_mut(|buf| buf.decode(value))
    }
}


impl FromLunabase {
    pub fn encode(&self, f: impl FnOnce(&[u8])) {
        BITCODE_BUFFER.with_borrow_mut(|buf| f(buf.encode(self)));
    }
}


#[derive(Debug, Encode, Decode)]
pub enum FromLunabot {
    Pong
}

impl TryFrom<&[u8]> for FromLunabot {
    type Error = bitcode::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        BITCODE_BUFFER.with_borrow_mut(|buf| buf.decode(value))
    }
}

impl FromLunabot {
    pub fn encode(&self, f: impl FnOnce(&[u8])) {
        BITCODE_BUFFER.with_borrow_mut(|buf| f(buf.encode(self)));
    }
}