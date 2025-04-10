#![no_std]
#![no_main]

use core::net::Ipv4Addr;
use core::panic::PanicInfo;

use accelerometer::vector::F32x3;
use cyw43::JoinOptions;
use cyw43_pio::{PioSpi, DEFAULT_CLOCK_DIVIDER};
use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_net::tcp::{ConnectError, TcpSocket};
use embassy_net::{Config, IpEndpoint, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{Async, I2c};
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::peripherals::{I2C0, USB};
use embassy_rp::pio::{self, Pio};
use embassy_rp::usb::Driver;
use embassy_rp::{bind_interrupts, i2c, usb, Peripherals};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Delay, Duration, Ticker, Timer};
use embassy_usb::class::cdc_acm::CdcAcmClass;
use embassy_usb::class::cdc_acm::State;
use embassy_usb::UsbDevice;
use embedded_common::AccelerationNorm;
use embedded_common::AngularRate;
use embedded_common::FromIMU;
use embedded_common::IMU_READING_DELAY_MS;
use lsm6dsox::accelerometer::Accelerometer;
use lsm6dsox::types::Error;
use lsm6dsox::*;
use rand::RngCore;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};


bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p: Peripherals = embassy_rp::init(Default::default());
    // Create the driver, from the HAL.
    let driver = Driver::new(p.USB, Irqs);

    const SERIAL_NUMBER: Option<&str> = option_env!("IMU_SERIAL");

    // Create embassy-usb Config
    let config = {
        let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
        config.manufacturer = Some("USR");
        config.product = Some("IMU");
        config.serial_number = SERIAL_NUMBER;
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

    // Build the builder.
    let usb = builder.build();

    // calls usb.run()
    spawner.spawn(usb_task(usb)).unwrap();
    
    spawner.spawn(okay_task()).unwrap();

    class.wait_connection().await;
    let i2c = I2c::new_async(p.I2C0, p.PIN_1, p.PIN_0, Irqs, i2c::Config::default());
    static LSM: StaticCell<Lsm6dsox<I2c<'_, I2C0, Async>, Delay>> = StaticCell::new();
    let lsm = LSM.init(lsm6dsox::Lsm6dsox::new(i2c, SlaveAddress::High, Delay {}));

    match setup_lsm(lsm) {
        Ok(_id) => {
            let _ = spawner.spawn(read_sensors_loop(lsm, IMU_READING_DELAY_MS, class));
        }
        Err(e) => {
            error!("lsm failed to setup: {:?}", e);
        }
    }
}

fn setup_lsm(lsm: &mut Lsm6dsox<I2c<'_, I2C0, Async>, Delay>) -> Result<u8, lsm6dsox::Error> {
    lsm.setup()?;
    lsm.set_gyro_sample_rate(DataRate::Freq52Hz)?;
    lsm.set_gyro_scale(GyroscopeScale::Dps2000)?;
    lsm.set_accel_sample_rate(DataRate::Freq52Hz)?;
    lsm.set_accel_scale(AccelerometerScale::Accel4g)?;
    lsm.check_id().map_err(|e| {
        error!("error checking id of lsm6dsox: {:?}", e);
        lsm6dsox::Error::NotSupported
    })
}

#[embassy_executor::task]
async fn okay_task() {
    Timer::after(Duration::from_secs(10)).await;
    info!("IMU is okay");
}


#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB>>) -> ! {
    usb.run().await
}


/// uses lsm to read in sensor data, then sends AngularRate and AccelerationNorm over ttyACM0
#[embassy_executor::task]
async fn read_sensors_loop(
    lsm: &'static mut Lsm6dsox<I2c<'static, I2C0, Async>, Delay>,
    delay_ms: u64,
    mut class: CdcAcmClass<'static, Driver<'static, USB>>,
) -> ! {
    let mut ticker = Ticker::every(Duration::from_millis(delay_ms));
    loop {
        let mut ack = [0u8];
        if let Err(e) = class.read_packet(&mut ack).await {
            error!("failed to read packet: {}", e);
            continue;
        }
        let rate = match lsm.angular_rate() {
            Ok(lsm6dsox::AngularRate { x, y, z }) => {
                // info!("gyro: x: {}, y: {}, z: {} (radians per sec)", x.as_radians_per_second(),y.as_radians_per_second(),z.as_radians_per_second());
                AngularRate {
                    x: x.as_radians_per_second() as f32,
                    y: y.as_radians_per_second() as f32,
                    z: z.as_radians_per_second() as f32
                }
            }
            Err(e) => {
                if Error::NoDataReady == e {
                    let _ = class.write_packet(&FromIMU::NoDataReady.serialize()).await;
                } else {
                    let _ = class.write_packet(&FromIMU::Error.serialize()).await;
                }
                error!("failed to read gyro: {:?}", e);
                continue;
            }
        };
        let accel = match lsm.accel_norm() {
            Ok(F32x3 { x, y, z }) => {
                // info!("accel: x: {}, y: {}, z: {} m/s normalized", x,y,z);
                AccelerationNorm { x, y, z }
            }
            Err(e) => {
                if Some(&Error::NoDataReady) == e.cause() {
                    let _ = class.write_packet(&FromIMU::NoDataReady.serialize()).await;
                } else {
                    let _ = class.write_packet(&FromIMU::Error.serialize()).await;
                    error!("failed to read accel: {:?}", e.cause());
                }
                continue;
            }
        };
        let _ = class.write_packet(&FromIMU::Reading(rate, accel).serialize()).await;
        ticker.next().await;
    }
}
