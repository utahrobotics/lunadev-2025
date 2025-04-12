#![no_std]

pub const IMU_READING_DELAY_MS: u64 = 10;


#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Direction {
    Forward = 0,
    Backward = 1,
}


#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
/// Used to specify which actuator a command is meant for.
pub enum Actuator {
    /// the lift
    M1 = 0,
    /// the bucket
    M2 = 1
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ActuatorCommand {
    SetSpeed(u16, Actuator),
    SetDirection(Direction, Actuator),
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FromIMU {
    Reading(AngularRate, AccelerationNorm),
    NoDataReady,
    Error,
}

/// Radians per second
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct AngularRate {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Negative z = robot accelerating forward
/// 
/// In the default orientation, should be [0.0, -9.81, 0.0]
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct AccelerationNorm {
    pub x: f32,
    pub y: f32,
    pub z: f32
}

impl AccelerationNorm {
    pub fn serialize(&self) -> [u8; 12] {
        let mut bytes = [0u8; 12];
        bytes[0..4].copy_from_slice(&self.x.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.y.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.z.to_le_bytes());
        bytes
    }

    pub fn deserialize(bytes: [u8; 12]) -> Result<Self, &'static str> {
        let x = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let y = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let z = f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        Ok(Self { x, y, z })
    }
}

impl AngularRate {
    pub fn serialize(&self) -> [u8; 12] {
        let mut bytes = [0u8; 12];
        bytes[0..4].copy_from_slice(&self.x.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.y.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.z.to_le_bytes());
        bytes
    }

    pub fn deserialize(bytes: [u8; 12]) -> Result<Self, &'static str> {
        let x = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let y = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let z = f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        Ok(Self { x, y, z })
    }
}

impl FromIMU {
    pub fn serialize(&self) -> [u8; 25] {
        let mut bytes = [0u8; 25];
        match self {
            FromIMU::Reading(rate, accel ) => {
                bytes[0] = 0;
                bytes[1..=12].copy_from_slice(&rate.serialize());
                bytes[13..].copy_from_slice(&accel.serialize());

            }
            FromIMU::NoDataReady => {
                bytes[0] = 2;
            }
            FromIMU::Error => {
                bytes[0] = 3;
            }
        }
        bytes
    }

    pub fn deserialize(bytes: [u8; 25]) -> Result<Self, &'static str> {
        let rate_bytes: [u8; 12] = bytes[1..=12].as_ref().try_into().map_err(|_|"failed to deserialize FromIMU")?;
        let accel_bytes: [u8; 12] = bytes[13..].as_ref().try_into().map_err(|_|"failed to deserialize FromIMU")?;

        
        match bytes[0] {
            0 => {
                let rate = AngularRate::deserialize(rate_bytes)?;
                let accel = AccelerationNorm::deserialize(accel_bytes)?;
                Ok(FromIMU::Reading(rate, accel))
            }
            2 => Ok(FromIMU::NoDataReady),
            3 => Ok(FromIMU::Error),
            _ => Err("Invalid variant tag")
        }
    }
}
impl ActuatorCommand {
    pub fn deserialize(bytes: [u8; 4]) -> Result<Self, &'static str> {
        let actuator = {
            if bytes[3] == Actuator::M1 as u8 {
                Actuator::M1
            } else if bytes[3] == Actuator::M2 as u8{
                Actuator::M2
            } else {
                return Err("Unknown actuator specifier (not m1 or m2)");
            }
        };
        match bytes[0] {
            tag if tag == 0 => {
                let speed = u16::from_le_bytes(
                    bytes[1..=2]
                        .try_into()
                        .map_err(|_| "Wrong number of bytes in actuator command")?,
                );
                Ok(ActuatorCommand::SetSpeed(speed, actuator))
            }
            tag if tag == 1 => {
                let dir = match bytes[1] {
                    0 => Direction::Forward,
                    1 => Direction::Backward,
                    _ => return Err("Invalid direction value"),
                };
                Ok(ActuatorCommand::SetDirection(dir, actuator))
            }
            _ => Err("Invalid variant tag"),
        }
    }

    pub fn serialize(&self) -> [u8; 4] {
        match self {
            ActuatorCommand::SetSpeed(speed, actuator) => {
                let mut bytes = [0u8; 4];
                bytes[0] = 0;
                bytes[1..=2].copy_from_slice(&speed.to_le_bytes());
                bytes[3] = *actuator as u8;
                bytes
            }
            ActuatorCommand::SetDirection(dir, actuator) => {
                let mut bytes = [0u8; 4];
                bytes[0] = 1;
                bytes[1] = *dir as u8;
                bytes[2] = 0;
                bytes[3] = *actuator as u8;
                bytes
            }
        }
    }
}