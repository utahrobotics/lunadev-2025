use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use common::AUDIO_FRAME_SIZE;
use crossbeam::atomic::AtomicCell;
use rodio::{buffer::SamplesBuffer, cpal::{default_host, traits::{HostTrait, StreamTrait}, StreamConfig}, static_buffer::StaticSamplesBuffer, DeviceTrait, OutputStream, Sink};
use tracing::{error, info};


pub fn audio_streaming() -> &'static AtomicCell<Option<IpAddr>> {
    let address: &_ = Box::leak(Box::new(AtomicCell::new(None)));
    let udp = match UdpSocket::bind(SocketAddr::new(
        Ipv4Addr::UNSPECIFIED.into(),
        common::ports::AUDIO,
    )) {
        Ok(x) => x,
        Err(e) => {
            error!("Failed to bind to UDP socket: {e}");
            return address;
        }
    };
    let host = default_host();
    let input_devices = match host.input_devices() {
        Ok(x) => x,
        Err(e) => {
            error!("Failed to get audio input devices: {e}");
            return address;
        }
    };
    // let audio_input = input_devices.for_each(|device| {
    //     tracing::info!("{}", device.name().unwrap_or_else(|_| "unknown device".to_string()));
    // });
    let audio_input = match host
        .default_input_device() {
            Some(x) => x,
            None => {
                error!("Failed to get default audio input device");
                return address;
            }
    };

    let mut enc = opus::Encoder::new(common::AUDIO_SAMPLE_RATE, opus::Channels::Mono, opus::Application::LowDelay).unwrap();
    enc.set_inband_fec(true).unwrap();
    enc.set_bitrate(opus::Bitrate::Bits(96000)).unwrap();
    let mut encoded = [0u8; AUDIO_FRAME_SIZE as usize];
    let mut samples_vec = vec![];

    std::thread::spawn(move || {
        let result = audio_input.build_input_stream(
            &StreamConfig {
                channels: 1,
                sample_rate: rodio::cpal::SampleRate(common::AUDIO_SAMPLE_RATE),
                buffer_size: rodio::cpal::BufferSize::Default,
            },
            move |samples: &[i16], _info| {
                samples_vec.extend_from_slice(samples);
                while samples_vec.len() >= AUDIO_FRAME_SIZE as usize {
                    if let Some(ip) = address.load() {
                        match enc.encode(&samples_vec[0..AUDIO_FRAME_SIZE as usize], &mut encoded) {
                            Ok(n) => {
                                let _ = udp.send_to(&encoded[..n], SocketAddr::new(ip, common::ports::AUDIO));
                            },
                            Err(e) => {
                                error!("Error encoding audio: {}", e);
                            }
                        }
                    }
                    samples_vec.drain(0..AUDIO_FRAME_SIZE as usize);
                }
            },
            |e| {
                error!("Error in audio input stream: {}", e);
            },
            None
        );
        match result {
            Ok(stream) => {
                if let Err(e) = stream.play() {
                    error!("Error playing audio input stream: {}", e);
                    return;
                }
                info!("Audio streaming started with device: {}", audio_input.name().unwrap_or_else(|_| "unknown device".to_string()));
                // Dropping the stream will stop it
                std::mem::forget(stream);
            }
            Err(e) => {
                error!("Error creating audio input stream: {}", e);
            }
        }
    });

    address
}