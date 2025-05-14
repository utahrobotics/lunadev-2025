use core::f32;
use crossbeam::{atomic::AtomicCell, utils::Backoff};
use fxhash::FxHashMap;
use serde::Deserialize;
use std::sync::mpmc::Receiver;
use tasker::{
    get_tokio_handle,
    tokio::{
        self,
        io::{AsyncReadExt, AsyncWriteExt, BufStream},
    },
    BlockOn,
};
use tokio_serial::SerialPortBuilderExt;
use tracing::{error, info, warn};
use udev::{EventType, MonitorBuilder, Udev};
use vesc_translator::{Alive, CanForwarded, GetValues, Getter, MinLength, SetRPM, VescPacker};

use crate::apps::production::udev_poll;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MotorMask {
    Left,
    Right,
}

impl MotorMask {
    fn mask(self, (left, right): (f32, f32)) -> f32 {
        match self {
            MotorMask::Left => left,
            MotorMask::Right => right,
        }
    }
}

#[derive(Default)]
pub struct VescIDs {
    /// Map from Can ID to sibling Can ID
    can_ids: FxHashMap<u8, Option<(u8, bool)>>,
    motor_masks: FxHashMap<u8, MotorMask>,
    device_count: usize,
}

impl VescIDs {
    pub fn add_dual_vesc(
        &mut self,
        id1: u8,
        id2: u8,
        mask1: MotorMask,
        mask2: MotorMask,
        command_both: bool,
    ) -> bool {
        if self.motor_masks.contains_key(&id1) || self.motor_masks.contains_key(&id2) {
            return true;
        }
        self.can_ids.insert(id1, Some((id2, command_both)));
        self.can_ids.insert(id2, Some((id1, command_both)));
        self.motor_masks.insert(id1, mask1);
        self.motor_masks.insert(id2, mask2);
        self.device_count += 1;
        false
    }

    pub fn add_single_vesc(&mut self, id: u8, mask: MotorMask) -> bool {
        if self.motor_masks.contains_key(&id) {
            return true;
        }
        self.can_ids.insert(id, None);
        self.motor_masks.insert(id, mask);
        self.device_count += 1;
        false
    }
}

pub struct MotorRef {
    speeds: AtomicCell<Option<(f32, f32)>>,
}

impl MotorRef {
    pub fn set_speed(&self, left: f32, right: f32) {
        self.speeds.store(Some((left, right)));
    }
}

pub fn enumerate_motors(vesc_ids: VescIDs, speed_multiplier: f32) -> &'static MotorRef {
    let motor_ref: &_ = Box::leak(Box::new(MotorRef {
        speeds: AtomicCell::new(None),
    }));

    let (tx, rx) = std::sync::mpmc::sync_channel::<String>(1);
    let vesc_ids = Box::leak(Box::new(vesc_ids));

    for _ in 0..vesc_ids.device_count {
        let mut task = MotorTask {
            path: rx.clone(),
            vesc_packer: VescPacker::default(),
            motor_ref,
            vesc_ids,
            speed_multiplier,
        };
        std::thread::spawn(move || loop {
            task.motor_task();
        });
    }

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
                    .filter(|event| {
                        matches!(event.event_type(), EventType::Add | EventType::Change)
                    })
                    .map(|event| event.device()),
            )
            .for_each(|device| {
                let Some(path) = device.devnode() else {
                    return;
                };
                let Some(path_str) = path.to_str() else {
                    return;
                };
                let Some(vendor_cstr) = device.property_value("ID_VENDOR") else {
                    return;
                };
                let Some(vendor) = vendor_cstr.to_str() else {
                    warn!("Failed to parse vendor of device {path_str}");
                    return;
                };
                if vendor != "STMicroelectronics" {
                    return;
                }
                let Some(serial_cstr) = device.property_value("ID_SERIAL") else {
                    return;
                };
                let Some(serial) = serial_cstr.to_str() else {
                    warn!("Failed to parse serial of device {path_str}");
                    return;
                };
                if serial != "STMicroelectronics_ChibiOS_RT_Virtual_COM_Port_304" {
                    warn!("Ignoring device {path_str} with serial {serial}");
                    return;
                }
                let _ = tx.send(path_str.into());
            });
    });
    motor_ref
}

struct MotorTask {
    path: Receiver<String>,
    vesc_packer: VescPacker,
    motor_ref: &'static MotorRef,
    vesc_ids: &'static VescIDs,
    speed_multiplier: f32,
}

impl MotorTask {
    fn motor_task(&mut self) {
        let path_str = match self.path.recv() {
            Ok(x) => x,
            Err(_) => loop {
                std::thread::park();
            },
        };
        let mut motor_port;
        {
            let _guard = get_tokio_handle().enter();
            motor_port = match tokio_serial::new(&path_str, 115200)
                .timeout(std::time::Duration::from_millis(500))
                .open_native_async()
            {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to open motor port {path_str}: {e}");
                    return;
                }
            };
        }
        if let Err(e) = motor_port.set_exclusive(true) {
            warn!("Failed to set motor port {path_str} exclusive: {e}");
        }
        let mut motor_port = BufStream::new(motor_port);

        let master_can_id;

        loop {
            let mut response = vec![];
            let mut tmp_buf = [0u8; 128];
            let task = async {
                motor_port
                    .write_all(self.vesc_packer.pack(&GetValues))
                    .await?;
                motor_port.flush().await?;
                // let mut count = 0usize;
                // let mut buf = vec![];
                // loop {
                //     let n = motor_port.read(&mut response).await.unwrap();
                //     count += n;
                //     buf.extend_from_slice(&response[..n]);
                //     println!("{path_str} {count} {buf:?}");
                // }
                while response.len() < 63 || response.last() != Some(&3) {
                    let n = motor_port.read(&mut tmp_buf).await?;
                    response.extend_from_slice(&tmp_buf[..n]);
                }
                std::io::Result::Ok(())
            };
            let task = async {
                tokio::select! {
                    res = task => res,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                        Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Timed out waiting for response"))
                    }
                }
            };
            if let Err(e) = task.block_on() {
                error!("Failed to read/write to motor port {path_str}: {e}");
                return;
            }

            let Ok(buf) = MinLength::try_from(response.as_slice()) else {
                error!("Received too short of a message from motor port {path_str}");
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            };

            let Ok(values) = GetValues::parse_response(&buf) else {
                error!("Received corrupt response from motor port {path_str}");
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            };

            master_can_id = values.vesc_id;
            break;
        }

        let Some(&slave_can) = self.vesc_ids.can_ids.get(&master_can_id) else {
            error!("Found unknown master Can ID {master_can_id}");
            return;
        };

        if let Some((can_id, _)) = slave_can {
            loop {
                let mut response = vec![];
                let mut tmp_buf = [0u8; 128];
                let task = async {
                    motor_port
                        .write_all(self.vesc_packer.pack(&CanForwarded {
                            can_id,
                            payload: GetValues,
                        }))
                        .await?;
                    motor_port.flush().await?;
                    while response.len() < 63 || response.last() != Some(&3) {
                        let n = motor_port.read(&mut tmp_buf).await?;
                        response.extend_from_slice(&tmp_buf[..n]);
                    }
                    std::io::Result::Ok(())
                };
                let task = async {
                    tokio::select! {
                        res = task => res,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Timed out waiting for response"))
                        }
                    }
                };
                if let Err(e) = task.block_on() {
                    error!("Failed to read/write to motor port {path_str}: {e}");
                    return;
                }

                let Ok(buf) = MinLength::try_from(response.as_slice()) else {
                    error!("Received too short of a message from motor port {path_str}");
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                };

                let Ok(values) = GetValues::parse_response(&buf) else {
                    error!("Received corrupt response from motor port {path_str}");
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                };
                if can_id != values.vesc_id {
                    error!(
                        "Received can id {} instead of {} from sibling",
                        values.vesc_id, can_id
                    );
                    return;
                }
                break;
            }
            info!("Opened motor {} and {}", master_can_id, can_id);
        } else {
            info!("Opened motor {}", master_can_id);
        }

        let master_mask = *self.vesc_ids.motor_masks.get(&master_can_id).unwrap();
        let slave_mask =
            slave_can.map(|(can_id, _)| *self.vesc_ids.motor_masks.get(&can_id).unwrap());

        let backoff = Backoff::new();

        loop {
            let values = loop {
                let values = self.motor_ref.speeds.take();
                if let Some(values) = values {
                    break values;
                }
                backoff.snooze();
            };
            backoff.reset();

            let task = async {
                if let Some((can_id, true)) = slave_can {
                    motor_port
                        .write_all(self.vesc_packer.pack(&CanForwarded {
                            can_id,
                            payload: SetRPM(
                                slave_mask.unwrap().mask(values) * self.speed_multiplier,
                            ),
                        }))
                        .await?;
                    motor_port.write_all(self.vesc_packer.pack(&Alive)).await?;
                }
                motor_port
                    .write_all(
                        self.vesc_packer
                            .pack(&SetRPM(master_mask.mask(values) * self.speed_multiplier)),
                    )
                    .await?;
                // motor_port
                //     .write_all(self.vesc_packer.pack(&Alive))
                //     .await?;
                motor_port.flush().await
            };

            if let Err(e) = task.block_on() {
                error!("Failed to write to motor port: {e}");
                break;
            }
        }

        if let Some((slave_can_id, _)) = slave_can {
            error!("Motors {} and {} closed", master_can_id, slave_can_id);
        } else {
            error!("Motor {} closed", master_can_id);
        }
    }
}
