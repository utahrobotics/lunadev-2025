#![feature(f16, try_blocks)]

use std::io::Write;

use bitcode::{Decode, Encode};
use nalgebra::{distance, Point2, Point3};

/// Taken from https://opus-codec.org/docs/opus_api-1.5/group__opus__encoder.html#gad2d6bf6a9ffb6674879d7605ed073e25
pub const AUDIO_FRAME_SIZE: u32 = 960;
pub const AUDIO_SAMPLE_RATE: u32 = 48000;
pub const THALASSIC_CELL_SIZE: f32 = 0.03125;
pub const THALASSIC_WIDTH: u32 = 128;
pub const THALASSIC_HEIGHT: u32 = 256;
pub const THALASSIC_CELL_COUNT: u32 = THALASSIC_WIDTH * THALASSIC_HEIGHT;

// #[cfg(feature = "godot_urdf")]
// pub mod godot_urdf;
pub mod lunasim;
pub mod ports;
#[cfg(feature = "lunabase_sync")]
pub mod lunabase_sync;

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

#[derive(Debug, Encode, Decode, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathInstruction {
    MoveTo,
    FaceTowards,
}

#[derive(Debug, Clone, Copy)]
pub struct PathPoint {
    pub point: Point3<f64>,
    pub instruction: PathInstruction,
}
impl PathPoint {
    /// min distance for robot to be considered at a point
    const AT_POINT_THRESHOLD: f64 = 0.2;

    /// min radians gap between robot  for robot to be considered facing towards a point
    const FACING_TOWARDS_THRESHOLD: f64 = 0.2;

    pub fn is_finished(&self, robot_pos: &Point2<f64>, robot_heading: &Point2<f64>) -> bool {
        match self.instruction {
            PathInstruction::MoveTo => {
                distance(&self.point.xz(), robot_pos) < Self::AT_POINT_THRESHOLD
            }

            PathInstruction::FaceTowards => {
                (self.point.xz() - robot_pos).angle(&robot_heading.coords)
                    < Self::FACING_TOWARDS_THRESHOLD
            }
        }
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
