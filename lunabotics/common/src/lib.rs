use std::{cell::RefCell, io::Write};

use bitcode::{Buffer, Decode, Encode};

pub mod lunasim;

thread_local! {
    static BITCODE_BUFFER: RefCell<Buffer> = RefCell::new(Buffer::default());
}

#[derive(Debug, Encode, Decode)]
pub enum FromLunabase {
    Pong,
    ContinueMission,
    TriggerSetup,
    Steering(Steering),
    TraverseObstacles,
    SoftStop,
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
        FromLunabase::Pong.write_code(&mut w)?;
        FromLunabase::ContinueMission.write_code(&mut w)?;
        FromLunabase::TriggerSetup.write_code(&mut w)?;
        FromLunabase::Steering(Steering::new(0.0, 0.0)).write_code(&mut w)?;
        FromLunabase::TraverseObstacles.write_code(&mut w)?;
        FromLunabase::SoftStop.write_code(&mut w)?;
        Ok(())
    }
}

#[derive(Debug, Encode, Decode)]
pub enum FromLunabot {
    Ping,
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
        FromLunabot::Ping.write_code(&mut w)?;
        Ok(())
    }
}

const MAX_STEERING: u8 = 14;

#[derive(Encode, Decode, Clone, Copy)]
pub struct Steering(u8);

impl std::fmt::Debug for Steering {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (drive, steering) = self.get_drive_and_steering();
        f.debug_struct("Steering")
            .field("drive", &drive)
            .field("steering", &steering)
            .finish()
    }
}

impl Steering {
    pub fn new(mut drive: f64, mut steering: f64) -> Self {
        if drive < -1.0 {
            drive = -1.0;
        } else if drive > 1.0 {
            drive = 1.0;
        }
        if steering < -1.0 {
            steering = -1.0;
        } else if steering > 1.0 {
            steering = 1.0;
        }
        let drive = ((drive + 1.0) / 2.0 * MAX_STEERING as f64).round() as u8;
        let steering = ((steering + 1.0) / 2.0 * MAX_STEERING as f64).round() as u8;

        Self(drive << 4 | steering)
    }

    pub fn get_drive_and_steering(self) -> (f64, f64) {
        let drive = (self.0 >> 4) as f64 / MAX_STEERING as f64 * 2.0 - 1.0;
        let steering = (self.0 & 0b00001111) as f64 / MAX_STEERING as f64 * 2.0 - 1.0;
        (drive, steering)
    }
}


impl Default for Steering {
    fn default() -> Self {
        Self::new(0.0, 0.0)
    }
}