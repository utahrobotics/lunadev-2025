use std::{
    cell::SyncUnsafeCell, io::{Cursor, ErrorKind, Read, Write}, net::{SocketAddr, UdpSocket}, sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    }, time::{Duration, Instant}
};

use anyhow::Context;
use cakap2::packet::Action;
use nalgebra::Vector2;
use openh264::{
    encoder::{Encoder, EncoderConfig, RateControlMode, SpsPpsStrategy},
    formats::{RgbSliceU8, YUVBuffer},
    OpenH264API,
};
use spin_sleep::SpinSleeper;
use urobotics::{
    log::error,
    parking_lot::{Mutex, RwLock},
};

use crate::teleop::PacketBuilder;

const CAMERA_COL_COUNT: usize = 3;
const CAMERA_ROW_COUNT: usize = 2;
pub const CAMERA_RESOLUTION: Vector2<u32> = Vector2::new(852, 480);

static CAMERA_STREAMS: RwLock<Vec<Box<[&SyncUnsafeCell<[u8]>]>>> = RwLock::new(vec![]);
static CAMERA_STREAM_LOCKS: OnceLock<Box<[AtomicBool]>> = OnceLock::new();

pub struct CameraStream {
    index: usize,
}

impl CameraStream {
    pub fn new(index: usize) -> Option<Self> {
        if index >= CAMERA_COL_COUNT * CAMERA_ROW_COUNT {
            error!("Camera index out of bounds: {}", index);
            return None;
        }
        if CAMERA_STREAM_LOCKS.get_or_init(|| {
            let mut locks = vec![];
            for _ in 0..CAMERA_COL_COUNT * CAMERA_ROW_COUNT {
                locks.push(AtomicBool::new(false));
            }
            locks.into_boxed_slice()
        })[index]
            .swap(true, Ordering::Relaxed)
        {
            error!("Camera stream {} already in use", index);
            return None;
        }

        Some(Self { index })
    }

    pub fn write(&mut self, mut src: impl Read) {
        let Some(camera_streams) = CAMERA_STREAMS.try_read() else {
            return;
        };
        let camera_stream = &camera_streams[self.index];
        for &row in camera_stream {
            let row = unsafe { &mut *row.get() };
            src.read(row).unwrap();
        }
    }
}

pub struct DownscaleRgbImageReader<'a> {
    rgb_image: &'a [u8],
    x_scale: f64,
    y_scale: f64,
    x: u32,
    y: u32,
    original_width: u32,
}

impl<'a> DownscaleRgbImageReader<'a> {
    pub fn new(rgb_image: &'a [u8], width: u32, height: u32) -> Self {
        debug_assert!(CAMERA_RESOLUTION.x <= width);
        debug_assert!(CAMERA_RESOLUTION.y <= height);
        Self {
            rgb_image,
            x_scale: width as f64 / CAMERA_RESOLUTION.x as f64,
            y_scale: height as f64 / CAMERA_RESOLUTION.y as f64,
            x: 0,
            y: 0,
            original_width: width,
        }
    }
}

impl<'a> Read for DownscaleRgbImageReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        assert_eq!(buf.len() % 3, 0);
        let mut n = 0;

        for i in 0..buf.len() / 3 {
            let scaled_i =
                ((self.y as f64 * self.y_scale).round() * self.original_width as f64 * 3.0
                    + (self.x as f64 * self.x_scale).round() * 3.0) as usize;
            buf[i * 3..i * 3 + 3].copy_from_slice(&self.rgb_image[scaled_i..scaled_i + 3]);
            n += 3;
            self.x += 1;
            if self.x >= CAMERA_RESOLUTION.x {
                self.x = 0;
                self.y += 1;
                if self.y >= CAMERA_RESOLUTION.y {
                    return Ok(n);
                }
            }
        }

        Ok(n)
    }
}

pub fn camera_streaming(lunabase_streaming_address: SocketAddr) -> anyhow::Result<()> {
    let camera_frame_buffer = vec![
        0u8;
        CAMERA_RESOLUTION.x as usize
            * CAMERA_RESOLUTION.y as usize
            * CAMERA_ROW_COUNT
            * CAMERA_COL_COUNT
            * 3
    ]
    .into_boxed_slice();
    {
        let mut camera_streams = CAMERA_STREAMS.write();
        let individual_frame_row_length = CAMERA_RESOLUTION.x as usize * 3;
        let global_frame_row_length = individual_frame_row_length * CAMERA_COL_COUNT;

        for cam_y in 0..CAMERA_ROW_COUNT {
            for cam_x in 0..CAMERA_COL_COUNT {
                let start = (cam_y * global_frame_row_length + cam_x * individual_frame_row_length)
                    as usize;
                let mut camera_stream = vec![];
                for y in 0..CAMERA_RESOLUTION.y as usize {
                    let start = start + y * global_frame_row_length;
                    let end = start + individual_frame_row_length;
                    camera_stream
                        .push(unsafe { std::mem::transmute(&camera_frame_buffer[start..end]) });
                }
                camera_streams.push(camera_stream.into_boxed_slice());
            }
        }
    }
    let mut h264_enc = Encoder::with_api_config(
        OpenH264API::from_source(),
        EncoderConfig::new()
            .set_bitrate_bps(1_000_000)
            .enable_skip_frame(true)
            .max_frame_rate(24.0)
            .rate_control_mode(RateControlMode::Bitrate)
            .sps_pps_strategy(SpsPpsStrategy::IncreasingId),
    )
    .context("Failed to create H264 encoder")?;
    let udp = UdpSocket::bind("0.0.0.0:0").context("Binding streaming UDP socket")?;
    udp.connect(lunabase_streaming_address)
        .context("Connecting to lunabase streaming address")?;
    // let mut tmp_output = std::io::BufWriter::new(std::fs::File::create("video.h264").unwrap());

    std::thread::spawn(move || {
        let mut yuv_buffer = YUVBuffer::new(
            CAMERA_RESOLUTION.x as usize * CAMERA_COL_COUNT,
            CAMERA_RESOLUTION.y as usize * CAMERA_ROW_COUNT,
        );
        let camera_frame_buffer: *const _ = Box::leak(camera_frame_buffer);
        let mut now = Instant::now();
        let sleeper = SpinSleeper::default();

        loop {
            now += now.elapsed();

            {
                // Must be write to get exclusive access to all camera
                // streams, even though we are only reading from them
                let _lock = CAMERA_STREAMS.write();
                let frame_data = unsafe { &*camera_frame_buffer };

                let rgb_slice = RgbSliceU8::new(
                    frame_data,
                    (
                        CAMERA_RESOLUTION.x as usize * CAMERA_COL_COUNT,
                        CAMERA_RESOLUTION.y as usize * CAMERA_ROW_COUNT,
                    ),
                );
                yuv_buffer.read_rgb(rgb_slice);
                match h264_enc.encode(&yuv_buffer) {
                    Ok(x) => {
                        for layer_i in 0..x.num_layers() {
                            let layer = x.layer(layer_i).unwrap();
                            for nal_i in 0..layer.nal_count() {
                                let nal = layer.nal_unit(nal_i).unwrap();
                                let mut buf = [0u8; 1400];
                                buf[2] = 1;
                                nal.chunks(1397).for_each(|chunk| {
                                    buf[3..3 + chunk.len()].copy_from_slice(chunk);
                                    if let Err(e) = udp.send(&buf) {
                                        if e.kind() == ErrorKind::ConnectionRefused {
                                            if lunabase_streaming_address.ip().is_loopback() {
                                                return;
                                            }
                                        }
                                        error!("Failed to send stream data to lunabase: {e}");
                                    }
                                });
                            }
                        }
                        // tmp_output.flush().unwrap();
                    }
                    Err(e) => {
                        error!("Failed to encode frame: {e}");
                    }
                }
            }

            // 24 fps
            sleeper.sleep(Duration::from_millis(1000 / 24).saturating_sub(now.elapsed()));
        }
    });

    Ok(())
}
