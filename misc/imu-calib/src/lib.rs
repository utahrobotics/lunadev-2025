use nalgebra::{Matrix3, Normed, Quaternion, UnitQuaternion, Vector3};

use optimization::{Function, Function1, Minimizer, Evaluation, GradientDescent, ArmijoLineSearch, NumericalDifferentiation, ExactLineSearch};
use std::vec::Vec;

const G_ACCEL: f64 = 9.81;
const NUM_PARAMS: usize = 4;

/// Represents biases and scale factors found by the minimizer.
#[derive(Clone, Debug)]
pub struct CalibrationParameters {
    pub rotation: Quaternion<f64>
}

impl CalibrationParameters {
    /// Constructs a new set of calibration parameters from known biases.
    pub fn new(rotation: Quaternion<f64>) -> Self {
        Self {
            rotation,
        }
    }

    /// Creates parameters with zero bias and unit scale.
    pub fn identity() -> Self {
        CalibrationParameters {
            rotation: Quaternion::identity(),
        }
    }

    /// Flattens parameters into a vector for the optimizer.
    pub fn to_vec(&self) -> Vec<f64> {
        let mut params = Vec::with_capacity(NUM_PARAMS);
        params.extend_from_slice(self.rotation.coords.as_slice());
        params
    }

    /// Creates parameters from a flat vector provided by the optimizer.
    pub fn from_slice(params: &[f64]) -> Result<Self, &'static str> {
        if params.len() != NUM_PARAMS {
            return Err("Incorrect number of parameters in slice");
        }
        Ok(CalibrationParameters {
            rotation: Quaternion::new(params[0],params[1],params[2],params[3]),
        })
    }

    /// Applies correction to an accelerometer reading.
    pub fn correct_accelerometer(&self, raw: &Vector3<f64>) -> Vector3<f64> {
        let bias_corrected = UnitQuaternion::from_quaternion(self.rotation).transform_vector(raw);
        bias_corrected
    }

    /// Applies correction to an gyroscope reading.
    pub fn correct_gyroscope(&self, raw: &Vector3<f64>) -> Vector3<f64> {
        *raw
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

    pub fn avg_accel_vector(&self) -> Vector3<f64> {
        let mut sum: Vector3<f64> = Vector3::zeros();
        for reading in self.static_readings.iter() {
            sum.x += reading.0.x;
            sum.y += reading.0.y;
            sum.z += reading.0.z;
        }
        println!("sum: {sum:?}, avg: {}", sum / (self.sample_count() as f64));
        return sum / (self.sample_count() as f64);
    }

    /// Adds a sample pair collected when the IMU was static.
    pub fn add_static_sample(&mut self, accel_raw: Vector3<f64>, gyro_raw: Vector3<f64>) {
        self.static_readings.push((accel_raw, gyro_raw));
    }

    pub fn reset(&mut self) {
        self.static_readings.clear()
    }


    /// moving avg filter
    /// `window_size` must be an odd number to have a symmetric window.
    pub fn smooth(&mut self, window_size: usize) -> Result<(), String> {
        let n = self.static_readings.len();
        if n == 0 {
            return Ok(());
        }
        if window_size == 0 || window_size % 2 == 0 {
            return Err("window_size must be a non-zero odd number".to_string());
        }
        
        let half_window = window_size / 2;
        let mut smoothed = Vec::with_capacity(n);
        
        for i in 0..n {
            let start = if i >= half_window { i - half_window } else { 0 };
            let end = std::cmp::min(n, i + half_window + 1); // non-inclusive upper bound
            
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

    /// Returns the number of static samples.
    pub fn sample_count(&self) -> usize {
        self.static_readings.len()
    }     

    /// Performs the calibration using the collected static samples
    pub fn calibrate(&mut self) -> Result<CalibrationParameters, String> {
        if self.static_readings.len() < 6 {
            return Err("Insufficient static samples for calibration (need at least 6)".to_string());
        }

        self.smooth(101)?;

        let cost_function = ImuCostFunction {
            readings: &self.static_readings
        };

        let cost_function_nd = NumericalDifferentiation::new(cost_function);

        let line_search = ArmijoLineSearch::new(0.5, 1., 0.5);
        let minimizer = GradientDescent::new()
            .line_search(line_search)
            .gradient_tolerance(1e-7)
            .max_iterations(Some(100_000));

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
    fn value(&self, params_vec: &[f64]) -> f64 {
        let ideal_accel = Vector3::new(0.,-G_ACCEL,0.);
        let ideal_gyro = Vector3::new(0.,0.,0.);
        let params = match CalibrationParameters::from_slice(params_vec) {
            Ok(p) => p,
            Err(_) => return f64::INFINITY,
        };

        let w_a = 1.;
        let w_g = 1.;

        let mut total_error = 0.0;

        for (accel_raw, gyro_raw) in self.readings {
            let accel_corrected = params.correct_accelerometer(accel_raw);
            let gyro_corrected = params.correct_gyroscope(gyro_raw);

            let diff = (accel_corrected - ideal_accel)*w_a;
            let accel_error = diff.norm_squared();

            // let diff = (gyro_corrected - ideal_gyro)*w_g;
            // let gyro_error = diff.norm_squared();

            total_error += accel_error;
        }

        if self.readings.is_empty() {
            0.0
        } else {
            total_error / (self.readings.len() as f64)
        }
    }

}



#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::*;
    use rand::Rng;

    // #[test]
    // fn test_calib_with_noise_and_scaling() {
    //     let mut calibrator = Calibrator::new();
    //     let accel_bias = Vector3::new(0.01, 0.1, 0.2);
    //     let gyro_bias = Vector3::new(0.,0.2,0.01);

    //     let accel_scale_bias = Vector3::new(1.102, 1.03, 0.991);
    //     let gyro_scale_bias = Vector3::new(1.405, 1.1, 0.94);


    //     let mut rng = rand::rng();
    //     let actual_accel = Vector3::new(0.,-G_ACCEL, 0.);
    //     let actual_gyro = Vector3::new(0.,0., 0.);
    //     let scaled_accel = actual_accel.component_mul(&accel_scale_bias);
    //     let scaled_gyro = actual_gyro.component_mul(&gyro_scale_bias);
    //     for _ in 0..2316 {
    //         let xrand_accel =rng.random_range(-0.4..=0.4);
    //         let yrand_accel =rng.random_range(-0.4..=0.4);
    //         let zrand_accel =rng.random_range(-0.4..=0.4);
    //         let accel_noise = Vector3::new(xrand_accel, yrand_accel, zrand_accel);
            
    //         let xrand_gyro = rng.random_range(-0.4..=0.4);
    //         let yrand_gyro =rng.random_range(-0.4..=0.4);
    //         let zrand_gyro = rng.random_range(-0.4..=0.4);
    //         let gyro_noise = Vector3::new(xrand_gyro, yrand_gyro, zrand_gyro);

    //         let noisy_biased_accel = (accel_noise + scaled_accel + accel_bias);

    //         let noisy_biased_gyro = (gyro_noise + scaled_gyro + gyro_bias);

    //         calibrator.add_static_sample(noisy_biased_accel, noisy_biased_gyro);
    //     }
    //     println!("finished adding static samples");

    //     let result = calibrator.calibrate().unwrap();
    //     let corrected_accel = result.correct_accelerometer(&((scaled_accel)+accel_bias));
    //     let corrected_gyro = result.correct_gyroscope(&((scaled_gyro)+gyro_bias));
    //     println!("actual accel: {actual_accel}, corrected accel: {corrected_accel}");
    //     println!("actual gyro: {actual_gyro}, corrected gyro: {corrected_gyro}");
    // }

    #[test]
    fn test_calib_with_noise_no_scaling() {
        let mut calibrator = Calibrator::new();
        let accel_bias = Vector3::new(0.01, 0.1, 0.2);
        let gyro_bias = Vector3::new(0.,0.2,0.01);


        let mut rng = rand::rng();
        let actual_accel = Vector3::new(0.,-9.81, 0.);
        let actual_gyro = Vector3::new(0.,0., 0.);

        for _ in 0..30000 {
            let xrand_accel =rng.random_range(-0.4..=0.4);
            let yrand_accel =rng.random_range(-0.4..=0.4);
            let zrand_accel =rng.random_range(-0.4..=0.4);
            let accel_noise = Vector3::new(xrand_accel, yrand_accel, zrand_accel);
            
            let xrand_gyro = rng.random_range(-0.4..=0.4);
            let yrand_gyro =rng.random_range(-0.4..=0.4);
            let zrand_gyro = rng.random_range(-0.4..=0.4);
            let gyro_noise = Vector3::new(xrand_gyro, yrand_gyro, zrand_gyro);

            let noisy_biased_accel = (actual_accel + accel_noise + accel_bias);

            let noisy_biased_gyro = (actual_gyro + gyro_noise + gyro_bias);

            calibrator.add_static_sample(noisy_biased_accel, noisy_biased_gyro);
        }
        println!("finished adding static samples");
        let avg = calibrator.avg_accel_vector();
        println!("Average accel: {}", avg);
        let rotation = Rotation3::rotation_between(&avg, &actual_accel).unwrap();
        // let result = calibrator.calibrate().unwrap();
        let corrected_accel = rotation.transform_vector(&avg);
        println!("actual accel: {actual_accel}, corrected accel: {corrected_accel}");
        // println!("actual gyro: {actual_gyro}, corrected gyro: {corrected_gyro}");
    }
}