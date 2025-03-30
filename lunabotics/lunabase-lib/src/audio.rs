use std::net::UdpSocket;
use std::ptr::NonNull;

use common::{AUDIO_FRAME_SIZE, AUDIO_SAMPLE_RATE};
use godot::prelude::*;
use godot::{
    builtin::Vector2,
    classes::{AudioStreamGenerator, AudioStreamGeneratorPlayback, AudioStreamPlayer},
    global::godot_error,
    obj::{BaseMut, Gd, NewAlloc, NewGd},
};

use crate::LunabotConn;

pub struct AudioStreaming {
    // playback: Gd<AudioStreamGeneratorPlayback>,
    udp: Option<UdpSocket>,
    decoder: NonNull<u32>,
    audio_buffer: [f32; AUDIO_FRAME_SIZE as usize],
    udp_buffer: [u8; 4096],
    player: Gd<AudioStreamPlayer>,
    expected_i: Option<u32>,
}

impl AudioStreaming {
    pub fn new() -> Self {
        let mut generator = AudioStreamGenerator::new_gd();
        generator.set_mix_rate(AUDIO_SAMPLE_RATE as f32);
        // generator.set_buffer_length(0.8);
        let mut player = AudioStreamPlayer::new_alloc();
        player.set_autoplay(true);
        player.set_stream(&generator);

        let udp = UdpSocket::bind(std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), common::ports::AUDIO))
            .map(|udp| {
                if let Err(e) = udp.set_nonblocking(true) {
                    godot_error!("Failed to set UDP socket to non-blocking: {:?}", e);
                    None
                } else {
                    Some(udp)
                }
            })
            .map_err(|e| {
                godot_error!("Failed to bind to UDP socket: {:?}", e);
            })
            .ok()
            .flatten();

        Self {
            udp,
            decoder: unsafe {
                // let mut err_code = 0;
                // let dec_ptr = opus_static_sys::opus_decoder_create(AUDIO_SAMPLE_RATE as i32, 1, &mut err_code);
                
                // let Some(dec) = NonNull::new(dec_ptr) else {
                //     panic!("Failed to create Opus decoder: {}", err_code);
                // };

                // dec
                todo!()
            },
            audio_buffer: [0.0; AUDIO_FRAME_SIZE as usize],
            udp_buffer: [0u8; 4096],
            player,
            expected_i: None,
        }
    }

    pub fn poll(&mut self, mut base: BaseMut<LunabotConn>) {
        if !self.player.is_inside_tree() {
            base.add_child(&self.player);
        }
        if let Some(udp) = &self.udp {
            loop {
                match udp.recv(&mut self.udp_buffer) {
                    Ok(n) => {
                        if n == 4096 {
                            godot_warn!("Received a full buffer of audio data");
                        }
                        let i = u32::from_le_bytes([self.udp_buffer[0], self.udp_buffer[1], self.udp_buffer[2], self.udp_buffer[3]]);
                        if self.expected_i.is_none() {
                            self.expected_i = Some(i);
                        }
                        let expected_i = self.expected_i.unwrap();

                        if i != expected_i {
                            // let result = unsafe {
                            //     opus_decode_float(
                            //         self.decoder.as_ptr(),
                            //         std::ptr::null(),
                            //         0,
                            //         self.audio_buffer.as_mut_ptr(),
                            //         AUDIO_FRAME_SIZE as i32,
                            //         0
                            //     )
                            // };
                            // godot_error!("Missed packet: {}", result);
                        }

                        self.expected_i = Some(i + 1);
                        let result = unsafe {
                            // opus_decode_float(
                            //     self.decoder.as_ptr(),
                            //     self.udp_buffer.as_ptr().add(4).cast(),
                            //     n as i32 - 4,
                            //     self.audio_buffer.as_mut_ptr(),
                            //     AUDIO_FRAME_SIZE as i32,
                            //     0
                            // )
                            1
                        };
                        if result < 0 {
                            godot_error!("Failed to decode audio: {}", -result);
                        } else {
                            let mut playback = self.player.get_stream_playback().unwrap().cast::<AudioStreamGeneratorPlayback>();
                            if playback.get_frames_available() < result {
                                godot_warn!("Dropped {} frames", result - playback.get_frames_available());
                            }
                            playback.push_buffer(
                                &self.audio_buffer[..result as usize]
                                    .iter()
                                    .map(|&x| Vector2::new(x, x))
                                    .collect(),
                            );
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut
                        {
                            break;
                        }
                        godot_error!("Failed to receive stream data: {e}");
                        break;
                    }
                }
            }
        }
    }
}
