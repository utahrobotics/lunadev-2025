use std::{sync::{mpsc::{Receiver, Sender, RecvTimeoutError}, Arc}, time::Duration};

use crate::localization::{IMUReading, LocalizerRef};
use embedded_common::*;
use nalgebra::Vector3;
use simple_motion::{StaticImmutableNode, Node, NodeData};
use tasker::{
    get_tokio_handle, tokio::{
        self,
        io::{AsyncReadExt, AsyncWriteExt, BufStream}, sync::Mutex,
    }, BlockOn
};
use tokio_serial::SerialPortBuilderExt;
use tokio_serial::SerialPort;
use tracing::{error, info, warn};
use udev::{EventType, MonitorBuilder, Udev};

use super::udev_poll;

pub struct V3PicoInfo {
    pub serial: String,
    pub imus: [IMUInfo; 4]
}

pub struct IMUInfo {
    pub node: StaticImmutableNode,
    pub link_name: String,
}

/// find pico connected to the v3 pcb.
pub fn enumerate_v3picos(hinge_node: Node<&'static [NodeData]>, localizer_ref: LocalizerRef, pico: V3PicoInfo) -> ActuatorController {
    let (path_tx, path_rx) = std::sync::mpsc::sync_channel::<String>(1);
    let (actuator_cmd_tx, actuator_cmd_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let shared = SharedState {
            localizer_ref,
            imus: [
                pico.imus[0].node,
                pico.imus[1].node,
                pico.imus[2].node,
                pico.imus[3].node
            ],
            hinge_node,
            first_startup: true
        };
        let mut task = V3PicoTask {
            path: path_rx,
            actuator_command_rx: actuator_cmd_rx,
            shared: Arc::new(Mutex::new(shared))
        };
        loop {
            task.v3pico_task().block_on();
        }
    });
    let controller_serial = pico.serial;
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
                let Some(tmp) = serial.strip_prefix("USR_V3PICO_") else {
                    if serial == "USR_V3PICO" {
                        warn!("Actuator controller at path {path_str} has no serial number");
                        return;
                    }
                    return;
                };
                serial = tmp;

                if serial == controller_serial {
                    if path_tx.send(path_str.into()).is_err() {
                        warn!("Couldnt send controller path");
                    }
                } else {
                    warn!("Unexpected actuator with serial {}", serial);
                }
            })
    });
    ActuatorController {
        command_tx: actuator_cmd_tx
    }
}

pub struct ActuatorController {
    command_tx: Sender<ActuatorCommand>,
}

impl ActuatorController {
    pub fn send_command(
        &mut self,
        cmd: ActuatorCommand,
    ) -> Result<(), std::sync::mpsc::SendError<ActuatorCommand>> {
        //tracing::info!("called send_command on ActuatorController");
        self.command_tx.send(cmd)?;
        Ok(())
    }
}

#[derive(Clone)]
struct SharedState {
    localizer_ref: LocalizerRef,
    /// imu node
    imus: [StaticImmutableNode; 4],
    hinge_node: Node<&'static [NodeData]>,
    first_startup: bool
}

pub struct V3PicoTask {
    path: Receiver<String>,
    actuator_command_rx: std::sync::mpsc::Receiver<ActuatorCommand>,
    shared: Arc<tokio::sync::Mutex<SharedState>>,
}

impl V3PicoTask {
    pub async fn v3pico_task(&mut self) {
        let path_str = match self.path.recv() {
            Ok(x) => x,
            Err(_) => loop {
                std::thread::park();
            },
        };
        let mut port = match tokio_serial::new(&path_str, 150000)
            // .timeout(std::time::Duration::from_millis(100))
            .flow_control(tokio_serial::FlowControl::None)
            .open_native_async()
        {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to open V3Pico controller port {path_str}: {e}");
                return;
            }
        };
        let _ = port.write_data_terminal_ready(true);

        info!("Opened V3Pico controller port {path_str}");
        if let Err(e) = port.set_exclusive(true) {
            warn!("Failed to set V3Pico controller port {path_str} exclusive: {e}");
        }
        let port = BufStream::new(port);
        let (mut reader, mut writer) = tokio::io::split(port);
        let shared = Arc::clone(&self.shared);
        let (is_broken_tx, is_broken_rx) = tokio::sync::watch::channel(false);
        let path_str_clone = path_str.clone();
        get_tokio_handle().spawn(async move {
            let mut guard = shared.lock().await;
            if guard.first_startup {
                guard.first_startup = false;
                // power_cycle(&path_str_clone).unwrap();
            }
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(IMU_READING_DELAY_MS)).await;
                let mut reading = [0u8; FromPicoV3::SIZE];
                if let Err(e) = reader.read_exact(&mut reading).await {
                    let _ = is_broken_tx.send(true);
                    error!("failed to read from pico: {}", e);
                    break;
                }
                let Ok(reading) = FromPicoV3::deserialize(reading) else {
                    error!("Failed to deserialize message from picov3 serial port");
                    let _ = is_broken_tx.send(true);
                    break;
                };
                if let FromPicoV3::Reading(imu_readings,actuators) = reading {
                    let deg_to_rad = 0.0174532925199; // pi/180
                    let lift_hinge_angle = (actuators.m1_reading as f64 * 0.00743033 - 2.19192);
                    tracing::info!("lift angle: {}", lift_hinge_angle);
		            guard.hinge_node.set_angle_one_axis(lift_hinge_angle*deg_to_rad);
                    for (i,(msg, node)) in imu_readings.into_iter().zip(guard.imus).enumerate() {
                        match msg {
                            FromIMU::Reading(rate, accel) => {
                                let rotation = node.get_isometry_from_base().rotation.cast();
                                let angular_velocity = Vector3::new(-rate.x, rate.z, rate.y);
                                let transformed_rate = (rotation * angular_velocity);
                                // info!("rotation: {}", local_isometry.rotation);
                                // info!("imu{} {:?}",i, transformed_rate);

                                let accel = Vector3::new(accel.x, -accel.z, -accel.y);

                                let transformed_accel = rotation * accel * 9.8;
                                // info!("imu{} {:?}",i ,transformed_accel);

                                guard.localizer_ref.set_imu_reading(
                                    i,
                                    IMUReading {
                                        angular_velocity: transformed_rate.cast(),
                                        acceleration: transformed_accel.cast(),
                                    }
                                );
                            }
                            FromIMU::NoDataReady => {
                                // warn!("No data ready");
                                continue;
                            }
                            FromIMU::Error => {
                                // error!("IMU reported error");
                                continue;
                            }
                        }
                    }
                } else {
                    error!("V3 pico reported an error");
                    let _ = is_broken_tx.send(true);
                    break;
                }
            }
        });

        loop {
            if *is_broken_rx.borrow() {
                break;
            }
            let cmd_result = self.actuator_command_rx.recv_timeout(Duration::from_secs(1));
            let Ok(cmd) = cmd_result else {
                if cmd_result.err().unwrap() != RecvTimeoutError::Timeout {
                    tracing::error!("Actuator command thread channel closed");
                }
                continue;
            };
            if let Err(e) = writer.write_all(&cmd.serialize()).await {
                tracing::error!("Failed to write to actuator port {e}");
                break;
            }
            if let Err(e) = writer.flush().await {
                tracing::error!("Failed to flush to actuator port {e}");
                break;
            }
        }
    }
}


use std::{fs, io, path::{Path, PathBuf}, thread};

/// gets the "…/authorized" for `/dev/ttyACM*`.
fn authorized_path(tty: &str) -> io::Result<PathBuf> {
    let path = Path::new("/sys/class/tty")
        .join(Path::new(tty).file_name().unwrap())
        .join("device");
    let iface_path = fs::canonicalize(path)?;
    info!("canonicalized: {:?}", iface_path);
    let dev_path: PathBuf = match iface_path.file_name().and_then(|n| n.to_str()) {
        Some(name) if name.contains(":") => iface_path
            .parent()
            .ok_or_else(||{ io::Error::new(io::ErrorKind::Other, "no parent dir")})?
            .to_path_buf(),
        _ => iface_path,
    };
    info!("authorized_path: {:?}", dev_path.join("authorized"));
    Ok(dev_path.join("authorized"))
}

pub fn power_cycle(tty: &str) -> io::Result<()> {
    let auth = authorized_path(tty)?;

    fs::write(&auth, b"0")?;
    info!("disabled port");
    thread::sleep(Duration::from_millis(500));
    fs::write(&auth, b"1")?;
    Ok(())
}
