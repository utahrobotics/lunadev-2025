#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{PWM_SLICE0, PWM_SLICE4};
use embassy_rp::pwm::{Config as PwmConfig, Pwm, PwmError, SetDutyCycle};
use embassy_time::{Duration, Timer};
use {defmt_rtt as _, panic_probe as _};

struct Motor<'d> {
    // m1_slp (active=high)
    sleep: Output<'d>,

    // 0 forward, 1 backward
    dir: Output<'d>,

    // speed
    pwm: Pwm<'d>,
}

impl<'d> Motor<'d> {
    /// Hardware Connections:
    /// - Motor Sleep Pin: GPIO 10
    /// - Motor Direction Pin: GPIO 15
    /// - Motor PWM Pin: GPIO 9
    pub fn new(
        sleep_pin: embassy_rp::peripherals::PIN_10,
        dir_pin: embassy_rp::peripherals::PIN_15,
        pwm_pin: embassy_rp::peripherals::PIN_9,
        pwm_slice: PWM_SLICE4,
    ) -> Self {
        let sleep = Output::new(sleep_pin, Level::High);
        let dir = Output::new(dir_pin, Level::Low);

        let pwm = Pwm::new_output_b(pwm_slice, pwm_pin, PwmConfig::default());

        Motor { sleep, dir, pwm }
    }

    /// set motor speed
    pub fn set_speed(&mut self, speed: u16) -> Result<(), PwmError> {
        let max_duty = self.pwm.max_duty_cycle();
        if speed <= max_duty {
            self.pwm.set_duty_cycle(speed)?;
            info!("Motor speed set to: speed: {}, max duty: {}", speed, max_duty);
        } else {
            warn!(
                "Error: Speed {} must be between 0 and {}",
                speed, max_duty
            );
        }
        Ok(())
    }

    /// set direction
    pub fn set_direction(&mut self, forward: bool) {
        if forward {
            self.dir.set_low();
            info!("Motor direction set to forward");
        } else {
            self.dir.set_high();
            info!("Motor direction set to backward");
        }
    }

    pub fn enable(&mut self) {
        self.sleep.set_high();
        info!("Motor enabled");
    }

    pub fn disable(&mut self) {
        self.sleep.set_low();
        info!("Motor disabled");
    }

    pub fn get_max_duty(&self) -> u16 {
        self.pwm.max_duty_cycle()
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Initializing peripherals...");
    let p = embassy_rp::init(Default::default());

    let mut motor = Motor::new(p.PIN_10, p.PIN_15, p.PIN_9, p.PWM_SLICE4);

    info!("Motor initialized. Max duty cycle: {}", motor.get_max_duty());

    motor.enable();

    spawner.spawn(motor_task(motor)).unwrap();
}

#[embassy_executor::task(pool_size = 1)]
async fn motor_task(mut motor: Motor<'static>) {
    info!("Starting motor test");

    let speed = 30000;
    loop {
        info!("seting direction to forward and speed to: {}", speed);
        motor.set_direction(true);
        motor.set_speed(speed);
        Timer::after(Duration::from_secs(2)).await;

        info!("seting direction to backward and speed to: {}", speed);
        motor.set_direction(false);
        motor.set_speed(speed);
        Timer::after(Duration::from_secs(2)).await;

        info!("Stopping motor");
        motor.set_speed(0);
        Timer::after(Duration::from_secs(1)).await;
    }
}