//! This example shows how to use USB (Universal Serial Bus) in the RP2040 chip.
//!
//! This creates a USB serial port that echos.

#![no_std]
#![no_main]

use accelerometer::vector::{F32x3, I16x3};
use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::pwm::{self, Pwm};
use embassy_rp::{bind_interrupts, gpio, i2c, Peripherals};
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{Async, I2c};
use embassy_rp::peripherals::{I2C0, USB};
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_time::{Delay, Duration, Ticker, Timer};
use embassy_usb::class::cdc_acm::CdcAcmClass;
use embassy_usb::UsbDevice;
use embassy_usb::class::cdc_acm::State;
use embedded_common::AccelerationNorm;
use embedded_common::AngularRate;
use embedded_common::FromIMU;
use static_cell::StaticCell;
use lsm6dsox::*;
use lsm6dsox::accelerometer::Accelerometer;
use lsm6dsox::accelerometer::RawAccelerometer;
use {defmt_rtt as _, panic_probe as _}; // global logger

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
});

#[macro_export]
macro_rules! unwrap {
    ($e:expr) => {
        match $e {
            Ok(x) => x,
            Err(e) => {
                log::error!("Unwrap failed: {:?}", e);
                panic!("Unwrap failed: {:?}", e);
            }
        }
    };
}


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p: Peripherals = embassy_rp::init(Default::default());
    // Create the driver, from the HAL.
    let driver = Driver::new(p.USB, Irqs);

    // Create embassy-usb Config
    let config = {
        let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
        config.manufacturer = Some("Embassy");
        config.product = Some("USB-serial example");
        config.serial_number = Some("12345678");
        config.max_power = 100;
        config.max_packet_size_0 = 64;
        config
    };

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut builder = {
        static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

        let builder = embassy_usb::Builder::new(
            driver,
            config,
            CONFIG_DESCRIPTOR.init([0; 256]),
            BOS_DESCRIPTOR.init([0; 256]),
            &mut [], // no msos descriptors
            CONTROL_BUF.init([0; 64]),
        );
        builder
    };

    // ttyACM0 for writing sensor data over usb to lunabot
    let mut class = {
        static CLASS_STATE: StaticCell<State> = StaticCell::new();
        let state = CLASS_STATE.init(State::new());
        CdcAcmClass::new(&mut builder, state, 64)
    };

    // ttyACM1 for logging
    let mut logger_class = {
        static STATE: StaticCell<State> = StaticCell::new();
        let state = STATE.init(State::new());
        CdcAcmClass::new(&mut builder, state, 64)
    };

    // Build the builder.
    let mut usb = builder.build();

    // task for writing logs to ttyACM1
    spawner.spawn(logger_task(logger_class));

    // calls usb.run()
    spawner.spawn(usb_task(usb));

    class.wait_connection().await;
    let i2c = I2c::new_async(p.I2C0, p.PIN_1, p.PIN_0, Irqs, i2c::Config::default());
    static LSM: StaticCell<Lsm6dsox<I2c<'_, I2C0, Async>, Delay>> = StaticCell::new();
    let lsm = LSM.init(lsm6dsox::Lsm6dsox::new(i2c, SlaveAddress::High, Delay{}));
    match setup_lsm(lsm) {
        Ok(id) => {
            log::info!("lsm setup sucessfully, id: {}", id);
            let _ = spawner.spawn(read_sensors_loop(lsm, 100, class));
        }
        Err(e) => {
            log::error!("lsm failed to setup: {:?}", e);
        }
    }
    
    loop {
        log::info!("Tick");
        Timer::after_secs(1).await;
    }

}

fn setup_lsm(lsm: &mut Lsm6dsox<I2c<'_, I2C0, Async>, Delay>) -> Result<u8, lsm6dsox::Error> {
    lsm.setup()?;
    lsm.set_gyro_sample_rate(DataRate::Freq52Hz)?;
    lsm.set_gyro_scale(GyroscopeScale::Dps2000)?;
    lsm.set_accel_sample_rate(DataRate::Freq52Hz)?;
    lsm.set_accel_scale(AccelerometerScale::Accel4g)?;
    lsm.check_id().map_err(|e| {
        log::error!("error checking id of lsm6dsox: {:?}", e);
        lsm6dsox::Error::NotSupported
    })
}


/// UNTESTED
fn initialize_motors(p: Peripherals) {
    let m1_slp = Output::new(p.PIN_10, Level::Low);
    let m1_dir = Output::new(p.PIN_15, Level::Low);
    let m1_pwm = Pwm::new_output_b(p.PWM_SLICE4, p.PIN_9, pwm::Config::default());
}

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB>>) -> ! {
    usb.run().await
}

#[embassy_executor::task]
async fn logger_task(mut class: CdcAcmClass<'static, Driver<'static, USB>>) {
    embassy_usb_logger::with_class!(1024, log::LevelFilter::Info, class).await;
}

#[embassy_executor::task]
async fn pos_handler() {
    let mut ticker = Ticker::every(Duration::from_millis(10));
    loop {
        // position handling stuff
        ticker.next().await;
    }
}

/// uses lsm to read in sensor data, then sends AngularRate and AccelerationNorm over ttyACM0
#[embassy_executor::task]
async fn read_sensors_loop(lsm: &'static mut Lsm6dsox<I2c<'static, I2C0, Async>, Delay>, delay_ms: u64, mut class: CdcAcmClass<'static, Driver<'static, USB>>) {
    let mut ticker = Ticker::every(Duration::from_millis(delay_ms));
    loop {
        match lsm.angular_rate() {
            Ok(lsm6dsox::AngularRate{x,y,z}) => {
                log::info!("gyro: x: {}, y: {}, z: {} (radians per sec)", x.as_radians_per_second(),y.as_radians_per_second(),z.as_radians_per_second());
                unwrap!(class.write_packet(
                    &FromIMU::AngularRateReading(AngularRate{
                        x: x.as_radians_per_second() as f32,
                        y: y.as_radians_per_second() as f32,
                        z: z.as_radians_per_second() as f32}
                    ).serialize()
                ).await);
            }
            Err(e) => {
                log::error!("failed to read gyro: {:?}", e);
            }
        }
        match lsm.accel_norm() {
            Ok(F32x3{x,y,z}) => {
                log::info!("accel: x: {}, y: {}, z: {} m/s normalized", x,y,z);
                unwrap!(class.write_packet(
                    &FromIMU::AccellerationNormReading(AccelerationNorm{
                        x,
                        y,
                        z
                    }).serialize()
                ).await);
            }
            Err(e) => {
                log::error!("failed to read accel: {:?}", e);
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
