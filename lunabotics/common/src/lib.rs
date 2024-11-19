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
    Pong,
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

#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq)]
pub struct Steering(u8);

impl std::fmt::Debug for Steering {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (left, right) = self.get_left_and_right();
        f.debug_struct("Steering")
            .field("left", &left)
            .field("right", &right)
            .finish()
    }
}

impl Steering {
    pub fn new(mut drive: f64, mut steering: f64) -> Self {
        drive = drive.max(-1.0).min(1.0);
        steering = steering.max(-1.0).min(1.0);

        let opposite_drive = 1.0 - 2.0 * steering.abs();

        let (left, right) = if steering >= 0.0 {
            (drive * opposite_drive, drive)
        } else {
            (drive, drive * opposite_drive)
        };

        Self::new_left_right(left, right)
    }

    pub fn get_left_and_right(self) -> (f64, f64) {
        let mut left = ((self.0 >> 4) as f64 - 7.0) / 7.0;
        let mut right = ((self.0 & 0b1111) as f64 - 7.0) / 7.0;

        left = left.min(1.0);
        right = right.min(1.0);

        (left, right)
    }

    pub fn new_left_right(mut left: f64, mut right: f64) -> Self {
        left = left.max(-1.0).min(1.0);
        right = right.max(-1.0).min(1.0);

        let left = (left * 7.0 + 7.0).round() as u8;
        let right = (right * 7.0 + 7.0).round() as u8;

        Self(left << 4 | right)
    }
}

impl Default for Steering {
    fn default() -> Self {
        Self::new(0.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Steering;

    #[test]
    fn left_right01() {
        let s = Steering::new(0.0, 0.0);
        assert_eq!(s, Steering::new_left_right(0.0, 0.0));
    }

    #[test]
    fn left_right02() {
        let s = Steering::new(1.0, 1.0);
        assert_eq!(s, Steering::new_left_right(-1.0, 1.0));
    }

    #[test]
    fn left_right03() {
        let s = Steering::new(-1.0, -1.0);
        assert_eq!(s, Steering::new_left_right(-1.0, 1.0));
    }

    #[test]
    fn equality01() {
        assert_eq!(
            Steering::new(1.0, 1.0).get_left_and_right(),
            Steering::new(-1.0, -1.0).get_left_and_right()
        );
    }

    #[test]
    fn equality02() {
        assert_eq!(Steering::new(1.0, 1.0), Steering::new(-1.0, -1.0));
    }

    #[test]
    fn invertibility01() {
        let s = Steering::new(1.0, 1.0);
        let (left, right) = s.get_left_and_right();
        assert_eq!((left, right), (-1.0, 1.0));
        assert_eq!(s, Steering::new_left_right(left, right));
    }
}
