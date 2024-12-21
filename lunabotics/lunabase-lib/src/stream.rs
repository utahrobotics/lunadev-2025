use std::{net::{Ipv4Addr, SocketAddrV4, UdpSocket}, sync::atomic::{AtomicBool, Ordering}};

use godot::global::{godot_error, godot_print};
use openh264::{decoder::Decoder, nal_units};
use tasker::shared::LoanedData;

pub fn camera_streaming(mut shared_rgb_img: LoanedData<Vec<u8>>, stream_corrupted: &'static AtomicBool) {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {

            });
    });

    std::thread::spawn(move || {
        let stream_udp = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 10601))
            .expect("Failed to bind to 10601");
        
        if let Err(e) = Decoder::new() {
            godot_error!("Failed to initialize decoder: {e}");
            return;
        }
        let mut dec = Decoder::new().expect("Failed to initialize decoder");
        let mut buf = [0u8; 1400];
        let mut stream = vec![];

        godot_print!("Stream server started");
        let mut nals = vec![];

        loop {
            match stream_udp.recv(&mut buf) {
                Ok(n) => {
                    stream.extend_from_slice(&buf[..n]);
                }
                Err(e) => {
                    godot_error!("Failed to receive stream data: {e}");
                    break;
                }
            }

            let mut last_stream_i = 0usize;
            let start_i = stream.as_ptr() as usize;
            nals.extend(
                nal_units(&stream)
                    .into_iter()
                    .map(|nal| (nal.as_ptr() as usize - start_i, nal.len())),
            );
            let mut read_frame = false;
            // The last packet is usually incomplete
            nals.pop();

            for (stream_index, len) in nals.drain(..) {
                last_stream_i = stream_index + len;
                match dec.decode(&stream[stream_index..last_stream_i]) {
                    Ok(Some(frame)) => {
                        if !read_frame {
                            read_frame = true;
                            stream_corrupted.store(false, Ordering::Relaxed);
                            match shared_rgb_img.try_recall() {
                                Ok(mut owned) => {
                                    frame.write_rgb8(&mut owned);
                                    shared_rgb_img = owned.pessimistic_share();
                                }
                                Err(shared) => {
                                    shared_rgb_img = shared;
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        stream_corrupted.store(false, Ordering::Relaxed);
                    }
                    Err(_) => {
                        stream_corrupted.store(true, Ordering::Relaxed);
                    }
                }
            }
            stream.drain(0..last_stream_i);
        }
    });
}