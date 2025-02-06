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
use lsm6dsox::accelerometer::Accelerometer;
use lsm6dsox::types::Error;
use lsm6dsox::*;
use rand::RngCore;
use static_cell::StaticCell;

#[inline(never)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    error!("{}", info);
    loop {
        // core::hint::spin_loop();
    }
}

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

static LOGGING_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static LOGGING_MUTEX: Mutex<CriticalSectionRawMutex, [u8; 256]> = Mutex::new([0; 256]);

#[defmt::global_logger]
struct Logger;

unsafe impl defmt::Logger for Logger {
    fn acquire() {
    }

    unsafe fn flush() {
        
    }

    unsafe fn release() {
    }

    unsafe fn write(mut bytes: &[u8]) {
        loop {
            if LOGGING_SIGNAL.signaled() {
                // Do nothing
            } else if let Ok(mut guard) = LOGGING_MUTEX.try_lock() {
                let buffer_len = guard.len();
                if buffer_len >= bytes.len() {
                    guard[..bytes.len()].copy_from_slice(bytes);
                    break;
                } else {
                    guard.copy_from_slice(&bytes[..buffer_len]);
                    bytes = &bytes[buffer_len..];
                }
            }
            core::hint::spin_loop();
        }
    }
}

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

    // // ttyACM1 for logging
    // let logger_class = {
    //     static STATE: StaticCell<State> = StaticCell::new();
    //     let state = STATE.init(State::new());
    //     CdcAcmClass::new(&mut builder, state, 64)
    // };
    // // task for writing logs to ttyACM1
    // spawner.spawn(logger_task(logger_class)).unwrap();

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
            // info!("lsm setup sucessfully, id: {}", id);
            let _ = spawner.spawn(read_sensors_loop(lsm, 10, class));
        }
        Err(e) => {
            error!("lsm failed to setup: {:?}", e);
        }
    }
    
    return;

    let fw = include_bytes!("../../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../../cyw43-firmware/43439A0_clm.bin");

    let mut rng = RoscRng;

    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        DEFAULT_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        p.DMA_CH0,
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
    spawner.spawn(cyw43_task(runner)).unwrap();

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::None)
        .await;

    macro_rules! enable_led {
        ($enable: literal) => {
            control.gpio_set(0, $enable).await
        };
    }

    let config = Config::dhcpv4(Default::default());

    // Generate random seed
    let seed = rng.next_u64();

    // Init network stack
    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        config,
        RESOURCES.init(StackResources::new()),
        seed,
    );

    spawner.spawn(net_task(runner)).unwrap();

    loop {
        loop {
            match control
                .join(
                    option_env!("WIFI_NETWORK").unwrap_or("USR-Wifi"),
                    JoinOptions::new(option_env!("WIFI_PASSWORD").unwrap_or_default().as_bytes()),
                )
                .await
            {
                Ok(_) => break,
                Err(_error) => {
                    enable_led!(true);
                    Timer::after(Duration::from_secs(1)).await;
                    enable_led!(false);
                    Timer::after(Duration::from_secs(1)).await;
                }
            }
        }

        enable_led!(true);
        // Wait for DHCP, not necessary when using static IP
        while !stack.is_config_up() {
            Timer::after_millis(100).await;
        }
        enable_led!(false);
        for _ in 0..5 {
            enable_led!(true);
            Timer::after(Duration::from_millis(100)).await;
            enable_led!(false);
            Timer::after(Duration::from_millis(100)).await;
        }
        let mut rx_buffer = [0; 0];
        let mut tx_buffer = [0; 4096];
        loop {
            let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
            if let Err(error) = socket
                .connect(IpEndpoint::new(
                    embassy_net::IpAddress::Ipv4(Ipv4Addr::new(192, 168, 0, 102)),
                    30600,
                ))
                .await
            {
                if error == ConnectError::NoRoute {
                    break;
                }
                enable_led!(true);
                Timer::after(Duration::from_secs(1)).await;
                enable_led!(false);
                Timer::after(Duration::from_secs(1)).await;
                continue;
            }
            'logging: loop {
                LOGGING_SIGNAL.wait().await;
                let guard = LOGGING_MUTEX.lock().await;
                let mut to_send: &[u8] = &*guard;
                while !to_send.is_empty() {
                    match socket.write(to_send).await {
                        Ok(n) => {
                            to_send = &to_send[n..];
                        }
                        Err(_) => {
                            break 'logging;
                        }
                    }
                }
                enable_led!(true);
                Timer::after(Duration::from_millis(100)).await;
                enable_led!(false);
                Timer::after(Duration::from_millis(100)).await;
            }
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
async fn cyw43_task(
    runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn okay_task() {
    Timer::after(Duration::from_secs(10)).await;
    info!("IMU is okay");
}

/// UNTESTED
// fn initialize_motors(p: Peripherals) {
//     let m1_slp = Output::new(p.PIN_10, Level::Low);
//     let m1_dir = Output::new(p.PIN_15, Level::Low);
//     let m1_pwm = Pwm::new_output_b(p.PWM_SLICE4, p.PIN_9, pwm::Config::default());
// }

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB>>) -> ! {
    usb.run().await
}

// #[embassy_executor::task]
// async fn logger_task(class: CdcAcmClass<'static, Driver<'static, USB>>) {
//     embassy_usb_logger::with_class!(1024, LevelFilter::Info, class).await;
// }

// #[embassy_executor::task]
// async fn pos_handler() {
//     let mut ticker = Ticker::every(Duration::from_millis(10));
//     loop {
//         // position handling stuff
//         ticker.next().await;
//     }
// }

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
