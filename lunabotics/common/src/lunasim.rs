use bitcode::{Decode, Encode};

use crate::BITCODE_BUFFER;

#[derive(Debug, Encode, Decode, Clone)]
pub enum FromLunasim {
    Accelerometer {
        id: usize,
        acceleration: [f32; 3],
    },
    Gyroscope {
        id: usize,
        axis: [f32; 3],
        angle: f32,
    },
    DepthMap(Box<[u32]>),
    ExplicitApriltag {
        robot_axis: [f32; 3],
        robot_angle: f32,
        robot_origin: [f32; 3],
    },
}

impl TryFrom<&[u8]> for FromLunasim {
    type Error = bitcode::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        BITCODE_BUFFER.with_borrow_mut(|buf| buf.decode(value))
    }
}

impl FromLunasim {
    pub fn encode<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T {
        BITCODE_BUFFER.with_borrow_mut(|buf| f(buf.encode(self)))
    }
}

#[derive(Debug, Encode, Decode, Clone)]
pub enum FromLunasimbot {
    PointCloud(Box<[[f32; 3]]>),
    Isometry {
        axis: [f32; 3],
        angle: f32,
        origin: [f32; 3],
    },
    Drive {
        left: f32,
        right: f32,
    },
}

impl TryFrom<&[u8]> for FromLunasimbot {
    type Error = bitcode::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        BITCODE_BUFFER.with_borrow_mut(|buf| buf.decode(value))
    }
}

impl FromLunasimbot {
    pub fn encode<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T {
        BITCODE_BUFFER.with_borrow_mut(|buf| f(buf.encode(self)))
    }
}
