use std::{io::Write, ops::Deref, time::Duration};

use common::{FromLunabase, LunabotStage, Steering, THALASSIC_CELL_COUNT};
use embedded_common::ActuatorCommand;
use nalgebra::{Isometry3, Quaternion, UnitQuaternion, Vector2, Vector3};
use thalassic::Occupancy;

pub const AI_HEARTBEAT_RATE: Duration = Duration::from_millis(50);
pub const HOST_HEARTBEAT_LISTEN_RATE: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParseError {
    NotEnoughBytes {
        bytes_needed: usize
    },
    InvalidData
}

#[repr(u8)]
enum FromHostHeader {
    BaseIsometry = 0,
    FromLunabase = 1,
    ActuatorReadings = 2,
    ThalassicData = 3
}

#[derive(Debug)]
pub enum FromHost {
    BaseIsometry {
        isometry: Isometry3<f64>
    },
    FromLunabase {
        msg: FromLunabase
    },
    ActuatorReadings {
        lift: u16,
        bucket: u16
    },
    ThalassicData {
        obstacle_map: Box<[Occupancy; THALASSIC_CELL_COUNT as usize]>
    }
}

impl FromHost {
    pub fn write_into(&self, writer: &mut impl Write) -> std::io::Result<()> {
        match self {
            FromHost::BaseIsometry { isometry } => {
                writer.write_all(&[FromHostHeader::BaseIsometry as u8])?;
                writer.write_all(bytemuck::bytes_of(&isometry.translation))?;
                writer.write_all(bytemuck::bytes_of(&isometry.rotation))?;
            }
            FromHost::FromLunabase { msg } => {
                writer.write_all(&[FromHostHeader::FromLunabase as u8])?;
                let bytes = bitcode::encode(msg);
                writer.write_all(&(bytes.len() as u16).to_ne_bytes())?;
                writer.write_all(&bytes)?;
            }
            FromHost::ActuatorReadings { lift, bucket: tilt } => {
                writer.write_all(&[FromHostHeader::ActuatorReadings as u8])?;
                writer.write_all(&lift.to_ne_bytes())?;
                writer.write_all(&tilt.to_ne_bytes())?;
            }
            FromHost::ThalassicData { obstacle_map } => {
                writer.write_all(&[FromHostHeader::ThalassicData as u8])?;
                writer.write_all(bytemuck::bytes_of(obstacle_map.deref()))?;
            }
        }
        writer.flush()
    }

    pub fn parse(bytes: &[u8]) -> Result<(Self, usize), ParseError> {
        if bytes.is_empty() {
            return Err(ParseError::NotEnoughBytes { bytes_needed: 1 });
        }
        match bytes[0] {
            x if x == FromHostHeader::BaseIsometry as u8 => {
                if bytes.len() < 57 {
                    return Err(ParseError::NotEnoughBytes { bytes_needed: 57 });
                }
                let mut origin = Vector3::<f64>::default();
                bytemuck::bytes_of_mut(&mut origin).copy_from_slice(&bytes[1..25]);
                let mut quat = Quaternion::<f64>::default();
                bytemuck::bytes_of_mut(&mut quat).copy_from_slice(&bytes[25..57]);
                Ok((
                    Self::BaseIsometry { isometry: Isometry3::from_parts(origin.into(), UnitQuaternion::new_unchecked(quat))},
                    57
                ))
            }
            x if x == FromHostHeader::FromLunabase as u8 => {
                if bytes.len() < 3 {
                    return Err(ParseError::NotEnoughBytes { bytes_needed: 3 });
                }
                let size = u16::from_ne_bytes([bytes[1], bytes[2]]) as usize;
                if bytes.len() < size + 3 {
                    return Err(ParseError::NotEnoughBytes { bytes_needed: size + 1 });
                }

                let Ok(msg) = bitcode::decode(&bytes[3..(size + 3)]) else {
                    return Err(ParseError::InvalidData);
                };

                Ok((
                    Self::FromLunabase { msg },
                    size + 3
                ))
            }
            x if x == FromHostHeader::ActuatorReadings as u8 => {
                if bytes.len() < 5 {
                    return Err(ParseError::NotEnoughBytes { bytes_needed: 5 });
                }
                let lift = u16::from_ne_bytes([bytes[1], bytes[2]]);
                let tilt = u16::from_ne_bytes([bytes[3], bytes[4]]);

                Ok((
                    Self::ActuatorReadings { lift, bucket: tilt },
                    5
                ))
            }
            x if x == FromHostHeader::ThalassicData as u8 => {
                if bytes.len() < THALASSIC_CELL_COUNT as usize * 4 + 1 {
                    return Err(ParseError::NotEnoughBytes { bytes_needed: THALASSIC_CELL_COUNT as usize * 4 + 1 });
                }

                let mut obstacle_map = Box::new([Occupancy::UNKNOWN; THALASSIC_CELL_COUNT as usize]);
                bytemuck::bytes_of_mut(&mut *obstacle_map).copy_from_slice(&bytes[1..THALASSIC_CELL_COUNT as usize * 4 + 1]);

                Ok((
                    Self::ThalassicData { obstacle_map },
                    THALASSIC_CELL_COUNT as usize * 4 + 1
                ))
            }
            _ => {
                Err(ParseError::InvalidData)
            }
        }
    }
}

#[repr(u8)]
enum FromAIHeader {
    SetSteering = 0,
    SetActuators = 1,
    Heartbeat = 2,
    StartPercuss = 3,
    StopPercuss = 4,
    SetStage = 5,
    RequestThalassic = 6,
    PathFound = 7
}

#[derive(Debug)]
pub enum FromAI {
    SetSteering(Steering),
    SetActuators(ActuatorCommand),
    Heartbeat,
    StartPercuss,
    StopPercuss,
    SetStage(LunabotStage),
    RequestThalassic,
    PathFound(Vec<Vector2<f64>>)
}

impl FromAI {
    pub fn write_into(&self, writer: &mut impl Write) -> std::io::Result<()> {
        match self {
            FromAI::SetSteering(steering) => {
                writer.write_all(&[FromAIHeader::SetSteering as u8])?;
                writer.write_all(bytemuck::bytes_of(steering))?;
            }
            FromAI::SetActuators(cmd) => {
                writer.write_all(&[FromAIHeader::SetActuators as u8])?;
                writer.write_all(&cmd.serialize())?;
            }
            FromAI::Heartbeat => {
                writer.write_all(&[FromAIHeader::Heartbeat as u8])?;
            }
            FromAI::StartPercuss => {
                writer.write_all(&[FromAIHeader::StartPercuss as u8])?;
            }
            FromAI::StopPercuss => {
                writer.write_all(&[FromAIHeader::StopPercuss as u8])?;
            }
            FromAI::SetStage(stage) => {
                writer.write_all(&[FromAIHeader::SetStage as u8])?;
                writer.write_all(&[*stage as u8])?;
            }
            FromAI::RequestThalassic => {
                writer.write_all(&[FromAIHeader::RequestThalassic as u8])?;
            }
            FromAI::PathFound(path) => {
                writer.write_all(&[FromAIHeader::PathFound as u8])?;
                writer.write_all(&(path.len() as u16).to_ne_bytes())?;
                for p in path {
                    let bytes = bytemuck::bytes_of(p);

                    writer.write_all(bytes)?;
                }
            }
        }
        writer.flush()
    }

    pub fn parse(bytes: &[u8]) -> Result<(Self, usize), ParseError> {
        if bytes.is_empty() {
            return Err(ParseError::NotEnoughBytes { bytes_needed: 1 });
        }
        match bytes[0] {
            x if x == FromAIHeader::SetSteering as u8 => {
                if bytes.len() < size_of::<Steering>() + 1 {
                    return Err(ParseError::NotEnoughBytes { bytes_needed: size_of::<Steering>() + 1 });
                }

                let mut steering = Steering::default();
                bytemuck::bytes_of_mut(&mut steering).copy_from_slice(&bytes[1..(size_of::<Steering>() + 1)]);
                
                Ok((
                    Self::SetSteering(steering),
                    size_of::<Steering>() + 1
                ))
            }
            x if x == FromAIHeader::SetActuators as u8 => {
                if bytes.len() < 6 {
                    return Err(ParseError::NotEnoughBytes { bytes_needed: 6 });
                }

                let Ok(cmd) = ActuatorCommand::deserialize([bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]]) else {
                    return Err(ParseError::InvalidData);
                };

                Ok((
                    Self::SetActuators(cmd),
                    6
                ))
            }
            x if x == FromAIHeader::Heartbeat as u8 => {
                Ok((
                    Self::Heartbeat,
                    1
                ))
            }
            x if x == FromAIHeader::StartPercuss as u8 => {
                Ok((
                    Self::StartPercuss,
                    1
                ))
            }
            x if x == FromAIHeader::StopPercuss as u8 => {
                Ok((
                    Self::StopPercuss,
                    1
                ))
            }
            x if x == FromAIHeader::SetStage as u8 => {
                if bytes.len() < 2 {
                    return Err(ParseError::NotEnoughBytes { bytes_needed: 2 });
                }
                Ok((
                    Self::SetStage(LunabotStage::try_from(bytes[1]).map_err(|_| ParseError::InvalidData)?),
                    2
                ))
            }
            x if x == FromAIHeader::RequestThalassic as u8 => {
                Ok((
                    Self::RequestThalassic,
                    1
                ))
            }
            x if x == FromAIHeader::PathFound as u8 => {
                if bytes.len() < 3 {
                    return Err(ParseError::NotEnoughBytes { bytes_needed: 3 });
                }
                let count = u16::from_ne_bytes([bytes[1], bytes[2]]) as usize;
                if bytes.len() < 3 + count * 16 {
                    return Err(ParseError::NotEnoughBytes { bytes_needed: 3 + count * 16 });
                }
                let mut path = Vec::<Vector2<f64>>::with_capacity(count);
                let mut bytes = &bytes[3..];
                for _ in 0..count {
                    let mut p: Vector2<f64> = Vector2::new(0.0, 0.0);
                    bytemuck::bytes_of_mut(&mut p).copy_from_slice(&bytes[0..16]);
                    bytes = &bytes[16..];
                    path.push(p);
                }
                Ok((
                    Self::PathFound(path),
                    3 + count * 16
                ))
            }
            _ => {
                Err(ParseError::InvalidData)
            }
        }
    }
}