use std::sync::mpsc::{Receiver, Sender, SyncSender};

use crate::localization::{IMUReading, LocalizerRef};
use embedded_common::*;
use fxhash::FxHashMap;
use nalgebra::{UnitQuaternion, UnitVector3, Vector3};
use simple_motion::StaticImmutableNode;
use tasker::{
    tokio::{
        self,
        io::{AsyncReadExt, AsyncWriteExt, BufStream},
    },
    BlockOn,
};
use tokio_serial::SerialPortBuilderExt;
use tracing::{error, info, warn};
use udev::{EventType, MonitorBuilder, Udev};

use super::udev_poll;

pub const ACTUATOR_CONTROLLER_DEFAULT_SERIAL: &'static str = "1";

pub struct IMUInfo {
    pub node: StaticImmutableNode,
    pub link_name: String,
    pub correction: UnitQuaternion<f32>
}

pub fn enumerate_imus(
    localizer_ref: &LocalizerRef,
    serial_to_chain: impl IntoIterator<Item = (String, IMUInfo)>,
) {
    // get_tokio_handle().spawn(imu_wifi_listener());
    // let data_queue: Box<ArrayQueue<IMUReading>> = Box::new(ArrayQueue::new(64));
    // let data_queue_ref:&'static ArrayQueue<IMUReading> = Box::leak(data_queue);

    // std::thread::spawn(move || {
    //     const SAMPLE_RATE_HZ: u32 = 100;
    //     let settings = FusionAhrsSettings::new();
    //     let mut fusion = Fusion::new(SAMPLE_RATE_HZ, settings);

    //     let mut reading: Option<IMUReading> = None;
    //     loop {
    //         reading = data_queue_ref.pop();
    //         if let Some(IMUReading(gyro, accel, time)) = reading {
    //             fusion.update_no_mag(gyro.into(), accel.into(), time);
    //             let acc = fusion.ahrs.linear_acc();
    //             // tracing::info!("x: {}, y: {}, z: {}", acc.x, acc.y, acc.z);
    //         }
    //     }
    // });

    let mut threads: FxHashMap<String, SyncSender<String>> = serial_to_chain
        .into_iter()
        .enumerate()
        .filter_map(|(index, (port, IMUInfo { node, correction, link_name }))| {
            let port2 = port.clone();
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let localizer_ref = localizer_ref.clone();
            std::thread::spawn(move || {
                let mut imu_task = IMUTask {
                    path: rx,
                    node,
                    localizer_ref,
                    index,
                    correction,
                    link_name
                };
                loop {
                    imu_task.imu_task().block_on();
                }
            });
            Some((port2, tx))
        })
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
                    .map(|event| event.device()),
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
                    if serial == "USR_IMU" {
                        warn!("IMU at path {path_str} has no serial number");
                        return;
                    }
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

struct IMUTask {
    path: Receiver<String>,
    node: StaticImmutableNode,
    localizer_ref: LocalizerRef,
    index: usize,
    link_name: String,
    correction: UnitQuaternion<f32>
}

const INIT_ACCEL_COUNT: usize = 10;

impl IMUTask {
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
        let mut data: [u8; 25] = [0; 25];

        let mut init_accel_sum = Vector3::zeros();
        let mut init_accel_count = 0usize;

        // let start = Instant::now();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(IMU_READING_DELAY_MS)).await;
            const ACK: [u8; 1] = [1];
            let result = async {
                imu_port.write(&ACK).await?;
                imu_port.flush().await?;
                imu_port.read_exact(&mut data).await
            }
            .await;
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
                FromIMU::Reading(rate, accel) => {
                    let local_isometry = self.node.get_local_isometry().cast();
                    let angular_velocity = Vector3::new(-rate.x, rate.z, rate.y);
                    let transformed_rate = self.correction * local_isometry.rotation * angular_velocity;

                    let accel = Vector3::new(accel.x, -accel.z, -accel.y);
                    let transformed_accel = self.correction * local_isometry.rotation * accel * 9.8;

                    if init_accel_count < INIT_ACCEL_COUNT {
                        init_accel_count += 1;
                        init_accel_sum += accel;

                        if init_accel_count == INIT_ACCEL_COUNT {
                            init_accel_sum.unscale_mut(init_accel_count as f32);
                            if let Some(mut init_accel) = UnitVector3::try_new(init_accel_sum, 0.01) {
                                init_accel = local_isometry.rotation * init_accel;
                                if let Some(correction) = UnitQuaternion::rotation_between_axis(&init_accel, &-Vector3::y_axis()) {
                                    tracing::info!("{} estimated correction: [{:.3}, {:.3}, {:.3}, {:.3}]", self.link_name, correction.i, correction.j, correction.k, correction.w);
                                } else {
                                    tracing::error!("{} failed to get correction quat", self.link_name);
                                }
                            } else {
                                tracing::error!("{} failed to acceleration", self.link_name);
                            }
                        }
                    }

                    self.localizer_ref.set_imu_reading(
                        self.index,
                        IMUReading {
                            angular_velocity: transformed_rate.cast(),
                            acceleration: transformed_accel.cast(),
                        },
                    );
                    // if let Err(_) = self.queue.push(IMUReading(transformed_rate, transformed_accel, start.elapsed().as_secs_f32())) {
                    //     tracing::warn!("couldn't push gyro reading to crossbeam queue");
                    // }
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

pub struct ActuatorControllerInfo {
    pub serial: String
}

impl Default for ActuatorControllerInfo {
    fn default() -> Self {
        tracing::warn!("Assuming default actuator serial");
        Self {
            serial: String::from(ACTUATOR_CONTROLLER_DEFAULT_SERIAL)
        }
    }
}

struct ActuatorControllerTask {
    path: Receiver<String>,
    localizer_ref: LocalizerRef,
    command_rx: Receiver<ActuatorCommand>
}

pub struct ActuatorController {
    command_tx: Sender<ActuatorCommand>
}

impl ActuatorController {
    pub fn send_command(&mut self, cmd: ActuatorCommand) -> Result<(), std::sync::mpsc::SendError<ActuatorCommand>> {
	//tracing::info!("called send_command on ActuatorController");
        self.command_tx.send(cmd)?;
        Ok(())
    }
}

pub fn enumerate_actuator_controllers(localizer_ref: &LocalizerRef, actuator_controller_info: ActuatorControllerInfo) -> ActuatorController {
    let (tx_path, rx_path) = std::sync::mpsc::sync_channel(1);
    let (tx_commands, rx_commands) = std::sync::mpsc::channel();

    let localizer_ref = localizer_ref.clone();
    std::thread::spawn(move || {
        let mut actuator_task = ActuatorControllerTask {
            path: rx_path,
            localizer_ref,
            command_rx: rx_commands
        };
        loop {
            actuator_task.actuator_task().block_on();
        }
    });

    let controller_serial = actuator_controller_info.serial;
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

        // infinite iterator
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
                let Some(tmp) = serial.strip_prefix("USR_ACTUATOR_") else {
                    if serial == "USR_ACTUATOR" {
                        warn!("Actuator controller at path {path_str} has no serial number");
                        return;
                    }
                    return;
                };
                serial = tmp;

                if serial == controller_serial {
                    if tx_path.send(path_str.into()).is_err() {
                        warn!("Couldnt send controller path");
                    }
                } else {
                    warn!("Unexpected actuator with serial {}", serial);
                }
            })
    });
    ActuatorController { command_tx: tx_commands }
}

impl ActuatorControllerTask {
    pub async fn actuator_task(&mut self) {
	tracing::info!("starting actuator task");
        let path_str = match self.path.recv() {
            Ok(x) => x,
            Err(_) => loop {
                std::thread::park();
            },
        };
        let mut actuator_port = match tokio_serial::new(&path_str, 9600)
            .timeout(std::time::Duration::from_millis(500))
            .open_native_async()
        {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to open actuator controller port {path_str}: {e}");
                return;
            }
        };
        info!("Opened Actuator controller port {path_str}");
        if let Err(e) = actuator_port.set_exclusive(true) {
            warn!("Failed to set actuator controller port {path_str} exclusive: {e}");
        }
        let mut actuator_port = BufStream::new(actuator_port);
        loop {
            let Ok(cmd) = self.command_rx.recv() else {
                tracing::error!("Error receiving actuator command in actuator_task()");
                continue;
            };

            // tracing::info!("Received actuator command in ActuatorControllerTask: {:?}", cmd);
            if let Err(e) = actuator_port.write_all(&cmd.serialize()).await {
                tracing::error!("Failed to write to actuator port {e}");
                break;
            }
	        if let Err(e) = actuator_port.flush().await {
                tracing::error!("Failed to flush to actuator port {e}");
                break;
            }
            tracing::warn!("Disconnnected from {}", path_str);
        }
    }
}
