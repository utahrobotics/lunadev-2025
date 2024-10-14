use std::io::Write;

use bitcode::{Decode, Encode};

pub mod lunasim;

#[derive(Debug, Encode, Decode, Clone, Copy, PartialEq, Eq)]
pub enum LunabotStage {
    TeleOp,
    SoftStop,
    TraverseObstacles,
    Dig,
    Dump,
}

#[derive(Debug, Encode, Decode, Clone, Copy, PartialEq, Eq)]
pub enum FromLunabase {
    // Pong,
    ContinueMission,
    Steering(Steering),
    TraverseObstacles,
    SoftStop,
}

impl FromLunabase {
    fn write_code(&self, mut w: impl Write) -> std::io::Result<()> {
        let bytes = bitcode::encode(self);
        write!(w, "{self:?} = 0x")?;
        for b in bytes {
            write!(w, "{b:x}")?;
        }
        writeln!(w, "")
    }

    pub fn write_code_sheet(mut w: impl Write) -> std::io::Result<()> {
        // FromLunabase::Pong.write_code(&mut w)?;
        FromLunabase::ContinueMission.write_code(&mut w)?;
        FromLunabase::Steering(Steering::new(0.0, 0.0)).write_code(&mut w)?;
        FromLunabase::TraverseObstacles.write_code(&mut w)?;
        FromLunabase::SoftStop.write_code(&mut w)?;
        Ok(())
    }
}

#[derive(Debug, Encode, Decode, Clone, Copy, PartialEq, Eq)]
pub enum FromLunabot {
    Ping(LunabotStage),
}

impl FromLunabot {
    fn write_code(&self, mut w: impl Write) -> std::io::Result<()> {
        let bytes = bitcode::encode(self);
        write!(w, "{self:?} = 0x")?;
        for b in bytes {
            write!(w, "{b:x}")?;
        }
        writeln!(w, "")
    }

    pub fn write_code_sheet(mut w: impl Write) -> std::io::Result<()> {
        FromLunabot::Ping(LunabotStage::TeleOp).write_code(&mut w)?;
        FromLunabot::Ping(LunabotStage::SoftStop).write_code(&mut w)?;
        FromLunabot::Ping(LunabotStage::TraverseObstacles).write_code(&mut w)?;
        FromLunabot::Ping(LunabotStage::Dig).write_code(&mut w)?;
        FromLunabot::Ping(LunabotStage::Dump).write_code(&mut w)?;
        Ok(())
    }
}

const MAX_STEERING: u8 = 14;

#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq)]
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

    pub fn get_left_and_right(self) -> (f64, f64) {
        let (drive, steering) = self.get_drive_and_steering();
        let opposite_drive = 1.0 - 2.0 * steering.abs();

        if steering >= 0.0 {
            (drive * opposite_drive, drive)
        } else {
            (drive, drive * opposite_drive)
        }
    }
}

impl Default for Steering {
    fn default() -> Self {
        Self::new(0.0, 0.0)
    }
}
