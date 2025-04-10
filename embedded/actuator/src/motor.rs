use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{PWM_SLICE0, PWM_SLICE4};
use embassy_rp::pwm::{Config as PwmConfig, Pwm, PwmError, SetDutyCycle};
use defmt::{error, info, warn};

pub struct Motor<'d> {
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