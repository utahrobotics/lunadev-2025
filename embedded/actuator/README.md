## Setup
1. Install [probe-rs](https://probe.rs)
2. Attach debug probe to the rpi pico.

## Usage
1. Set the ACTUATOR_SERIAL environment variable to anything, because that will be used to set the serial number of the pico when it is pluged in via micro usb so that the computer can detect that it is there.
2. ```cargo run``` will use probe-rs to flash the firmware to the pico.
3. The pico will wait for a connection via microusb before begining the motor test.


Currently this program should just move the actuator up and down: 
```rust
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
```


All logs are sent over the probe and are displayed by probe-run when you call cargo run.