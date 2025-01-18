use embedded_common::*;
use tokio::{self, io::{AsyncReadExt, AsyncWriteExt}};
use tracing::error;

pub struct PicoController {
    serial_port: tokio::fs::File
}


impl PicoController {
    pub async fn new(serial_port: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let file = tokio::fs::OpenOptions::new().read(true).write(true).open(serial_port).await?;
        Ok(PicoController{
            serial_port: file
        })
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
        self.serial_port.read(&mut data).await?;
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

        for _ in 0..100 {
            match controller.get_message_from_pico().await {
                Ok(msg) => println!("{msg:?}"),
                Err(e) => {
                    eprintln!("{e}");
                    panic!("FAILED TO GET MESSAGE")
                }
            }
        }
    }
}