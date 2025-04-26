# App Configs
Move the appropriate `app-config.toml` into the root directory.

## Fields

### Main.rerun_viz
* Log(Level)
* Viz(Level)
* Disabled (default)

Level can be All, or Minimal. Minimal only logs robot isometry, april tags, and expanded obstacle map. All will log height maps and depth image point clouds.

### Main.lunabase_address


### Main.cameras


### Main.apriltags

### Main.vesc

### Main.vesc_pairs

### Main.imu_correction
To generate IMU corrections run '''cargo make calibrate''' while the robot is on a flat surface.


* accelerometer_bias - [f64; 3] - Additive bias to the accelerometer.
* gyroscope_bias - [f64; 3] - Additive bias to the gyroscope.
* accelerometer_scale - [f64; 3] - Scaling bias.
* gyroscope_scale - [f64; 3] - Scaling bias.
* with_scaling - bool - Weather or not to try calculating scaling bias, included as an option but not needed in most cases.
* misalignment - [f64; 4] - Quaternion representing orientation misalignment. 


### Main.v3pico

* imus: an array of 4 IMUInfo structs
* serial: serial number to identify pico.
