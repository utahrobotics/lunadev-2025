use nalgebra::{Vector3, Matrix3, Normed};

use optimization::{Function, Function1, Minimizer, Evaluation, GradientDescent, ArmijoLineSearch, NumericalDifferentiation};
use std::vec::Vec;

const G_ACCEL: f64 = 9.81;
const NUM_PARAMS: usize = 12;

/// Represents biases and scale factors found by the minimizer.
/// Misalignment is omitted for initial simplicity but could be added later.
#[derive(Clone, Debug)]
pub struct CalibrationParameters {
    pub accelerometer_bias: Vector3<f64>,
    pub gyroscope_bias: Vector3<f64>,
    pub accelerometer_scale: Vector3<f64>,
    pub gyroscope_scale: Vector3<f64>,
}

impl CalibrationParameters {
    /// Creates parameters with zero bias and unit scale.
    pub fn identity() -> Self {
        CalibrationParameters {
            accelerometer_bias: Vector3::zeros(),
            gyroscope_bias: Vector3::zeros(),
            accelerometer_scale: Vector3::new(1.0, 1.0, 1.0),
            gyroscope_scale: Vector3::new(1.0, 1.0, 1.0),
        }
    }

    /// Flattens parameters into a vector for the optimizer.
    pub fn to_vec(&self) -> Vec<f64> {
        let mut params = Vec::with_capacity(NUM_PARAMS);
        params.extend_from_slice(self.accelerometer_bias.as_slice());
        params.extend_from_slice(self.gyroscope_bias.as_slice());
        params.extend_from_slice(self.accelerometer_scale.as_slice());
        params.extend_from_slice(self.gyroscope_scale.as_slice());
        params
    }

    /// Creates parameters from a flat vector provided by the optimizer.
    pub fn from_slice(params: &[f64]) -> Result<Self, &'static str> {
        if params.len() != NUM_PARAMS {
            return Err("Incorrect number of parameters in slice");
        }
        Ok(CalibrationParameters {
            accelerometer_bias: Vector3::new(params[0], params[1], params[2]),
            gyroscope_bias: Vector3::new(params[3], params[4], params[5]),
            accelerometer_scale: Vector3::new(params[6], params[7], params[8]),
            gyroscope_scale: Vector3::new(params[9], params[10], params[11]),
        })
    }

    /// Applies correction to a raw accelerometer reading.
    /// Assumes diagonal scale matrix for simplicity.
    pub fn correct_accelerometer(&self, raw: &Vector3<f64>) -> Vector3<f64> {
        let bias_corrected = raw - self.accelerometer_bias;
        Vector3::new(
            bias_corrected.x * self.accelerometer_scale.x,
            bias_corrected.y * self.accelerometer_scale.y,
            bias_corrected.z * self.accelerometer_scale.z,
        )
    }

    /// Applies correction to a raw gyroscope reading.
    /// Assumes diagonal scale matrix for simplicity.
    pub fn correct_gyroscope(&self, raw: &Vector3<f64>) -> Vector3<f64> {
        let bias_corrected = raw - self.gyroscope_bias;
        Vector3::new(
            bias_corrected.x * self.gyroscope_scale.x,
            bias_corrected.y * self.gyroscope_scale.y,
            bias_corrected.z * self.gyroscope_scale.z,
        )
    }
}


/// Holds the raw sensor readings collected during calibration procedures.
/// Assumes readings were taken when the IMU was STATIC in various orientations.
pub struct Calibrator {
    static_readings: Vec<(Vector3<f64>, Vector3<f64>)>,
}

impl Calibrator {
    pub fn new() -> Self {
        Calibrator {
            static_readings: Vec::new(),
        }
    }

    /// Adds a sample pair collected when the IMU was static.
    pub fn add_static_sample(&mut self, accel_raw: Vector3<f64>, gyro_raw: Vector3<f64>) {
        self.static_readings.push((accel_raw, gyro_raw));
    }

    /// Performs the calibration using the collected static samples.
    pub fn calibrate(&self) -> Result<CalibrationParameters, String> {
        if self.static_readings.len() < 6 {
            return Err("Insufficient static samples for calibration (need at least 6)".to_string());
        }

        let cost_function = ImuCostFunction {
            readings: &self.static_readings,
        };

        let cost_function_nd = NumericalDifferentiation::new(cost_function);

        let line_search = ArmijoLineSearch::new(0.5, 0.1, 0.5);
        let minimizer = GradientDescent::new()
            .line_search(line_search)
            .gradient_tolerance(1e-7)
            .max_iterations(Some(400));

        println!("Starting optimization...");
        let result = minimizer.minimize(&cost_function_nd, CalibrationParameters::identity().to_vec());

        let best_params_vec = result.position();
        CalibrationParameters::from_slice(best_params_vec)
            .map_err(|e| format!("Failed to parse final parameters: {}", e))

    }
}


struct ImuCostFunction<'a> {
    readings: &'a [(Vector3<f64>, Vector3<f64>)],
}

impl<'a> Function for ImuCostFunction<'a> {
    /// Calculates the total error for a given set of calibration parameters.
    fn value(&self, params_vec: &[f64]) -> f64 {
        let params = match CalibrationParameters::from_slice(params_vec) {
            Ok(p) => p,
            Err(_) => return f64::INFINITY,
        };

        let mut total_error = 0.0;

        for (accel_raw, gyro_raw) in self.readings {
            let accel_corrected = params.correct_accelerometer(accel_raw);
            let gyro_corrected = params.correct_gyroscope(gyro_raw);

            let accel_magnitude = accel_corrected.norm();
            let accel_error = (accel_magnitude - G_ACCEL).powi(2);

            let gyro_magnitude_sq = gyro_corrected.norm_squared();
            let gyro_error = gyro_magnitude_sq;

            total_error += accel_error + gyro_error;
        }

        if self.readings.is_empty() {
            0.0
        } else {
            total_error / (self.readings.len() as f64)
        }
    }
}