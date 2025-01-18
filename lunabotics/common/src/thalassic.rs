use std::{io::{Read, Write}, net::{SocketAddr, TcpListener, TcpStream}};

use bytemuck::{Pod, Zeroable};
use crossbeam::sync::Parker;
use tracing::error;

pub const CELL_COUNT: u32 = 128 * 256;

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
    pub heightmap: [f32; CELL_COUNT as usize],
    pub gradmap: [f32; CELL_COUNT as usize],
    pub expanded_obstacle_map: [Occupancy; CELL_COUNT as usize],
}
const THALASSIC_BUFFER_SIZE: usize = size_of::<ThalassicData>();

pub fn lunabase_task(mut on_data: impl FnMut(&ThalassicData) + Send + 'static) -> &'static (impl Fn() + Send + Sync) {
    let parker = Parker::new();
    let unparker = parker.unparker().clone();

    std::thread::spawn(move || {
        let listener = match TcpListener::bind("0.0.0.0:20000") {
            Ok(listener) => listener,
            Err(e) => {
                error!("Failed to bind to port 20000: {}", e);
                return;
            }
        };

        let mut buffer = [0u8; THALASSIC_BUFFER_SIZE];
    
        loop {
            let stream = match listener.accept() {
                Ok((x, _)) => x,
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    continue;
                }
            };
            let mut reader = brotli::Decompressor::new(
                stream, THALASSIC_BUFFER_SIZE,
            );
            loop {
                parker.park();
                if let Err(e) = reader.get_mut().write_all(&[0]) {
                    error!("Failed to write to stream: {}", e);
                    break;
                }
                if let Err(e) = reader.read_exact(&mut buffer) {
                    error!("Failed to read from stream: {}", e);
                    break;
                }
                on_data(bytemuck::cast_ref(&buffer));
            }
        }
    });

    Box::leak(Box::new(move || {
        unparker.unpark();
    }))
}

pub fn lunabot_task(address: SocketAddr, mut gen_data: impl FnMut(&mut ThalassicData) + Send + 'static) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 1];
        let mut data = ThalassicData {
            heightmap: [0.0; CELL_COUNT as usize],
            gradmap: [0.0; CELL_COUNT as usize],
            expanded_obstacle_map: [Occupancy(0); CELL_COUNT as usize],
        };
    
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
                        gen_data(&mut data);
                        if let Err(e) = writer.write_all(bytemuck::bytes_of(&data)) {
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