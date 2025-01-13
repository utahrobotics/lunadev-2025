//! This example shows how to use USB (Universal Serial Bus) in the RP2040 chip.
//!
//! This creates a USB serial port that echos.

#![no_std]
#![no_main]

use cortex_m::peripheral::SYST;
use cortex_m::prelude::_embedded_hal_blocking_delay_DelayMs;
use defmt::{info, panic, unwrap, Formatter};
use core::fmt::write;
use core::fmt::Write;
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
use {defmt_rtt as _, panic_probe as _};
use lsm6dsox::accelerometer::{ Accelerometer};
use lsm6dsox::*;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    // Create the driver, from the HAL.
    let driver = Driver::new(p.USB, Irqs);

    // blink twice indicating setup
    let mut indicator_1 = gpio::Output::new(p.PIN_7, Level::Low);
    let mut indicator_2 = gpio::Output::new(p.PIN_8, Level::Low);
    blink_twice(&mut indicator_1, &mut indicator_2).await;

    let i2c = I2c::new_async(p.I2C0, p.PIN_1, p.PIN_0, Irqs, i2c::Config::default());
    let mut lsm = lsm6dsox::Lsm6dsox::new(i2c, SlaveAddress::Low, Delay{});
    lsm.setup().unwrap();
    lsm.set_gyro_sample_rate(DataRate::Freq52Hz).unwrap();
    lsm.set_gyro_scale(GyroscopeScale::Dps2000).unwrap();
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

    // Create classes on the builder.
    let mut class = {
        static STATE: StaticCell<State> = StaticCell::new();
        let state = STATE.init(State::new());
        CdcAcmClass::new(&mut builder, state, 64)
    };

    // Build the builder.
    let usb = builder.build();

    // Run the USB device.
    unwrap!(spawner.spawn(usb_task(usb)));

    // Do stuff with the class!
    loop {
        class.wait_connection().await;
        info!("Connected");
        let _ = read_sensors_loop(&mut lsm, &mut class).await;
        info!("Disconnected");
    }
}

type MyUsbDriver = Driver<'static, USB>;
type MyUsbDevice = UsbDevice<'static, MyUsbDriver>;

#[embassy_executor::task]
async fn usb_task(mut usb: MyUsbDevice) -> ! {
    usb.run().await
}

struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => Disconnected {},
        }
    }
}
async fn read_line<'d, T: Instance + 'd>(
    class: &mut CdcAcmClass<'d, Driver<'d, T>>,
    line_buffer: &mut [u8],
) -> Result<usize, Disconnected> {
    let mut packet_buffer = [0; 64];
    let mut buffer_pos = 0;

    while buffer_pos < line_buffer.len() {
        let n = class.read_packet(&mut packet_buffer).await?;
        let packet_data = &packet_buffer[..n];
        if let Some(newline_pos) = packet_data.iter().position(|&b| b == b'\r') {
            let bytes_to_copy = (newline_pos + 1).min(line_buffer.len() - buffer_pos);
            line_buffer[buffer_pos..buffer_pos + bytes_to_copy]
                .copy_from_slice(&packet_data[..bytes_to_copy]);

            return Ok(buffer_pos + bytes_to_copy);
        }
        
        let bytes_to_copy = n.min(line_buffer.len() - buffer_pos);
        line_buffer[buffer_pos..buffer_pos + bytes_to_copy]
            .copy_from_slice(&packet_data[..bytes_to_copy]);

        buffer_pos += bytes_to_copy;
    }

    Ok(buffer_pos)
}

async fn echo<'d, T: Instance + 'd>(
    class: &mut CdcAcmClass<'d, Driver<'d, T>>
) -> Result<(), Disconnected> {
    let mut line_buffer = [0u8; 256]; // Stack-allocated buffer
    loop {
        let n = read_line(class, &mut line_buffer).await?;
        class.write_packet("\n\r".as_bytes()).await;
        class.write_packet(&line_buffer[..n]).await?;
    }
}

#[embassy_executor::task]
async fn pos_handler() {
    let mut ticker = Ticker::every(Duration::from_millis(10));
    loop {
        // position handling stuff
        ticker.next().await;
    }
}

async fn read_sensors_loop<'d, T: Instance + 'd>(lsm: &mut Lsm6dsox<I2c<'_, I2C0, Async>, Delay>, class: &mut CdcAcmClass<'d, Driver<'d, T>>) {
    let mut ticker = Ticker::every(Duration::from_millis(10));
    loop {
        match lsm.angular_rate() {
            Ok(AngularRate{x,y,z}) => {
                let mut out: heapless::String<64> = heapless::String::new();
                write!(out, "x: {}, y: {}, z: {}\n\r", x.as_radians_per_second(),y.as_radians_per_second(),z.as_radians_per_second());
                class.write_packet(out.as_bytes()).await.unwrap();
            }
            Err(_) => {
                class.write_packet("failed to read angular rate".as_bytes()).await.unwrap();
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