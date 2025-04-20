#![feature(f16, try_blocks)]

use std::io::Write;

use bitcode::{Decode, Encode};
use embedded_common::{Actuator, ActuatorCommand};
use nalgebra::{distance, Point2, Point3};

// Taken from https://opus-codec.org/docs/opus_api-1.5/group__opus__encoder.html#gad2d6bf6a9ffb6674879d7605ed073e25
pub const AUDIO_FRAME_SIZE: u32 = 960;
pub const AUDIO_SAMPLE_RATE: u32 = 48000;
pub const THALASSIC_CELL_SIZE: f32 = 0.03125;
pub const THALASSIC_WIDTH: u32 = 128;
pub const THALASSIC_HEIGHT: u32 = 256;
pub const THALASSIC_CELL_COUNT: u32 = THALASSIC_WIDTH * THALASSIC_HEIGHT;

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
    LiftActuators(i8),
    BucketActuators(i8),
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
        FromLunabase::Steering(Steering::default()).write_code(&mut w)?;
        FromLunabase::TraverseObstacles.write_code(&mut w)?;
        FromLunabase::SoftStop.write_code(&mut w)?;
        Ok(())
    }

    pub fn set_lift_actuator(mut speed: f64) -> Self {
        speed = speed.clamp(-1.0, 1.0);
        let speed = if speed < 0.0 {
            (-speed * i8::MIN as f64) as i8
        } else {
            (speed * i8::MAX as f64) as i8
        };
        FromLunabase::LiftActuators(speed)
    }

    pub fn set_bucket_actuator(mut speed: f64) -> Self {
        speed = speed.clamp(-1.0, 1.0);
        let speed = if speed < 0.0 {
            (-speed * i8::MIN as f64) as i8
        } else {
            (speed * i8::MAX as f64) as i8
        };
        FromLunabase::BucketActuators(speed)
    }

    pub fn get_lift_actuator_commands(self) -> Option<[ActuatorCommand; 2]> {
        match self {
            FromLunabase::LiftActuators(value) => {
                Some(if value < 0 {
                    [
                        ActuatorCommand::backward(Actuator::M1),
                        ActuatorCommand::set_speed(value as f64 / i8::MIN as f64, Actuator::M1),
                    ]
                } else {
                    [
                        ActuatorCommand::forward(Actuator::M1),
                        ActuatorCommand::set_speed(value as f64 / i8::MAX as f64, Actuator::M1),
                    ]
                })
            }
            _ => None,
        }
    }

    pub fn get_bucket_actuator_commands(self) -> Option<[ActuatorCommand; 2]> {
        match self {
            FromLunabase::BucketActuators(value) => {
                Some(if value < 0 {
                    [
                        ActuatorCommand::forward(Actuator::M2),
                        ActuatorCommand::set_speed(value as f64 / i8::MIN as f64, Actuator::M2),
                    ]
                } else {
                    [
                        ActuatorCommand::backward(Actuator::M2),
                        ActuatorCommand::set_speed(value as f64 / i8::MAX as f64, Actuator::M2),
                    ]
                })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Encode, Decode, Clone, Copy)]
pub enum FromLunabot {
    RobotIsometry {
        origin: [f32; 3],
        quat: [f32; 4],
    },
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
pub struct Steering {
    left: i8,
    right: i8,
    weight: u16
}

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
    pub const DEFAULT_WEIGHT: f64 = 25.0;

    pub fn get_left_and_right(self) -> (f64, f64) {
        (
            if self.left < 0 {
                -(self.left as f64) / i8::MIN as f64
            } else {
                self.left as f64 / i8::MAX as f64
            },
            if self.right < 0 {
                -(self.right as f64) / i8::MIN as f64
            } else {
                self.right as f64 / i8::MAX as f64
            }
        )
    }

    pub fn get_weight(self) -> f64 {
        f16::from_bits(self.weight) as f64
    }

    pub fn new(mut left: f64, mut right: f64, weight: f64) -> Self {
        left = left.max(-1.0).min(1.0);
        right = right.max(-1.0).min(1.0);

        let left = if left < 0.0 {
            (-left * i8::MIN as f64) as i8
        } else {
            (left * i8::MAX as f64) as i8
        };
        let right = if right < 0.0 {
            (-right * i8::MIN as f64) as i8
        } else {
            (right * i8::MAX as f64) as i8
        };
        let weight = weight as f16;
        let weight = weight.to_bits();
        Self {
            left,
            right,
            weight
        }
    }
}

impl Default for Steering {
    fn default() -> Self {
        Self::new(0.0, 0.0, Self::DEFAULT_WEIGHT)
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
