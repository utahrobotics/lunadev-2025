#![no_std]

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
        let rate_bytes: [u8; 12] = bytes[1..=12].as_ref().try_into().map_err(|err|"failed to deserialize FromIMU")?;
        let accel_bytes: [u8; 12] = bytes[13..].as_ref().try_into().map_err(|err|"failed to deserialize FromIMU")?;

        
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