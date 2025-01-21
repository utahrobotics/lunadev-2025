use tasker::{
    parking_lot::{Condvar, Mutex},
    tokio::io::{AsyncWriteExt, BufStream},
    BlockOn,
};
use tokio_serial::SerialStream;
use tracing::error;
use udev::Udev;
use vesc_translator::{CanForwarded, SetDutyCycle, VescPacker};

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

pub fn enumerate_motors() -> &'static MotorRef {
    let motor_ref: &_ = Box::leak(Box::new(MotorRef {
        mutex: Mutex::new((0.0, 0.0)),
        condvar: Condvar::new(),
    }));
    std::thread::spawn(|| enumerate_motors_priv(motor_ref));
    motor_ref
}

fn enumerate_motors_priv(motor_ref: &'static MotorRef) {
    let mut vesc_packer = VescPacker::default();

    loop {
        loop {
            let mut motor_port: Option<SerialStream> = None;

            {
                let udev = match Udev::new() {
                    Ok(x) => x,
                    Err(e) => {
                        error!("Failed to create udev context: {e}");
                        break;
                    }
                };
                let mut enumerator = match udev::Enumerator::with_udev(udev.clone()) {
                    Ok(x) => x,
                    Err(e) => {
                        error!("Failed to create udev enumerator: {e}");
                        return;
                    }
                };
                let devices = match enumerator.scan_devices() {
                    Ok(x) => x,
                    Err(e) => {
                        error!("Failed to scan devices: {e}");
                        return;
                    }
                };
                for udev_device in devices {
                    let Some(path) = udev_device.devnode() else {
                        continue;
                    };
                    println!("{:?}", path);
                    udev_device.attributes().for_each(|entry| {
                        println!("{:?}: {:?}", entry.name(), entry.value());
                    });
                    udev_device.properties().for_each(|entry| {
                        println!("{:?}: {:?}", entry.name(), entry.value());
                    });
                }
            }

            let Some(motor_port) = motor_port else {
                break;
            };
            let mut motor_port = BufStream::new(motor_port);

            loop {
                let (left, right) = {
                    let mut guard = motor_ref.mutex.lock();
                    motor_ref.condvar.wait(&mut guard);
                    *guard
                };
                let result = async {
                    motor_port
                        .write_all(vesc_packer.pack(&SetDutyCycle(left)))
                        .await?;
                    motor_port
                        .write_all(vesc_packer.pack(&CanForwarded {
                            can_id: 4,
                            payload: SetDutyCycle(right),
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
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
