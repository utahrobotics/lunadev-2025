
use std::sync::mpsc::{Receiver, SyncSender};

use embedded_common::*;
use fxhash::FxHashMap;
use nalgebra::Vector3;
use simple_motion::StaticImmutableNode;
use tasker::{tokio::{self, io::{AsyncReadExt, AsyncWriteExt, BufStream}}, BlockOn};
use tokio_serial::SerialPortBuilderExt;
use tracing::{error, info, warn};
use udev::{EventType, MonitorBuilder, Udev};

use crate::localization::LocalizerRef;

use super::udev_poll;

pub struct IMUInfo {
    pub node: StaticImmutableNode,
}

pub fn enumerate_imus(
    localizer_ref: &LocalizerRef,
    serial_to_chain: impl IntoIterator<Item = (String, IMUInfo)>,
) {
    let mut threads: FxHashMap<String, SyncSender<String>> = serial_to_chain
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
                    let mut imu_task = IMUTask {
                        path: rx,
                        localizer: &localizer_ref,
                        node,
                    };
                    loop {
                        imu_task.imu_task().block_on();
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
                let Some(mut serial) = serial_cstr.to_str() else {
                    warn!("Failed to parse serial of device {path_str}");
                    return;
                };
                let Some(tmp) = serial.strip_prefix("USR_IMU_") else {
                    return;
                };
                serial = tmp;
                
                if let Some(path_sender) = threads.get(serial) {
                    if path_sender.send(path_str.into()).is_err() {
                        threads.remove(serial);
                    }
                } else {
                    warn!("Unexpected IMU with serial {}", serial);
                }
            })
    });
}

struct IMUTask<'a> {
    path: Receiver<String>,
    localizer: &'a LocalizerRef,
    node: StaticImmutableNode,
}

impl<'a> IMUTask<'a> {
    async fn imu_task(&mut self) {
        let path_str = match self.path.recv() {
            Ok(x) => x,
            Err(_) => loop {
                std::thread::park();
            },
        };
        let mut imu_port = match tokio_serial::new(&path_str, 9600)
            .timeout(std::time::Duration::from_millis(500))
            .open_native_async()
        {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to open motor port {path_str}: {e}");
                return;
            }
        };
        info!("Opened IMU port {path_str}");
        if let Err(e) = imu_port.set_exclusive(true) {
            warn!("Failed to set motor port {path_str} exclusive: {e}");
        }
        let mut imu_port = BufStream::new(imu_port);
        let mut data: [u8; 13] = [0; 13];

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            const ACK: [u8; 1] = [1];
            let result = async {
                imu_port.write(&ACK).await?;
                imu_port.flush().await?;
                imu_port.read_exact(&mut data).await
            }.await;
            if let Err(e) = result {
                error!("Failed to read/write to IMU: {e}");
                break;
            }
            let msg = match FromIMU::deserialize(data) {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to deserialize IMU message: {e}");
                    break;
                }
            };
            match msg {
                FromIMU::AngularRateReading(AngularRate { .. }) => {
                    
                }
                FromIMU::AccelerationNormReading(AccelerationNorm { x, y, z }) => {
                    let accel: Vector3<f64> = Vector3::new(x, y, z).cast();
                    self.localizer.set_acceleration(self.node.get_local_isometry() * accel);
                    println!("{accel:?}");
                }
                FromIMU::NoDataReady => {
                    continue;
                }
                FromIMU::Error => {
                    error!("IMU reported error");
                    break;
                }
            }
        }
    }
}