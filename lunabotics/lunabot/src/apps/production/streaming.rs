use std::{
    io::{ErrorKind, Read},
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    time::{Duration, Instant},
};

use audio::audio_streaming;
use nalgebra::Vector2;
use openh264::{
    encoder::{Encoder, EncoderConfig},
    formats::{RgbSliceU8, YUVBuffer},
    OpenH264API,
};
use spin_sleep::SpinSleeper;
use tasker::parking_lot::RwLock;
use tracing::{error, info};

mod audio;

const CAMERA_COL_COUNT: usize = 3;
const CAMERA_ROW_COUNT: usize = 3;
pub const CAMERA_RESOLUTION: Vector2<u32> = Vector2::new(640, 360);
const KEYFRAME_INTERVAL: usize = 60;

struct ImgPtr(*mut u8, usize);

unsafe impl Send for ImgPtr {}
unsafe impl Sync for ImgPtr {}

static CAMERA_STREAMS: RwLock<ImgPtr> = RwLock::new(ImgPtr(std::ptr::null_mut(), 0));
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

    pub fn write(&mut self, mut src: impl Read) -> std::io::Result<()> {
        let Some(camera_streams) = CAMERA_STREAMS.try_read() else {
            return Ok(());
        };
        let ptr = camera_streams.0;
        let cam_x = self.index % CAMERA_COL_COUNT;
        let cam_y = self.index / CAMERA_COL_COUNT;
        let individual_frame_row_length = CAMERA_RESOLUTION.x as usize * 3;
        let global_frame_row_length = individual_frame_row_length * CAMERA_COL_COUNT;

        let start = (cam_y * global_frame_row_length * CAMERA_RESOLUTION.y as usize
            + cam_x * individual_frame_row_length) as usize;
        for y in 0..CAMERA_RESOLUTION.y as usize {
            let start = start + y * global_frame_row_length;
            let row = unsafe {
                &mut *std::ptr::slice_from_raw_parts_mut(
                    ptr.add(start),
                    individual_frame_row_length,
                )
            };
            src.read_exact(row)?;
        }
        Ok(())
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

pub fn start_streaming(mut lunabase_address: Option<IpAddr>) {
    audio_streaming(lunabase_address);

    std::thread::spawn(move || {
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
            camera_streams.1 = camera_frame_buffer.len();
            camera_streams.0 = Box::leak(camera_frame_buffer).as_mut_ptr();
        }
        let mut h264_enc = match Encoder::with_api_config(
            OpenH264API::from_source(),
            EncoderConfig::new()
                .set_bitrate_bps(400_000)
                // .enable_skip_frame(true)
                .max_frame_rate(24.0)
                // .rate_control_mode(RateControlMode::Timestamp)
                .set_multiple_thread_idc(4), // .sps_pps_strategy(SpsPpsStrategy::IncreasingId)
        ) {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to create H264 encoder: {e}");
                return;
            }
        };
        let udp = match UdpSocket::bind(SocketAddr::new(
            Ipv4Addr::UNSPECIFIED.into(),
            common::ports::CAMERAS,
        )) {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to bind to UDP socket: {e}");
                return;
            }
        };
        if let Err(e) = udp.set_nonblocking(true) {
            error!("Failed to set UDP socket to non-blocking: {e}");
            return;
        }

        let mut yuv_buffer = YUVBuffer::new(
            CAMERA_RESOLUTION.x as usize * CAMERA_COL_COUNT,
            CAMERA_RESOLUTION.y as usize * CAMERA_ROW_COUNT,
        );
        let mut now = Instant::now();
        let sleeper = SpinSleeper::default();
        let mut keyframe = 0usize;

        info!(
            "Starting camera streaming with resolution: {}x{}",
            CAMERA_RESOLUTION.x as usize * CAMERA_COL_COUNT,
            CAMERA_RESOLUTION.y as usize * CAMERA_ROW_COUNT
        );

        loop {
            let Some(ip) = lunabase_address else {
                if let Ok((_, addr)) = udp.recv_from(&mut [0u8; 1]) {
                    if addr.port() == common::ports::CAMERAS {
                        lunabase_address = Some(addr.ip());
                    }
                }
                continue;
            };
            now += now.elapsed();
            keyframe = (keyframe + 1) % KEYFRAME_INTERVAL;
            if keyframe == 0 {
                h264_enc.force_intra_frame();
            }

            {
                // Must be write to get exclusive access to all camera
                // streams, even though we are only reading from them
                let writer = CAMERA_STREAMS.write();
                let frame_data =
                    unsafe { &*std::ptr::slice_from_raw_parts_mut(writer.0, writer.1) };

                let rgb_slice = RgbSliceU8::new(
                    frame_data,
                    (
                        CAMERA_RESOLUTION.x as usize * CAMERA_COL_COUNT,
                        CAMERA_RESOLUTION.y as usize * CAMERA_ROW_COUNT,
                    ),
                );
                yuv_buffer.read_rgb(rgb_slice);
            }
            match h264_enc.encode(&yuv_buffer) {
                Ok(x) => {
                    for layer_i in 0..x.num_layers() {
                        let layer = x.layer(layer_i).unwrap();
                        for nal_i in 0..layer.nal_count() {
                            let nal = layer.nal_unit(nal_i).unwrap();

                            nal.chunks(1400).for_each(|chunk| {
                                if let Err(e) =
                                    udp.send_to(chunk, SocketAddr::new(ip, common::ports::CAMERAS))
                                {
                                    if e.kind() == ErrorKind::ConnectionRefused {
                                        if let Some(ip) = lunabase_address {
                                            if ip.is_loopback() {
                                                return;
                                            }
                                        }
                                    }
                                    lunabase_address = None;
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

            // 24 fps
            let delta = Duration::from_millis(1000 / 24).saturating_sub(now.elapsed());
            sleeper.sleep(delta);
        }
    });
}
