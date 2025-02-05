use tasker::{
    parking_lot::{Condvar, Mutex},
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt, BufStream},
        runtime::Handle,
    },
    BlockOn,
};
use tokio_serial::SerialPortBuilderExt;
use tracing::{error, info, warn};
use udev::{EventType, MonitorBuilder, Udev};
use vesc_translator::{CanForwarded, GetValues, Getter, SetDutyCycle, VescPacker};

use crate::apps::production::udev_poll;

pub struct MotorRef {
    mutex: Mutex<(f32, f32)>,
    condvar: Condvar,
}

impl MotorRef {
    pub fn set_speed(&self, left: f32, right: f32) {
        let mut guard = self.mutex.lock();
        *guard = (left, right);
        self.condvar.notify_all();
    }
}

pub fn enumerate_motors(handle: Handle) -> &'static MotorRef {
    let motor_ref: &_ = Box::leak(Box::new(MotorRef {
        mutex: Mutex::new((0.0, 0.0)),
        condvar: Condvar::new(),
    }));
    std::thread::spawn(move || {
        let _guard = handle.enter();
        let mut vesc_packer = VescPacker::default();
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
                let mut motor_port = match tokio_serial::new(path_str, 115200)
                    .timeout(std::time::Duration::from_millis(500))
                    .open_native_async()
                {
                    Ok(x) => x,
                    Err(e) => {
                        error!("Failed to open motor port {path_str}: {e}");
                        return;
                    }
                };
                info!("Opened motor port {path_str}");
                if let Err(e) = motor_port.set_exclusive(true) {
                    warn!("Failed to set motor port {path_str} exclusive: {e}");
                }
                let mut motor_port = BufStream::new(motor_port);

                loop {
                    if let Err(e) = motor_port
                        .write_all(vesc_packer.pack(&CanForwarded {
                            can_id: 4,
                            payload: GetValues
                        }))
                        .block_on()
                    {
                        error!("Failed to write to motor port {path_str}: {e}");
                        return;
                    }
                    if let Err(e) = motor_port.flush().block_on() {
                        error!("Failed to flush motor port {path_str}: {e}");
                        return;
                    }
                    let mut response = [0u8; 79];
                    if let Err(e) = motor_port.read_exact(&mut response).block_on() {
                        error!("Failed to read from motor port {path_str}: {e}");
                        return;
                    }

                    let Ok(values) = GetValues::parse_response(&response) else {
                        error!("Received corrupt response from motor port {path_str}");
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        continue;
                    };
                    info!("Received values from motor port {path_str}: {values:#?}");
                    break;
                }

                loop {
                    let (left, right) = {
                        let mut guard = motor_ref.mutex.lock();
                        motor_ref.condvar.wait(&mut guard);
                        *guard
                    };
                    let result = async {
                        motor_port
                            .write_all(vesc_packer.pack(&SetDutyCycle(right * 0.1)))
                            .await?;
                        // motor_port
                        //     .write_all(vesc_packer.pack(&CanForwarded {
                        //         can_id: 87,
                        //         payload: SetDutyCycle(right * 0.2),
                        //     }))
                        //     .await?;
                        motor_port
                            .write_all(vesc_packer.pack(&CanForwarded {
                                can_id: 4,
                                payload: SetDutyCycle(left * 0.1),
                            }))
                            .await?;
                        motor_port.flush().await
                    }
                    .block_on();
                    if let Err(e) = result {
                        error!("Failed to write to motor port: {e}");
                        break;
                    }
                }
                error!("Motor port {path_str} closed");
            });
    });
    motor_ref
}
