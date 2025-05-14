use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
};

use brotli::{CompressorWriter, Decompressor};
use bytemuck::{Pod, Zeroable};
use nalgebra::Vector3;

use super::THALASSIC_CELL_COUNT;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ThalassicData {
    pub heightmap: [f16; THALASSIC_CELL_COUNT as usize],
}
unsafe impl Pod for ThalassicData {}
unsafe impl Zeroable for ThalassicData {}

impl Default for ThalassicData {
    fn default() -> Self {
        Self::zeroed()
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum SyncDataTypes {
    ThalassicData = 0,
    /// A path is a list of [f16; 3], prefixed by the number of points as [`u16`].
    Path = 1,
}

impl TryFrom<u8> for SyncDataTypes {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::ThalassicData),
            1 => Ok(Self::Path),
            _ => Err(()),
        }
    }
}

const BROTLI_BUFFER_SIZE: usize = 4096;

pub fn lunabot_task(
    request_data: impl Fn(&mut Vec<Vector3<f16>>, &mut ThalassicData) -> (bool, bool) + Sync + 'static,
) {
    let request_data: &_ = Box::leak(Box::new(request_data));
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(SocketAddr::new(
            Ipv4Addr::UNSPECIFIED.into(),
            crate::ports::LUNABASE_SYNC_DATA,
        )) {
            Ok(listener) => listener,
            Err(e) => {
                tracing::error!(
                    "Failed to bind to port {}: {}",
                    crate::ports::LUNABASE_SYNC_DATA,
                    e
                );
                return;
            }
        };

        tracing::info!("Started lunabase sync data task");

        loop {
            match listener.accept() {
                Ok((peer, _)) => {
                    std::thread::spawn(move || {
                        let mut peer = CompressorWriter::new(peer, BROTLI_BUFFER_SIZE, 11, 22);
                        let mut path = Vec::new();
                        let mut thalassic_data = ThalassicData::default();

                        loop {
                            let (path_updated, thalassic_updated) =
                                request_data(&mut path, &mut thalassic_data);

                            let result: std::io::Result<()> = try {
                                if path_updated {
                                    peer.write_all(&[SyncDataTypes::Path as u8])?;
                                    peer.write_all(&(path.len() as u16).to_le_bytes())?;
                                    for point in path.drain(..) {
                                        peer.write_all(&point.x.to_le_bytes())?;
                                        peer.write_all(&point.y.to_le_bytes())?;
                                        peer.write_all(&point.z.to_le_bytes())?;
                                    }
                                }
                                if thalassic_updated {
                                    peer.write_all(&[SyncDataTypes::ThalassicData as u8])?;
                                    peer.write_all(bytemuck::bytes_of(&thalassic_data))?;
                                }
                                peer.flush()?;
                            };

                            if let Err(_) = result {
                                break;
                            }
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to accept connection: {}", e);
                }
            }
        }
    });
}

pub fn lunabase_task(
    lunabot_addr: IpAddr,
    mut on_thalassic: impl FnMut(&ThalassicData) + Send + 'static,
    mut on_path: impl FnMut(&[[f32; 3]]) + Send + 'static,
    mut on_error: impl FnMut(std::io::Error) + Send + 'static,
) {
    std::thread::spawn(move || {
        let mut thalassic_data = ThalassicData::default();
        let mut path = Vec::new();
        loop {
            let stream = match TcpStream::connect(SocketAddr::new(
                lunabot_addr,
                crate::ports::LUNABASE_SYNC_DATA,
            )) {
                Ok(stream) => stream,
                Err(e) => {
                    on_error(e);
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    continue;
                }
            };
            let mut stream = Decompressor::new(stream, BROTLI_BUFFER_SIZE);

            loop {
                let result: std::io::Result<()> = try {
                    let mut buf = [0; 1];
                    stream.read_exact(&mut buf)?;
                    match SyncDataTypes::try_from(buf[0]) {
                        Ok(SyncDataTypes::ThalassicData) => {
                            stream.read_exact(bytemuck::bytes_of_mut(&mut thalassic_data))?;
                            on_thalassic(&thalassic_data);
                        }
                        Ok(SyncDataTypes::Path) => {
                            let mut buf = [0; 2];
                            stream.read_exact(&mut buf)?;
                            let len = u16::from_le_bytes(buf) as usize;
                            path.resize(len, [0.0; 3]);
                            let mut tmp_f = [0u8; 2];

                            for point in path.iter_mut() {
                                for i in 0..3 {
                                    stream.read_exact(&mut tmp_f)?;
                                    point[i] = f16::from_le_bytes(tmp_f) as f32;
                                }
                            }
                            on_path(&path);
                        }
                        Err(()) => {
                            on_error(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid sync data type",
                            ));
                        }
                    }
                };

                if let Err(e) = result {
                    on_error(e);
                    break;
                }
            }
        }
    });
}
