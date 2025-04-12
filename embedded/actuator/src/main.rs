#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, peripherals::USB, usb::{self, Driver}};
use embassy_time::{Duration, Timer};
use embassy_usb::{class::cdc_acm::{CdcAcmClass, State}, UsbDevice};
use embedded_common::ActuatorCommand;
use static_cell::StaticCell;
use defmt::{info, error};
use {defmt_rtt as _, panic_probe as _};
mod motor;
use motor::*;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
});


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    
    info!("Initializing peripherals...");

    let p = embassy_rp::init(Default::default());

    let mut motor = Motor::new_m2(p.PIN_17, p.PIN_14, p.PIN_16, p.PWM_SLICE0);

    info!("Motor initialized. Max duty cycle: {}", motor.get_max_duty());

    let driver = Driver::new(p.USB, Irqs);

    const SERIAL_NUMBER: Option<&str> = option_env!("ACTUATOR_SERIAL");

    // Create embassy-usb Config
    let config = {
        let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
        config.manufacturer = Some("USR");
        config.product = Some("ACTUATOR");
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

    // tty for communicating with lunabot
    let mut class = {
        static CLASS_STATE: StaticCell<State> = StaticCell::new();
        let state = CLASS_STATE.init(State::new());
        CdcAcmClass::new(&mut builder, state, 64)
    };
    let usb = builder.build();
    spawner.spawn(usb_task(usb)).unwrap();

    class.wait_connection().await;

    motor.enable();

    // spawner.spawn(motor_test_task(motor)).unwrap();
    spawner.spawn(motor_controller_loop(class,motor)).unwrap();
}

#[embassy_executor::task(pool_size = 1)]
async fn motor_controller_loop(mut class: CdcAcmClass<'static, Driver<'static, USB>>, mut motor: Motor<'static>) {
    loop {
        let mut cmd = [0,0,0u8];
        if let Err(e) = class.read_packet(&mut cmd).await {
            error!("failed to read packet: {}", e);
            continue;
        } else {
            info!("read packet");
        }

        // deserialize actuator command
        let Ok(cmd) = embedded_common::ActuatorCommand::deserialize(cmd) else {
            warn!("failed to deserialize actuator command: {:?}", cmd);
            continue;
        };

        match cmd {
            ActuatorCommand::SetSpeed(speed) => {
                if let Err(e) = motor.set_speed(speed) {
                    error!("Couldn't set motor speed to {}: Invalid Duty Cycle", speed);
                }
            }
            ActuatorCommand::SetDirection(dir) =>{
                info!("got dir: {:?}", dir);
                motor.set_direction(dir as u8 == 0);
            }
        }
    }
}

#[embassy_executor::task(pool_size = 1)]
async fn motor_test_task(mut motor: Motor<'static>) {
    info!("Starting motor test");

    let speed = 30000;
    loop {
        info!("seting direction to forward and speed to: {}", speed);
        motor.set_direction(true);
        expect!(motor.set_speed(speed), "couldnt set speed");
        Timer::after(Duration::from_secs(2)).await;

        info!("seting direction to backward and speed to: {}", speed);
        motor.set_direction(false);
        expect!(motor.set_speed(speed), "couldnt set speed");
        Timer::after(Duration::from_secs(2)).await;

        info!("Stopping motor");
        expect!(motor.set_speed(speed), "couldnt set speed to 0");
        Timer::after(Duration::from_secs(1)).await;
    }
}

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB>>) -> ! {
    usb.run().await
}