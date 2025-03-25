use std::net::UdpSocket;
use std::ptr::NonNull;

use common::{AUDIO_FRAME_SIZE, AUDIO_SAMPLE_RATE};
use godot::prelude::*;
use godot::{
    builtin::Vector2,
    classes::{AudioStreamGenerator, AudioStreamGeneratorPlayback, AudioStreamPlayer},
    global::{godot_error, godot_print},
    obj::{BaseMut, Gd, NewAlloc, NewGd},
};
use opus_static_sys::{opus_decode_float, OpusDecoder};

use crate::LunabotConn;

pub struct AudioStreaming {
    // playback: Gd<AudioStreamGeneratorPlayback>,
    udp: Option<UdpSocket>,
    decoder: NonNull<OpusDecoder>,
    audio_buffer: [f32; AUDIO_FRAME_SIZE as usize],
    udp_buffer: [u8; 4096],
    player: Gd<AudioStreamPlayer>,
}

impl AudioStreaming {
    pub fn new() -> Self {
        let generator = AudioStreamGenerator::new_gd();
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
                let mut err_code = 0;
                let dec_ptr = opus_static_sys::opus_decoder_create(AUDIO_SAMPLE_RATE as i32, 1, &mut err_code);
                
                let Some(dec) = NonNull::new(dec_ptr) else {
                    panic!("Failed to create Opus decoder: {}", err_code);
                };

                dec
            },
            audio_buffer: [0.0; AUDIO_FRAME_SIZE as usize],
            udp_buffer: [0u8; 4096],
            player,
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
                        let result = unsafe {
                            opus_decode_float(
                                self.decoder.as_ptr(),
                                self.udp_buffer.as_ptr().cast(),
                                n as i32,
                                self.audio_buffer.as_mut_ptr(),
                                AUDIO_FRAME_SIZE as i32,
                                1
                            )
                        };
                        if result < 0 {
                            godot_error!("Failed to decode audio: {}", -result);
                        } else {
                            // godot_print!("Decoded {} samples", result);
                            self.player.get_stream_playback().unwrap().cast::<AudioStreamGeneratorPlayback>().push_buffer(
                                &self.audio_buffer[..result as usize]
                                    .iter()
                                    .map(|&x| Vector2::new(x, x))
                                    .collect(),
                            );
                        }
                        // match self.decoder.decode_float(
                        //     &self.udp_buffer[..n],
                        //     &mut self.audio_buffer,
                        //     false,
                        // ) {
                        //     Ok(n) => {
                        //         // godot_print!("{:?}", &self.audio_buffer[0..n]);
                        //         self.playback.push_buffer(
                        //             &self.audio_buffer[0..n]
                        //                 .iter()
                        //                 .map(|&x| Vector2::new(x, x))
                        //                 .collect(),
                        //         );
                        //     }
                        //     Err(e) => {
                        //         godot_print!("{n}");
                        //         godot_error!("Failed to decode audio: {}", e);
                        //         if let Err(e) = self.decoder.reset_state() {
                        //             godot_error!("Failed to reset decoder state: {}", e);
                        //         }
                        //     }
                        // }
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

impl Drop for AudioStreaming {
    fn drop(&mut self) {
        unsafe {
            opus_static_sys::opus_decoder_destroy(self.decoder.as_ptr());
        }
    }
}