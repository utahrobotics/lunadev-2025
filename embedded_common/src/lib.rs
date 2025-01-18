#![no_std]

use defmt::Format;

#[derive(Clone, Copy, Debug, Format)]
pub enum FromIMU {
    AngularRateReading(AngularRate),
    AccellerationNormReading(AccelerationNorm),
}

#[derive(Clone, Copy, Debug, Format)]
pub struct AngularRate {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Clone, Copy, Debug, Format)]
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
    pub fn serialize(&self) -> [u8; 13] {
        let mut bytes = [0u8; 13];
        match self {
            FromIMU::AngularRateReading(rate) => {
                bytes[0] = 0;
                bytes[1..].copy_from_slice(&rate.serialize());
            }
            FromIMU::AccellerationNormReading(accel) => {
                bytes[0] = 1;
                bytes[1..].copy_from_slice(&accel.serialize());
            }
        }
        bytes
    }

    pub fn deserialize(bytes: [u8; 13]) -> Result<Self, &'static str> {
        let variant_bytes = [bytes[1], bytes[2], bytes[3], bytes[4], 
                           bytes[5], bytes[6], bytes[7], bytes[8],
                           bytes[9], bytes[10], bytes[11], bytes[12]];
        
        match bytes[0] {
            0 => Ok(FromIMU::AngularRateReading(AngularRate::deserialize(variant_bytes)?)),
            1 => Ok(FromIMU::AccellerationNormReading(AccelerationNorm::deserialize(variant_bytes)?)),
            _ => Err("Invalid variant tag")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_imu_angular_rate() {
        let original = FromIMU::AngularRateReading(AngularRate {
            x: 3.14159,
            y: -1.5708,
            z: 0.7854,
        });
        
        let serialized = original.serialize();
        let deserialized = FromIMU::deserialize(serialized).unwrap();
        
        match (original, deserialized) {
            (FromIMU::AngularRateReading(orig), FromIMU::AngularRateReading(des)) => {
                assert_eq!(orig.x, des.x);
                assert_eq!(orig.y, des.y);
                assert_eq!(orig.z, des.z);
            },
            _ => panic!("Wrong variant after deserialization"),
        }
    }

    #[test]
    fn test_from_imu_acceleration() {
        let original = FromIMU::AccellerationNormReading(AccelerationNorm {
            x: 1.0,
            y: -2.5,
            z: 0.5,
        });
        
        let serialized = original.serialize();
        let deserialized = FromIMU::deserialize(serialized).unwrap();
        
        match (original, deserialized) {
            (FromIMU::AccellerationNormReading(orig), FromIMU::AccellerationNormReading(des)) => {
                assert_eq!(orig.x, des.x);
                assert_eq!(orig.y, des.y);
                assert_eq!(orig.z, des.z);
            },
            _ => panic!("Wrong variant after deserialization"),
        }
    }

    #[test]
    fn test_invalid_variant() {
        let mut invalid_bytes = [0u8; 13];
        invalid_bytes[0] = 2;
        
        assert!(FromIMU::deserialize(invalid_bytes).is_err());
    }
}