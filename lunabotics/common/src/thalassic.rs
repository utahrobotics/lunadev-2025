use bytemuck::{Pod, Zeroable};
use tracing::error;

use super::THALASSIC_CELL_COUNT;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Occupancy(u8);

impl Occupancy {
    pub fn occupied(self) -> bool {
        self.0 != 0
    }

    pub fn new(occupied: bool) -> Self {
        Self(occupied as u8)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ThalassicData {
    pub heightmap: [f32; THALASSIC_CELL_COUNT as usize],
    pub gradmap: [f32; THALASSIC_CELL_COUNT as usize],
    pub expanded_obstacle_map: [Occupancy; THALASSIC_CELL_COUNT as usize],
    point_count: usize,
}

impl Default for ThalassicData {
    fn default() -> Self {
        Self::zeroed()
    }
}
