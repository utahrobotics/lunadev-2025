### Usage

1. Flashing Firmware
    * First set the IMU_SERIAL env variable to a number representing what you want the picos serial number to be.
    * Unplug the pico, hold down the bootsel button and plug it back in.
    * execute cargo run

2. Interacting with the pico
    * Each pico will create a new device /dev/ttyACM* with the serial number set to whatever the environment variable was.
