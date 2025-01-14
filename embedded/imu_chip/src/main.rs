//! This example shows how to use USB (Universal Serial Bus) in the RP2040 chip.
//!
//! This creates a USB serial port that echos.

#![no_std]
#![no_main]

use cortex_m::prelude::_embedded_hal_blocking_delay_DelayMs;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::{bind_interrupts, gpio, i2c};
use embassy_rp::gpio::Level;
use embassy_rp::i2c::{Async, I2c};
use embassy_rp::peripherals::{I2C0, USB};
use embassy_rp::pio::Pin;
use embassy_rp::usb::{Driver, Instance, InterruptHandler};
use embassy_time::{Delay, Duration, Ticker, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::UsbDevice;
use static_cell::StaticCell;
use lsm6dsox::accelerometer::Accelerometer;
use lsm6dsox::*;
use core::panic::PanicInfo;
use {defmt_rtt as _, panic_probe as _}; // global logger

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    // Create the driver, from the HAL.
    let driver: Driver<USB> = Driver::new(p.USB, Irqs);
    spawner.spawn(logger_task(driver));
    Timer::after_millis(5000).await;

    let i2c = I2c::new_async(p.I2C0, p.PIN_1, p.PIN_0, Irqs, i2c::Config::default());
    static LSM: StaticCell<Lsm6dsox<I2c<'_, I2C0, Async>, Delay>> = StaticCell::new();
    let lsm = LSM.init(lsm6dsox::Lsm6dsox::new(i2c, SlaveAddress::Low, Delay{}));
    match setup_lsm(lsm) {
        Ok(id) => log::info!("lsm setup sucessfully, id: {}", id),
        Err(e) => {
            loop {
                log::error!("lsm failed to setup: {:?}", e);
                Timer::after_secs(1).await;
            }
        }
    }
    spawner.spawn(read_sensors_loop(lsm));
    let mut counter = 0;
    loop {
        counter += 1;
        log::info!("Tick {}", counter);
        Timer::after_secs(1).await;
    }

}

fn setup_lsm(lsm: &mut Lsm6dsox<I2c<'_, I2C0, Async>, Delay>) -> Result<u8, lsm6dsox::Error> {
    lsm.setup()?;
    lsm.set_gyro_sample_rate(DataRate::Freq52Hz)?;
    lsm.set_gyro_scale(GyroscopeScale::Dps2000)?;
    lsm.check_id().map_err(|e| {lsm6dsox::Error::NotSupported})
}

#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, USB>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}

#[embassy_executor::task]
async fn pos_handler() {
    let mut ticker = Ticker::every(Duration::from_millis(10));
    loop {
        // position handling stuff
        ticker.next().await;
    }
}

#[embassy_executor::task]
async fn read_sensors_loop(lsm: &'static mut Lsm6dsox<I2c<'static, I2C0, Async>, Delay>) {
    let mut ticker = Ticker::every(Duration::from_millis(10));
    loop {
        match lsm.angular_rate() {
            Ok(AngularRate{x,y,z}) => {

            }
            Err(_) => {
            }
        }
        ticker.next().await;
    }
}

async fn blink_twice<'a>(p1: &mut gpio::Output<'a>, p2: &mut gpio::Output<'a>) {
    for _ in 0..2 {
        p1.set_high();
        p2.set_high();
        Timer::after_millis(200).await;
        p1.set_low();
        p2.set_low();
    }
}