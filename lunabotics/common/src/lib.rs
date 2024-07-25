use std::{cell::RefCell, io::Write};

use bitcode::{Buffer, Decode, Encode};

thread_local! {
    static BITCODE_BUFFER: RefCell<Buffer> = RefCell::new(Buffer::default());
}

#[derive(Debug, Encode, Decode)]
pub enum FromLunabase {
    Ping,
    ContinueMission,
    TriggerSetup,
}

impl TryFrom<&[u8]> for FromLunabase {
    type Error = bitcode::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        BITCODE_BUFFER.with_borrow_mut(|buf| buf.decode(value))
    }
}

impl FromLunabase {
    pub fn encode<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T {
        BITCODE_BUFFER.with_borrow_mut(|buf| f(buf.encode(self)))
    }

    fn write_code(&self, mut w: impl Write) -> std::io::Result<()> {
        self.encode(|bytes| {
            write!(w, "{self:?} = 0x")?;
            for b in bytes {
                write!(w, "{b:x}")?;
            }
            writeln!(w, "")
        })
    }

    pub fn write_code_sheet(mut w: impl Write) -> std::io::Result<()> {
        FromLunabase::Ping.write_code(&mut w)?;
        FromLunabase::ContinueMission.write_code(&mut w)?;
        FromLunabase::TriggerSetup.write_code(&mut w)?;
        Ok(())
    }
}

#[derive(Debug, Encode, Decode)]
pub enum FromLunabot {
    Pong,
}

impl TryFrom<&[u8]> for FromLunabot {
    type Error = bitcode::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        BITCODE_BUFFER.with_borrow_mut(|buf| buf.decode(value))
    }
}

impl FromLunabot {
    pub fn encode<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T {
        BITCODE_BUFFER.with_borrow_mut(|buf| f(buf.encode(self)))
    }

    fn write_code(&self, mut w: impl Write) -> std::io::Result<()> {
        self.encode(|bytes| {
            write!(w, "{self:?} = 0x")?;
            for b in bytes {
                write!(w, "{b:x}")?;
            }
            writeln!(w, "")
        })
    }

    pub fn write_code_sheet(mut w: impl Write) -> std::io::Result<()> {
        FromLunabot::Pong.write_code(&mut w)?;
        Ok(())
    }
}
