use embedded_common::*;
use tokio::{self, fs, io::{AsyncReadExt, AsyncWriteExt}, process::Command};
use tokio_serial::SerialStream;
use tracing::{error, info};

pub struct PicoController {
    serial_port: SerialStream
}


impl PicoController {

    pub async fn auto_discover() -> Result<Self, Box<dyn std::error::Error>> {
        let mut tty_entries = fs::read_dir("/sys/class/tty/").await?;
        let mut acm_paths: Vec<String> = Vec::new();
        while let Ok(Some(entry)) = tty_entries.next_entry().await {
            if let Some(entry) = entry.file_name().to_str() {
                if entry.starts_with("ttyACM") {
                    acm_paths.push(format!("/dev/{}", entry));
                }
            }
        }
        acm_paths.sort();
        if let Some(entry) = acm_paths.get(0) {
            Self::check_udevadm_info(entry).await?;
            return Self::new(&entry).await;
        } else {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::NotFound, "No ttyACM* found (is the pico pluged in?)")))
        }
    }

    pub async fn new(serial_port: &str) -> Result<Self, Box<dyn std::error::Error>> {
        eprintln!("opening {serial_port}");
        let builder = tokio_serial::new(serial_port, 9600);
        let file  = SerialStream::open(&builder)?;
        // let file = tokio::fs::OpenOptions::new().read(true).write(true).open(serial_port).await?;
        Ok(PicoController{
            serial_port: file
        })
    }

    /// makes sure the serial number is Embassy_USB-serial_12345678
    async fn check_udevadm_info(path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let udevadm = Command::new("udevadm").arg("info").arg(path).output().await?;
        if String::from_utf8_lossy(&udevadm.stdout).contains(UDEVADM_ID) {
            return Ok(());
        } else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{path} has the wrong serial number")
            )))
        }
    }

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

#[cfg(test)]
mod tests {

    use crate::PicoController;

    #[tokio::test]
    async fn read_loop() {
        let mut controller = PicoController::new("/dev/ttyACM0").await.unwrap();
        loop {
            match controller.get_message_from_pico().await {
                Ok(msg) => println!("{msg:?}"),
                Err(e) => {
                    eprintln!("{e}");
                }
            }
        }
    }

    #[tokio::test] 
    async fn test_autodiscovery() {
        let controller = PicoController::auto_discover().await.unwrap();
    }
}