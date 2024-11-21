use std::{cell::SyncUnsafeCell, io::{Cursor, Read, Write}, sync::{atomic::{AtomicBool, Ordering}, OnceLock}, time::{Duration, Instant}};

use anyhow::Context;
use nalgebra::Vector2;
use openh264::{encoder::Encoder, formats::{RgbSliceU8, YUVBuffer}};
use spin_sleep::SpinSleeper;
use urobotics::{log::error, parking_lot::{Mutex, RwLock}};

const CAMERA_COL_COUNT: usize = 3;
const CAMERA_ROW_COUNT: usize = 2;
pub const CAMERA_RESOLUTION: Vector2<u32> = Vector2::new(640, 480);
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
        })[index].swap(true, Ordering::Relaxed) {
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
            let row = unsafe {
                &mut *row.get()
            };
            src.read(row).unwrap();
        }
        println!("Wrote");
    }
}


pub fn camera_streaming() -> anyhow::Result<()> {
    let camera_frame_buffer = vec![0u8; CAMERA_RESOLUTION.x as usize * CAMERA_RESOLUTION.y as usize * CAMERA_ROW_COUNT * CAMERA_COL_COUNT * 3].into_boxed_slice();
    {
        let mut camera_streams = CAMERA_STREAMS.write();
        for cam_y in 0..CAMERA_ROW_COUNT {
            for cam_x in 0..CAMERA_COL_COUNT {
                let start = (cam_y * CAMERA_RESOLUTION.x as usize * CAMERA_COL_COUNT * 3 + cam_x * CAMERA_RESOLUTION.x as usize) as usize;
                let mut camera_stream = vec![];
                for y in 0..CAMERA_RESOLUTION.y as usize {
                    let start = start + y * (CAMERA_RESOLUTION.x as usize * CAMERA_COL_COUNT * 3);
                    let end = start + CAMERA_RESOLUTION.x as usize * 3;
                    camera_stream.push(
                        unsafe { std::mem::transmute(&camera_frame_buffer[start..end]) }
                    );
                }
                camera_streams.push(camera_stream.into_boxed_slice());
            }
        }
    }
    let mut h264_enc = Encoder::new().context("Failed to create H264 encoder")?;
    let mut tmp_output = std::io::BufWriter::new(std::fs::File::create("video.h264").unwrap());

    std::thread::spawn(move || {
        let mut yuv_buffer = YUVBuffer::new(CAMERA_RESOLUTION.x as usize * CAMERA_COL_COUNT, CAMERA_RESOLUTION.y as usize * CAMERA_ROW_COUNT);
        let camera_frame_buffer: *const _ = Box::leak(camera_frame_buffer);
        let mut now = Instant::now();
        let sleeper = SpinSleeper::default();
        let mut count = 0usize;
        
        loop {
            now += now.elapsed();

            {
                println!("Locked");
                // Must be write to get exclusive access to all camera
                // streams, even though we are only reading from them
                let _lock = CAMERA_STREAMS.write();
                let frame_data = unsafe { &*camera_frame_buffer };

                {
                    let rgb_img = urobotics_apriltag::image::ImageBuffer::from_raw(CAMERA_RESOLUTION.x * CAMERA_COL_COUNT as u32, CAMERA_RESOLUTION.y * CAMERA_ROW_COUNT as u32, frame_data.to_vec()).unwrap();
                    urobotics_apriltag::image::DynamicImage::ImageRgb8(rgb_img).save("frame.png").unwrap();
                    count += 1;
                    println!("{}x{}", CAMERA_RESOLUTION.x * CAMERA_COL_COUNT as u32, CAMERA_RESOLUTION.y * CAMERA_ROW_COUNT as u32);
                    if count > 3 {
                        return;
                    }
                }

                // let rgb_slice = RgbSliceU8::new(frame_data, (CAMERA_RESOLUTION.x as usize * CAMERA_COL_COUNT, CAMERA_RESOLUTION.y as usize * CAMERA_ROW_COUNT));
                // yuv_buffer.read_rgb(rgb_slice);
                // const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
                // match h264_enc.encode(&yuv_buffer) {
                //     Ok(x) => {
                //         for layer_i in 0..x.num_layers() {
                //             let layer = x.layer(layer_i).unwrap();
                //             for nal_i in 0..layer.nal_count() {
                //                 let nal = layer.nal_unit(nal_i).unwrap();
                //                 tmp_output.write_all(START_CODE).unwrap();
                //                 tmp_output.write_all(nal).unwrap();
                //             }
                //         }
                //         // x.write(&mut tmp_output).unwrap();
                //         tmp_output.flush().unwrap();
                //     }
                //     Err(e) => {
                //         error!("Failed to encode frame: {e}");
                //     }
                // }
                println!("Unlocked");
            }

            // 24 fps
            sleeper.sleep(Duration::from_millis(1200).saturating_sub(now.elapsed()));
        }
    });

    Ok(())
}