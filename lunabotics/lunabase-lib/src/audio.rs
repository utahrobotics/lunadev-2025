use std::net::UdpSocket;

use common::{AUDIO_FRAME_SIZE, AUDIO_SAMPLE_RATE};
use godot::{
    builtin::Vector2,
    classes::{AudioStreamGenerator, AudioStreamGeneratorPlayback, AudioStreamPlayer},
    global::{godot_error, godot_print},
    obj::{BaseMut, Gd, NewAlloc, NewGd},
};
use opus::Decoder;

use crate::LunabotConn;

pub struct AudioStreaming {
    playback: Gd<AudioStreamGeneratorPlayback>,
    udp: Option<UdpSocket>,
    decoder: Decoder,
    audio_buffer: [f32; AUDIO_FRAME_SIZE as usize],
    udp_buffer: [u8; 4096],
    player: Option<Gd<AudioStreamPlayer>>,
}

impl AudioStreaming {
    pub fn new() -> Self {
        let mut generator = AudioStreamGenerator::new_gd();
        let playback = generator.instantiate_playback().unwrap().cast();
        let mut player = AudioStreamPlayer::new_alloc();
        player.set_autoplay(true);
        player.set_stream(&generator);
        
        let udp = UdpSocket::bind("0.0.0.0:10602")
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
            playback,
            udp,
            decoder: Decoder::new(AUDIO_SAMPLE_RATE, opus::Channels::Mono)
                .expect("Failed to initialize decoder"),
            audio_buffer: [0.0; AUDIO_FRAME_SIZE as usize],
            udp_buffer: [0u8; 4096],
            player: Some(player)
        }
    }

    pub fn poll(&mut self, mut base: BaseMut<LunabotConn>) {
        if let Some(player) = self.player.take() {
            base.add_child(&player);
        }
        if let Some(udp) = &self.udp {
            loop {
                match udp.recv(&mut self.udp_buffer) {
                    Ok(n) => {
                        match self.decoder.decode_float(
                            &self.udp_buffer[..n],
                            &mut self.audio_buffer,
                            false,
                        ) {
                            Ok(n) => {
                                // godot_print!("{:?}", &self.audio_buffer[0..n]);
                                self.playback.push_buffer(
                                    &self.audio_buffer[0..n]
                                        .iter()
                                        .map(|&x| Vector2::new(x, x))
                                        .collect(),
                                );
                            }
                            Err(e) => {
                                godot_print!("{n}");
                                godot_error!("Failed to decode audio: {}", e);
                                if let Err(e) = self.decoder.reset_state() {
                                    godot_error!("Failed to reset decoder state: {}", e);
                                }
                            }
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
