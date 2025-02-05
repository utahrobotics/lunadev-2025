use std::{io::{Read, Write}, net::{SocketAddr, TcpStream}};

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

const THALASSIC_BUFFER_SIZE: usize = size_of::<ThalassicData>();

#[cfg(feature = "godot")]
pub fn lunabase_task(mut on_data: impl FnMut(&ThalassicData, &[godot::builtin::Vector3]) + Send + 'static) -> (impl Fn() + Send + Sync) {
    use std::net::Ipv4Addr;

    let parker = crossbeam::sync::Parker::new();
    let unparker = parker.unparker().clone();

    std::thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), crate::ports::DATAVIZ)) {
            Ok(listener) => listener,
            Err(e) => {
                godot::global::godot_error!("Failed to bind to dataviz port: {}", e);
                return;
            }
        };

        let mut data = ThalassicData::default();
        let mut points = vec![];
        let mut points_bytes: Vec<[f32; 3]> = vec![];
    
        loop {
            let stream = match listener.accept() {
                Ok((x, _)) => x,
                Err(e) => {
                    godot::global::godot_error!("Failed to accept connection: {}", e);
                    continue;
                }
            };
            let mut reader = brotli::Decompressor::new(
                stream, THALASSIC_BUFFER_SIZE,
            );
            loop {
                parker.park();
                if let Err(e) = reader.get_mut().write_all(&[0]) {
                    godot::global::godot_error!("Failed to write to stream: {}", e);
                    break;
                }
                if let Err(e) = reader.read_exact(bytemuck::bytes_of_mut(&mut data)) {
                    godot::global::godot_error!("Failed to read from stream: {}", e);
                    break;
                }
                points_bytes.resize(data.point_count, [0.0; 3]);
                if let Err(e) = reader.read_exact(bytemuck::cast_slice_mut(&mut points_bytes)) {
                    godot::global::godot_error!("Failed to read from stream: {}", e);
                    break;
                }
                points.clear();
                for &[x, y, z] in points_bytes.iter() {
                    points.push(godot::builtin::Vector3::new(x, y, z));
                }
                on_data(&data, &points);
            }
        }
    });

    move || {
        unparker.unpark();
    }
}

pub fn lunabot_task(address: SocketAddr, mut gen_data: impl FnMut(&mut ThalassicData, &mut Vec<nalgebra::Vector3<f32>>) + Send + 'static) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 1];
        let mut data = ThalassicData {
            heightmap: [0.0; THALASSIC_CELL_COUNT as usize],
            gradmap: [0.0; THALASSIC_CELL_COUNT as usize],
            expanded_obstacle_map: [Occupancy(0); THALASSIC_CELL_COUNT as usize],
            point_count: 0,
        };
        let mut points = vec![];
    
        loop {
            let stream = match TcpStream::connect(address) {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    continue;
                }
            };
            let mut writer = brotli::CompressorWriter::new(
                stream, THALASSIC_BUFFER_SIZE, 11, 22
            );
            loop {
                if let Err(e) = writer.get_mut().read_exact(&mut buffer) {
                    error!("Failed to read from stream: {}", e);
                    break;
                }
                match buffer[0] {
                    0 => {
                        gen_data(&mut data, &mut points);
                        data.point_count = points.len();
                        if let Err(e) = writer.write_all(bytemuck::bytes_of(&data)) {
                            error!("Failed to write to stream: {}", e);
                            break;
                        }
                        if let Err(e) = writer.write_all(bytemuck::cast_slice(&points)) {
                            error!("Failed to write to stream: {}", e);
                            break;
                        }
                        if let Err(e) = writer.flush() {
                            error!("Failed to flush stream: {}", e);
                            break;
                        }
                    }
                    _ => {
                        error!("Invalid command: {}", buffer[0]);
                        break;
                    }
                }
            }
        }
    });
}