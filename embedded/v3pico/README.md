### Usage

1. Flashing Firmware
    * First set the PICO_SERIAL env variable to a number representing what you want the picos serial number to be.
    * ensure debug probe is attached
    * execute cargo run

2. Interacting with the pico
    * Each pico will create a new device /dev/ttyACM* with the serial number set to whatever the environment variable was.


### Features

1. Actuator control
2. Actuator length reading
