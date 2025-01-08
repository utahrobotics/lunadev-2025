use std::net::{SocketAddr, UdpSocket};

use anyhow::Context;
use common::AUDIO_FRAME_SIZE;
use rodio::{cpal::{default_host, traits::{HostTrait, StreamTrait}, StreamConfig}, DeviceTrait};
use tracing::debug;


pub fn audio_streaming(lunabase_audio_streaming_address: SocketAddr) -> anyhow::Result<()> {
    let udp = UdpSocket::bind("0.0.0.0:0").context("Binding streaming UDP socket")?;
    udp.connect(lunabase_audio_streaming_address)
        .context("Connecting to lunabase streaming address")?;
    let audio_input = default_host()
        .default_input_device()
        .context("No input device")?;

    let mut enc = opus::Encoder::new(AUDIO_SAMPLE_RATE, opus::Channels::Mono, opus::Application::LowDelay)?;
    enc.set_inband_fec(true)?;
    enc.set_bitrate(opus::Bitrate::Bits(96000))?;
    let mut encoded = [0u8; 4096];
    let mut samples_vec = vec![];

    std::thread::spawn(move || {
        let result = audio_input.build_input_stream(
            &StreamConfig {
                channels: 1,
                sample_rate: rodio::cpal::SampleRate(48000),
                buffer_size: rodio::cpal::BufferSize::Fixed(AUDIO_FRAME_SIZE),
            },
            move |samples: &[i16], _info| {
                samples_vec.extend_from_slice(samples);
                while samples_vec.len() >= AUDIO_FRAME_SIZE as usize {
                    match enc.encode(&samples_vec[0..AUDIO_FRAME_SIZE as usize], &mut encoded) {
                        Ok(n) => {
                            let _ = udp.send(&encoded[..n]);
                        },
                        Err(e) => {
                            debug!("Error encoding audio: {}", e);
                        }
                    }
                    samples_vec.drain(0..AUDIO_FRAME_SIZE as usize);
                }
            },
            |e| {
                debug!("Error in audio input stream: {}", e);
            },
            None
        );
        match result {
            Ok(stream) => {
                if let Err(e) = stream.play() {
                    debug!("Error playing audio input stream: {}", e);
                    return;
                }
                loop {
                    std::thread::park();
                }
            },
            Err(e) => {
                debug!("Error creating audio input stream: {}", e);
            }
        }
    });

    Ok(())
}