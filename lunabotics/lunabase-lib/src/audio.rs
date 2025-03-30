use std::net::{IpAddr, UdpSocket};

use common::{AUDIO_FRAME_SIZE, AUDIO_SAMPLE_RATE};
#[cfg(not(target_arch = "aarch64"))]
use godot::prelude::*;
use godot::{
    classes::{AudioStreamGenerator, AudioStreamPlayer},
    global::{godot_error, godot_warn},
    obj::{BaseMut, Gd, NewAlloc, NewGd},
};
#[cfg(not(target_arch = "aarch64"))]
use godot::{builtin::Vector2, classes::AudioStreamGeneratorPlayback};
#[cfg(not(target_arch = "aarch64"))]
use opus::Decoder;

use crate::LunabotConn;

#[allow(dead_code)]
pub struct AudioStreaming {
    // playback: Gd<AudioStreamGeneratorPlayback>,
    udp: Option<UdpSocket>,
    #[cfg(not(target_arch = "aarch64"))]
    decoder: Decoder,
    audio_buffer: [f32; AUDIO_FRAME_SIZE as usize],
    udp_buffer: [u8; 4096],
    player: Gd<AudioStreamPlayer>,
    expected_i: Option<u32>,
    lunabot_address: Option<IpAddr>,
    ping_timeout: f64
}

impl AudioStreaming {
    pub fn new(lunabot_address: Option<IpAddr>) -> Self {
        #[cfg(target_arch = "aarch64")]
        godot_warn!("Audio streaming is not supported on aarch64");
        
        let mut generator = AudioStreamGenerator::new_gd();
        generator.set_mix_rate(AUDIO_SAMPLE_RATE as f32);
        // generator.set_buffer_length(0.8);
        let mut player = AudioStreamPlayer::new_alloc();
        player.set_autoplay(true);
        player.set_stream(&generator);

        let udp = UdpSocket::bind(std::net::SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), common::ports::AUDIO))
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
            #[cfg(not(target_arch = "aarch64"))]
            decoder: Decoder::new(AUDIO_SAMPLE_RATE, opus::Channels::Mono).unwrap(),
            audio_buffer: [0.0; AUDIO_FRAME_SIZE as usize],
            udp_buffer: [0u8; 4096],
            player,
            expected_i: None,
            lunabot_address,
            ping_timeout: 1.0,
        }
    }

    pub fn poll(&mut self, mut base: BaseMut<LunabotConn>, delta: f64) {
        if !self.player.is_inside_tree() {
            base.add_child(&self.player);
        }

        #[cfg(target_arch = "aarch64")]
        let _delta = delta;
        #[cfg(not(target_arch = "aarch64"))]
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
                            godot_error!("Missed packet");
                            // godot_error!("Missed packet: {}", result);
                        }

                        self.expected_i = Some(i.wrapping_add(1));
                        let result = self.decoder.decode_float(&self.udp_buffer[4..n], &mut self.audio_buffer, false);
                        match result {
                            Ok(n) => {
                                let mut playback = self.player.get_stream_playback().unwrap().cast::<AudioStreamGeneratorPlayback>();
                                if (playback.get_frames_available() as usize) < n {
                                    godot_warn!("Dropped {} frames", n - playback.get_frames_available() as usize);
                                }
                                playback.push_buffer(
                                    &self.audio_buffer[..n]
                                        .iter()
                                        .map(|&x| Vector2::new(x, x))
                                        .collect(),
                                );
                            }
                            Err(e) => godot_error!("Failed to decode audio: {}", e)
                        }
                    }
                    Err(e) => {
                        if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
                            let Some(ip) = self.lunabot_address else {
                                break;
                            };
                            self.ping_timeout -= delta;
                            if self.ping_timeout <= 0.0 {
                                self.ping_timeout = 1.0;
                                if let Err(e) = udp.send_to(&[0u8; 1], std::net::SocketAddr::new(ip, common::ports::AUDIO)) {
                                    godot_error!("Failed to send audio ping: {e}");
                                }
                            }
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
