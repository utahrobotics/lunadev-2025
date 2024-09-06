use bitcode::{Decode, Encode};
use byteable::{FillByteVecBitcode, IntoBytes, IntoBytesSliceBitcode};

#[derive(Debug, Encode, Decode, Clone, FillByteVecBitcode, IntoBytesSliceBitcode, IntoBytes)]
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
        __FromLunasim_BUFFER.with_borrow_mut(|queue| {
            if queue.is_empty() {
                queue.push_back(Default::default());
            }
            queue.front_mut().unwrap().decode(value)
        })
    }
}

#[derive(Debug, Encode, Decode, Clone, FillByteVecBitcode, IntoBytesSliceBitcode, IntoBytes)]
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
        __FromLunasimbot_BUFFER.with_borrow_mut(|queue| {
            if queue.is_empty() {
                queue.push_back(Default::default());
            }
            queue.front_mut().unwrap().decode(value)
        })
    }
}
