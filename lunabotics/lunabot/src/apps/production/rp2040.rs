
use std::{path::PathBuf, sync::mpsc::SyncSender};

use embedded_common::*;
use fxhash::FxHashMap;
use simple_motion::StaticImmutableNode;
use tasker::tokio::{
    self,
    io::{AsyncReadExt, AsyncWriteExt},
};
use tokio_serial::SerialStream;
use tracing::{error, warn};
use udev::{Device, Enumerator, EventType, MonitorBuilder, Udev};

use crate::localization::LocalizerRef;

use super::udev_poll;

pub struct PicoController {
    serial_port: SerialStream,
}

impl PicoController {
    /// returns a sorted Vec of /dev/ttyACM* that have the serial number specified in embedded_common::ID_SERIAL
    pub fn enumerate_picos() -> Result<Vec<String>, std::io::Error> {
        let udev = Udev::new()?;
        let mut enumerator = Enumerator::with_udev(udev)?;
        let devices = enumerator.scan_devices()?;

        let mut candidates = Vec::new();
        for device in devices.collect::<Vec<Device>>() {
            if let Some(path) = device.devnode() {
                device.properties().for_each(|property| {
                    if property.value().to_string_lossy().to_string() == UDEVADM_ID
                        && !path.starts_with("/dev/bus/")
                    {
                        let path = path.to_owned();
                        candidates.push(path.to_string_lossy().to_string());
                    }
                });
            }
        }
        candidates.sort();
        return Ok(candidates);
    }

    pub async fn new(serial_port: &str) -> Result<Self, std::io::Error> {
        let builder = tokio_serial::new(serial_port, 9600);
        let file = SerialStream::open(&builder)?;
        // let file = tokio::fs::OpenOptions::new().read(true).write(true).open(serial_port).await?;
        Ok(PicoController { serial_port: file })
    }

    // /// makes sure the serial number is Embassy_USB-serial_12345678
    // async fn check_udevadm_info(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    //     let udevadm = Command::new("udevadm").arg("info").arg(path).output().await?;
    //     if String::from_utf8_lossy(&udevadm.stdout).contains(UDEVADM_ID) {
    //         return Ok(());
    //     } else {
    //         return Err(Box::new(std::io::Error::new(
    //             std::io::ErrorKind::NotFound,
    //             format!("{path} has the wrong serial number")
    //         )))
    //     }
    // }

    async fn send_ack(&mut self) -> Result<(), std::io::Error> {
        let ack = [1];
        self.serial_port.write(&ack).await?;
        self.serial_port.flush().await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        Ok(())
    }

    pub async fn get_message_from_pico(&mut self) -> Result<FromIMU, std::io::Error> {
        self.send_ack().await?;
        let mut data: [u8; 13] = [0; 13];
        self.serial_port.read_exact(&mut data).await?;
        return Ok(FromIMU::deserialize(data).map_err(|e| {
            error!("Error deserializing message FromIMU: {e}");
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "failed to deserialize FromIMU from the serial port",
            )
        })?);
    }
}

pub struct IMUInfo {
    pub node: StaticImmutableNode,
}

pub fn enumerate_imus(
    localizer_ref: &LocalizerRef,
    port_to_chain: impl IntoIterator<Item = (String, IMUInfo)>,
) {
    let mut threads: FxHashMap<String, SyncSender<PathBuf>> = port_to_chain
        .into_iter()
        .filter_map(
            |(
                port,
                IMUInfo {
                    node
                },
            )| {
                let port2 = port.clone();
                let localizer_ref = localizer_ref.clone();
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                std::thread::spawn(move || {
                    let mut camera_task = CameraTask {
                        path: rx,
                        port,
                        camera_stream,
                        image: OnceCell::new(),
                        focal_length_x_px,
                        focal_length_y_px,
                        apriltags,
                        localizer_ref,
                        node,
                    };
                    loop {
                        camera_task.camera_task();
                    }
                });
                Some((port2, tx))
            },
        )
        .collect();

    std::thread::spawn(move || {
        let mut monitor = match MonitorBuilder::new() {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to create udev monitor: {e}");
                return;
            }
        };
        monitor = match monitor.match_subsystem("tty") {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to set match-subsystem filter: {e}");
                return;
            }
        };
        let listener = match monitor.listen() {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to listen for udev events: {e}");
                return;
            }
        };

        let mut enumerator = {
            let udev = match Udev::new() {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to create udev context: {e}");
                    return;
                }
            };
            match udev::Enumerator::with_udev(udev) {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to create udev enumerator: {e}");
                    return;
                }
            }
        };
        if let Err(e) = enumerator.match_subsystem("tty") {
            error!("Failed to set match-subsystem filter: {e}");
        }
        let devices = match enumerator.scan_devices() {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to scan devices: {e}");
                return;
            }
        };
        devices
            .into_iter()
            .chain(
                udev_poll(listener)
                    .filter(|event| event.event_type() == EventType::Add)
                    .map(|event| {
                        event.device()
                    }),
            )
            .for_each(|device| {
                let Some(path) = device.devnode() else {
                    return;
                };
                let Some(path_str) = path.to_str() else {
                    return;
                };
                let Some(serial_cstr) = device.property_value("ID_SERIAL") else {
                    return;
                };
                let Some(serial) = serial_cstr.to_str() else {
                    warn!("Failed to parse serial of device {path_str}");
                    return;
                };
            })
    });
}