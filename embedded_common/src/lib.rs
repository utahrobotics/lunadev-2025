#![no_std]

use defmt::Format;


/// Message to be sent over USB from rp2040 to lunabot
#[derive(Clone, Copy, Debug, Format)]
pub enum FromIMU {
    AngularRateReading(AngularRate),
    AccellerationNormReading(AccelerationNorm),
}


/// Use Radians per Second
#[derive(Clone, Copy, Debug, Format)]
pub struct AngularRate {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Normalized Acceleration Vector, m/s
#[derive(Clone, Copy, Debug, Format)]
pub struct AccelerationNorm {
    pub x: f32,
    pub y: f32,
    pub z: f32
}

impl AccelerationNorm {
    /// Serializes the acceleration data into a fixed-size byte array
    pub fn serialize(&self) -> [u8; 12] {
        let mut bytes = [0u8; 12];
        bytes[0..4].copy_from_slice(&self.x.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.y.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.z.to_le_bytes());
        
        bytes
    }

    /// Deserializes from bytes into an AccelerationNorm struct
    pub fn deserialize(bytes: [u8; 12]) -> Result<Self, &'static str> {
        // Convert bytes back to f32s
        let x = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let y = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let z = f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        
        Ok(Self { x, y, z })
    }
}

impl AngularRate {

    /// Serializes the angular rate data into a fixed-size byte array
    pub fn serialize(&self) -> [u8; 12] {
        let mut bytes = [0u8; 12];
        bytes[0..4].copy_from_slice(&self.x.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.y.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.z.to_le_bytes());
        
        bytes
    }

    /// Deserializes from fixed size bytes into an AngularRate struct
    pub fn deserialize(bytes: [u8; 12]) -> Result<Self, &'static str> {
        let x = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let y = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let z = f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        Ok(Self { x, y, z })
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acceleration_serialization_roundtrip() {
        let original = AccelerationNorm {
            x: 1.0,
            y: -2.5,
            z: 0.5,
        };
        
        let serialized = original.serialize();
        let deserialized = AccelerationNorm::deserialize(serialized).unwrap();
        
        assert_eq!(original.x, deserialized.x);
        assert_eq!(original.y, deserialized.y);
        assert_eq!(original.z, deserialized.z);
    }

    #[test]
    fn test_angular_rate_serialization_roundtrip() {
        let original = AngularRate {
            x: 3.14159,
            y: -1.5708,
            z: 0.7854,
        };
        
        let serialized = original.serialize();
        let deserialized = AngularRate::deserialize(serialized).unwrap();
        
        assert_eq!(original.x, deserialized.x);
        assert_eq!(original.y, deserialized.y);
        assert_eq!(original.z, deserialized.z);
    }

    #[test]
    fn test_zeros() {
        let original = AccelerationNorm {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        
        let serialized = original.serialize();
        let deserialized = AccelerationNorm::deserialize(serialized).unwrap();
        
        assert_eq!(original.x, deserialized.x);
        assert_eq!(original.y, deserialized.y);
        assert_eq!(original.z, deserialized.z);
    }

    #[test]
    fn test_extremes() {
        let original = AngularRate {
            x: f32::MAX,
            y: f32::MIN,
            z: f32::EPSILON,
        };
        
        let serialized = original.serialize();
        let deserialized = AngularRate::deserialize(serialized).unwrap();
        
        assert_eq!(original.x, deserialized.x);
        assert_eq!(original.y, deserialized.y);
        assert_eq!(original.z, deserialized.z);
    }
}
