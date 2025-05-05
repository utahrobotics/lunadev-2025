use mio::{Events, Interest, Poll, Token};
use udev::{Event, EventType, MonitorBuilder, Udev};
use v4l::{buffer::Type, io::traits::CaptureStream, prelude::MmapStream};

fn main() {
    let mut monitor = match MonitorBuilder::new() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Failed to create udev monitor: {e}");
            return;
        }
    };
    monitor = match monitor.match_subsystem("video4linux") {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Failed to set match-subsystem filter: {e}");
            return;
        }
    };
    let listener = match monitor.listen() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Failed to listen for udev events: {e}");
            return;
        }
    };

    let mut enumerator = {
        let udev = match Udev::new() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Failed to create udev context: {e}");
                return;
            }
        };
        match udev::Enumerator::with_udev(udev) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Failed to create udev enumerator: {e}");
                return;
            }
        }
    };
    if let Err(e) = enumerator.match_subsystem("video4linux") {
        eprintln!("Failed to set match-subsystem filter: {e}");
    }
    let devices = match enumerator.scan_devices() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Failed to scan devices: {e}");
            return;
        }
    };
    devices
        .into_iter()
        .chain(
            udev_poll(listener)
                .filter(|event| event.event_type() == EventType::Add)
                .map(|event| event.device()),
        )
        .for_each(|device| {
            let Some(path) = device.devnode() else {
                return;
            };
            // Valid camera paths are of the form /dev/videoN
            let Some(path_str) = path.to_str() else {
                return;
            };
            if !path_str.starts_with("/dev/video") {
                return;
            }
            let Some(udev_index) = device.attribute_value("index") else {
                eprintln!("No udev_index for camera {path_str}");
                return;
            };
            if udev_index.to_str() != Some("0") {
                return;
            }
            if let Some(name) = device.attribute_value("name") {
                if let Some(name) = name.to_str() {
                    if name.contains("RealSense") {
                        return;
                    }
                }
            }
            let Some(port_raw) = device.property_value("ID_PATH") else {
                eprintln!("No port for camera {path_str}");
                return;
            };
            let Some(port) = port_raw.to_str() else {
                eprintln!("Failed to parse port of camera {path_str}");
                return;
            };
            let path = path.to_path_buf();
            let path_str = path_str.to_string();
            let port = port.to_string();

            std::thread::spawn(move || {
                let mut camera = match v4l::Device::with_path(path) {
                    Ok(x) => x,
                    Err(e) => {
                        eprintln!("Failed to open camera {}: {e}", port);
                        return;
                    }
                };
                let mut stream = match MmapStream::with_buffers(&mut camera, Type::VideoCapture, 4) {
                    Ok(x) => x,
                    Err(e) => {
                        eprintln!("Failed to create mmap stream for camera {}: {e}", port);
                        return;
                    }
                };
                println!("Connected to camera {}: {path_str}", port);
                loop {
                    match stream.next() {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("Failed to get next frame from camera {}: {e}", port);
                            break;
                        }
                    }
                }
            });
        });
}

pub fn udev_poll(mut socket: udev::MonitorSocket) -> impl Iterator<Item = Event> {
    let mut poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(1024);

    poll.registry()
        .register(
            &mut socket,
            Token(0),
            Interest::READABLE | Interest::WRITABLE,
        )
        .unwrap();

    std::iter::from_fn(move || loop {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            if event.token() == Token(0) && event.is_writable() {
                return Some(socket.iter().collect::<Vec<_>>());
            }
        }
    })
    .flatten()
}