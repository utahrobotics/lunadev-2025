use std::path::PathBuf;

use embedded_common::*;
use tokio::{self, fs, io::{AsyncReadExt, AsyncWriteExt}, process::Command};
use tokio_serial::SerialStream;
use tracing::{error, info};
use udev::{Device, Entry, Enumerator, Udev};

pub struct PicoController {
    serial_port: SerialStream
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
                    if property.value().to_string_lossy().to_string() == UDEVADM_ID && !path.starts_with("/dev/bus/") {
                        let path = path.to_owned();
                        candidates.push(path.to_string_lossy().to_string());
                    }
                });
            }
        }
        candidates.sort();
        return Ok(candidates);
    }

    pub async fn new(serial_port: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let builder = tokio_serial::new(serial_port, 9600);
        let file  = SerialStream::open(&builder)?;
        // let file = tokio::fs::OpenOptions::new().read(true).write(true).open(serial_port).await?;
        Ok(PicoController{
            serial_port: file
        })
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

    async fn send_ack(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let ack = [1];
        self.serial_port.write(&ack).await?;
        self.serial_port.flush().await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        Ok(())
    }

    pub async fn get_message_from_pico(&mut self) -> Result<FromIMU, Box<dyn std::error::Error>> {
        self.send_ack().await?;
        let mut data: [u8; 13] = [0; 13];
        let readcount = self.serial_port.read(&mut data).await?;
        return Ok(FromIMU::deserialize(data).map_err(|e| {
            error!("Error deserializing message FromIMU: {e}");
            std::io::Error::new(std::io::ErrorKind::InvalidData, "failed to deserialize FromIMU from the serial port")
        })?)
    }
}