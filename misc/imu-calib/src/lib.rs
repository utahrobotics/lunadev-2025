#![allow(unused_parens)]
use nalgebra::{Quaternion, UnitQuaternion, UnitVector3, Vector3, Vector4};
use optimization::{Function, Minimizer, Evaluation, GradientDescent, ArmijoLineSearch, NumericalDifferentiation};
use std::vec::Vec;
use serde::{Deserialize, Serialize};

const G_ACCEL: f64 = 9.81;
const NUM_PARAMS: usize = 16;

/// Represents biases, scale factors, and misalignment found by the minimizer.
#[derive(Clone, Debug, Serialize, Deserialize, Copy)]
pub struct CalibrationParameters {
    pub accelerometer_bias: Vector3<f64>,
    pub gyroscope_bias: Vector3<f64>,
    pub accelerometer_scale: Vector3<f64>,
    pub gyroscope_scale: Vector3<f64>,
    pub with_scaling: bool,
    pub misalignment: UnitQuaternion<f64>
}

impl CalibrationParameters {
    /// Constructs a new set of calibration parameters from known biases and misalignment.
    pub fn new(
        accelerometer_bias: Vector3<f64>,
        gyroscope_bias: Vector3<f64>,
        accelerometer_scale: Vector3<f64>,
        gyroscope_scale: Vector3<f64>,
        misalignment: UnitQuaternion<f64>,
        with_scaling: bool
    ) -> Self {
        Self {
            accelerometer_bias,
            gyroscope_bias,
            accelerometer_scale,
            gyroscope_scale,
            with_scaling,
            misalignment
        }
    }

    pub fn serialize_to_string(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(toml::to_string(self)?)
    }

    /// Creates parameters with zero bias, unit scale, and identity misalignment.
    pub fn identity(with_scaling: bool) -> Self {
        Self::new(
            Vector3::zeros(),
            Vector3::zeros(),
            Vector3::new(1.0, 1.0, 1.0),
            Vector3::new(1.0, 1.0, 1.0),
            UnitQuaternion::identity(),
            with_scaling
        )
    }

    /// Creates parameters with a specified initial misalignment.
    pub fn with_initial_misalignment(initial_misalignment: UnitQuaternion<f64>, with_scaling: bool) -> Self {
         Self::new(
            Vector3::zeros(),
            Vector3::zeros(),
            Vector3::new(1.0, 1.0, 1.0),
            Vector3::new(1.0, 1.0, 1.0),
            initial_misalignment,
            with_scaling
        )
    }

    /// Flattens parameters into a vector for the optimizer.
    pub fn to_vec(&self) -> Vec<f64> {
        let mut params = Vec::with_capacity(NUM_PARAMS);
        params.extend_from_slice(self.accelerometer_bias.as_slice());
        params.extend_from_slice(self.gyroscope_bias.as_slice());
        params.extend_from_slice(self.accelerometer_scale.as_slice());
        params.extend_from_slice(self.gyroscope_scale.as_slice());
        params.extend_from_slice(self.misalignment.coords.as_slice());
        params
    }

    /// Creates parameters from a flat vector provided by the optimizer.
    pub fn from_slice(params: &[f64], with_scaling: bool) -> Result<Self, &'static str> {
        if params.len() != NUM_PARAMS {
            return Err("Incorrect number of parameters in slice");
        }
        let q_vec = Vector4::new(params[12], params[13], params[14], params[15]);
        let misalignment = UnitQuaternion::from_quaternion(Quaternion::from_vector(q_vec));
        Ok(CalibrationParameters {
            accelerometer_bias: Vector3::new(params[0], params[1], params[2]),
            gyroscope_bias: Vector3::new(params[3], params[4], params[5]),
            accelerometer_scale: Vector3::new(params[6], params[7], params[8]),
            gyroscope_scale: Vector3::new(params[9], params[10], params[11]),
            misalignment,
            with_scaling
        })
    }

    /// Applies correction to an accelerometer reading based on the model:
    /// corrected = diag(S)^-1 * R_misalign * (raw - bias)
    pub fn correct_accelerometer(&self, raw: &Vector3<f64>) -> Vector3<f64> {
        let bias_corrected = raw - self.accelerometer_bias;
        let rotated = self.misalignment * bias_corrected;
        if !self.with_scaling {
            return rotated;
        }
        Vector3::new(
            rotated.x / self.accelerometer_scale.x,
            rotated.y / self.accelerometer_scale.y,
            rotated.z / self.accelerometer_scale.z,
        )
    }

    /// Applies correction to a gyroscope reading based on the model:
    /// corrected = diag(S)^-1 * R_misalign * (raw - bias)
    pub fn correct_gyroscope(&self, raw: &Vector3<f64>) -> Vector3<f64> {
        // YOU CANT JUST APPLY A ROTATION TO EULER ANGLES. Ugh so obvious.
        let bias_corrected = (raw - self.gyroscope_bias);
        // TODO: figure out if the "convention" of the raw reading because depending on the correspondence between pitch roll and yaw, 
        // then convert it to a quaternion and apply the rotation
        if !self.with_scaling {
            return bias_corrected;
        }
        Vector3::new(
            bias_corrected.x / self.gyroscope_scale.x,
            bias_corrected.y / self.gyroscope_scale.y,
            bias_corrected.z / self.gyroscope_scale.z,
        )
    }
}

/// Holds the raw sensor readings collected during calibration procedures.
/// Provides functionality for using a Gradient Descent algorithm to to find the additive and scaling biases of the IMU.
pub struct Calibrator {
    static_readings: Vec<(Vector3<f64>, Vector3<f64>)>,
}

impl Calibrator {
    pub fn new() -> Self {
        Calibrator {
            static_readings: Vec::new(),
        }
    }

    pub fn avg_accel_vector(&self) -> Option<Vector3<f64>> {
        if self.static_readings.is_empty() {
            return None;
        }
        let mut sum: Vector3<f64> = Vector3::zeros();
        for reading in self.static_readings.iter() {
            sum += reading.0;
        }
        let avg = sum / (self.sample_count() as f64);
        Some(avg)
    }

    /// Returns the average raw gyroscope vector
    pub fn avg_gyro_vector(&self) -> Option<Vector3<f64>> {
        if self.static_readings.is_empty() {
            return None;
        }
        let mut sum: Vector3<f64> = Vector3::zeros();
        for reading in self.static_readings.iter() {
            sum += reading.1;
        }
        let avg = sum / (self.sample_count() as f64);
        Some(avg)
    }

    /// Adds a sample pair
    pub fn add_static_sample(&mut self, accel_raw: Vector3<f64>, gyro_raw: Vector3<f64>) {
        self.static_readings.push((accel_raw, gyro_raw));
    }

    pub fn reset(&mut self) {
        self.static_readings.clear()
    }

    /// Applies a moving average filter 
    /// `window_size` must be an odd number to have a symmetric window.
    pub fn smooth(&mut self, window_size: usize) -> Result<(), String> {
        let n = self.static_readings.len();
        if n == 0 {
            return Ok(());
        }
        if window_size == 0 || window_size % 2 == 0 {
            return Err("window_size must be a non-zero odd number".to_string());
        }
        if window_size >= n {
             if let (Some(avg_accel), Some(avg_gyro)) = (self.avg_accel_vector(), self.avg_gyro_vector()) {
                 self.static_readings = vec![(avg_accel, avg_gyro); n];
             }
             return Ok(());
        }
        let half_window = window_size / 2;
        let mut smoothed = Vec::with_capacity(n);
        for i in 0..n {
            let start = if i >= half_window { i - half_window } else { 0 };
            let end = std::cmp::min(n, i + half_window + 1);
            let count = (end - start) as f64;
            let mut sum_accel = Vector3::zeros();
            let mut sum_gyro = Vector3::zeros();
            for j in start..end {
                let (ref accel, ref gyro) = self.static_readings[j];
                sum_accel += accel;
                sum_gyro += gyro;
            }
            let avg_accel = sum_accel / count;
            let avg_gyro = sum_gyro / count;
            smoothed.push((avg_accel, avg_gyro));
        }
        self.static_readings = smoothed;
        Ok(())
    }

    pub fn sample_count(&self) -> usize {
        self.static_readings.len()
    }

    /// Estimate the initial misalignment based on the average acceleration vector.
    #[allow(unused)]
    fn estimate_initial_misalignment(&self) -> UnitQuaternion<f64> {
        let ideal_gravity_direction = UnitVector3::new_normalize(Vector3::new(0.0, -G_ACCEL, 0.0));
        if let Some(avg_accel) = self.avg_accel_vector() {
            if let Some(measured_direction) = UnitVector3::try_new(avg_accel, 1e-6) {
               return UnitQuaternion::rotation_between(&measured_direction, &ideal_gravity_direction)
                    .unwrap_or_else(UnitQuaternion::identity);
            }
        }
        UnitQuaternion::identity()
    }

    /// Performs the calibration using the collected static samples.
    pub fn calibrate(&mut self, with_scaling: bool) -> Result<CalibrationParameters, String> {
        if self.static_readings.len() < 6 {
            return Err("Insufficient static samples for calibration (need at least 6)".to_string());
        }
        self.smooth(15)?;
        let initial_misalignment = UnitQuaternion::identity();
        let cost_function = ImuCostFunction {
            readings: &self.static_readings,
            with_scaling,
        };
        let cost_function_nd = NumericalDifferentiation::new(cost_function);
        let line_search = ArmijoLineSearch::new(0.5, 0.9, 0.1);
        let minimizer = GradientDescent::new()
            .line_search(line_search)
            .gradient_tolerance(1e-9)
            .max_iterations(Some(20_000));
        
        let initial_params = CalibrationParameters::with_initial_misalignment(initial_misalignment, with_scaling);
        let initial_params_vec = initial_params.to_vec();
        println!("Starting optimization with initial params: {:?}", initial_params);
        let result = minimizer.minimize(&cost_function_nd, initial_params_vec);
        let best_params_vec = result.position();
        println!("Optimization finished");
        CalibrationParameters::from_slice(best_params_vec, with_scaling)
            .map_err(|e| format!("Failed to parse final parameters: {}", e))
    }
}

struct ImuCostFunction<'a> {
    readings: &'a [(Vector3<f64>, Vector3<f64>)],
    with_scaling: bool,
}

impl<'a> Function for ImuCostFunction<'a> {
    fn value(&self, params_vec: &[f64]) -> f64 {
        let ideal_accel = Vector3::new(0.0, -G_ACCEL, 0.0);
        let ideal_gyro = Vector3::new(0.0, 0.0, 0.0);
        let params = match CalibrationParameters::from_slice(params_vec, self.with_scaling) {
            Ok(p) => p,
            Err(_) => return f64::INFINITY,
        };
        let mut total_error = 0.0;
        let num_readings = self.readings.len() as f64;
        if num_readings == 0.0 {
            return 0.0;
        }
        let w_g = 10.0;
        let w_a = 5.0;
        let _w_m = 0.5;
        for (accel_raw, gyro_raw) in self.readings {
            let accel_corrected = params.correct_accelerometer(accel_raw);
            let gyro_corrected = params.correct_gyroscope(gyro_raw);
            let accel_diff = accel_corrected - ideal_accel;
            let accel_error = (accel_diff.norm_squared()*w_a).powi(2);
            let gyro_diff = gyro_corrected - ideal_gyro;
            let gyro_error = (gyro_diff.norm_squared()*w_g).powi(2);
            total_error += accel_error + gyro_error;
        }
        total_error / num_readings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;
    use rand::distr::Uniform;

    fn generate_test_sample(
        ideal_accel: &Vector3<f64>,
        ideal_gyro: &Vector3<f64>,
        accel_bias: &Vector3<f64>,
        gyro_bias: &Vector3<f64>,
        accel_scale: &Vector3<f64>,
        gyro_scale: &Vector3<f64>,
        misalignment: &UnitQuaternion<f64>,
        accel_noise_stddev: f64,
        gyro_noise_stddev: f64,
        rng: &mut impl Rng,
    ) -> (Vector3<f64>, Vector3<f64>)
    {
        let noise_dist = Uniform::new_inclusive(-accel_noise_stddev, accel_noise_stddev).unwrap();
        let accel_noise = Vector3::new(rng.sample(noise_dist), rng.sample(noise_dist), rng.sample(noise_dist));
        let gyro_noise_dist = Uniform::new_inclusive(-gyro_noise_stddev, gyro_noise_stddev).unwrap();
        let gyro_noise = Vector3::new(rng.sample(gyro_noise_dist), rng.sample(gyro_noise_dist), rng.sample(gyro_noise_dist));
        let noisy_accel = ideal_accel + accel_noise;
        let noisy_gyro = ideal_gyro + gyro_noise;
        let scaled_accel = Vector3::new(
            noisy_accel.x * accel_scale.x,
            noisy_accel.y * accel_scale.y,
            noisy_accel.z * accel_scale.z,
        );
         let scaled_gyro = Vector3::new(
            noisy_gyro.x * gyro_scale.x,
            noisy_gyro.y * gyro_scale.y,
            noisy_gyro.z * gyro_scale.z,
        );
        let rotated_accel = misalignment.inverse() * scaled_accel;
        let rotated_gyro = misalignment.inverse() * scaled_gyro;
        let raw_accel = rotated_accel + accel_bias;
        let raw_gyro = rotated_gyro + gyro_bias;
        (raw_accel, raw_gyro)
    }

    #[test]
    fn test_calib_with_noise_rotation_no_scaling() {
        let mut calibrator = Calibrator::new();
        let true_misalignment_quat = UnitQuaternion::from_euler_angles(
            0.1,
           -0.05,
            0.15
        );
        
        let true_accel_bias = Vector3::new(0.05, -0.42, 0.03);
        let true_gyro_bias = Vector3::new(0.005, 0.01, -0.008);
        let true_accel_scale = Vector3::new(1., 1., 1.0);
        let true_gyro_scale = Vector3::new(1., 1.0, 1.0);
        println!("----- GROUND TRUTHS -----");
        println!("Misalignment (Quaternion): {:?}", true_misalignment_quat);
        println!("Misalignment (Euler): {:?}", true_misalignment_quat.euler_angles());
        println!("Accelerometer Bias: {}", true_accel_bias);
        println!("Gyroscope Bias: {}", true_gyro_bias);
        println!("Accelerometer Scale: {}", true_accel_scale);
        println!("Gyroscope Scale: {}", true_gyro_scale);
        let mut rng = rand::thread_rng();
        let ideal_accel = Vector3::new(0.0, -G_ACCEL, 0.0);
        let ideal_gyro = Vector3::new(0.0, 0.0, 0.0);
        let num_samples = 2000;
        let accel_noise_stddev = 0.1;
        let gyro_noise_stddev = 0.01;
        for _ in 0..num_samples {
            let (raw_accel, raw_gyro) = generate_test_sample(
                &ideal_accel,
                &ideal_gyro,
                &true_accel_bias,
                &true_gyro_bias,
                &true_accel_scale,
                &true_gyro_scale,
                &true_misalignment_quat,
                accel_noise_stddev,
                gyro_noise_stddev,
                &mut rng,
            );
            calibrator.add_static_sample(raw_accel, raw_gyro);
        }
        println!("Finished adding {} static samples.", num_samples);
        let with_scaling = true;
        let result = calibrator.calibrate(false).expect("Calibration failed");
        println!("----- ESTIMATED BIASES -----");
        println!("Misalignment (Quaternion): {:?}", result.misalignment);
        println!("Misalignment (Euler): {:?}", result.misalignment.euler_angles());
        println!("Accelerometer Bias: {}", result.accelerometer_bias);
        println!("Gyroscope Bias: {}", result.gyroscope_bias);
        if with_scaling {
            println!("Accelerometer Scale: {}", result.accelerometer_scale);
            println!("Gyroscope Scale: {}", result.gyroscope_scale);
        }
        let (test_raw_accel, test_raw_gyro) = generate_test_sample(
            &ideal_accel,
            &ideal_gyro,
            &true_accel_bias,
            &true_gyro_bias,
            &true_accel_scale,
            &true_gyro_scale,
            &true_misalignment_quat,
            0.0,
            0.0,
            &mut rng,
        );
        let corrected_accel = result.correct_accelerometer(&test_raw_accel);
        let corrected_gyro = result.correct_gyroscope(&test_raw_gyro);
        println!("----- VERIFICATION -----");
        println!("Ideal Accel: {:.6}", ideal_accel);
        println!("Corrected Accel: {:.6}", corrected_accel);
        println!("Ideal Gyro: {:.6}", ideal_gyro);
        println!("Corrected Gyro: {:.6}", corrected_gyro);
        println!("Gyro Error Norm: {:.6}", (corrected_gyro - ideal_gyro).norm());
        let tolerance = 0.1;
        assert!((result.accelerometer_bias - true_accel_bias).norm() < tolerance, "Accel bias mismatch");
        assert!((result.gyroscope_bias - true_gyro_bias).norm() < tolerance, "Gyro bias mismatch");
        if with_scaling {
             assert!((result.accelerometer_scale - true_accel_scale).norm() < tolerance, "Accel scale mismatch");
             assert!((result.gyroscope_scale - true_gyro_scale).norm() < tolerance, "Gyro scale mismatch");
        }
        let angle_diff = result.misalignment.angle_to(&true_misalignment_quat);
        println!("Misalignment Angle Difference (radians): {:.6}", angle_diff);
        assert!(angle_diff < 0.1, "Misalignment mismatch");
        assert!((corrected_accel - ideal_accel).norm() < 0.1, "Corrected accel verification failed");
        assert!((corrected_gyro - ideal_gyro).norm() < 0.05, "Corrected gyro verification failed");
    }
}