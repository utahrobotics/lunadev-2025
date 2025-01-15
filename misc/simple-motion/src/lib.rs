use crossbeam::atomic::AtomicCell;
use nalgebra::{Isometry3, Point3, Vector3};
use tracing::error;

#[derive(Clone, Copy)]
struct LinearDynamicState {
    current_origin: Point3<f64>,
    current_length: f64,
}


enum TranslationRestrictionState {
    Fixed {
        origin: Point3<f64>
    },
    Linear {
        start_origin: Point3<f64>,
        axis: Vector3<f64>,
        min_length: Option<f64>,
        max_length: Option<f64>,
        dynamic: AtomicCell<LinearDynamicState>
    },
    Free {
        origin: AtomicCell<Point3<f64>>
    }
}


pub struct Transformable {
    translation_restriction: TranslationRestrictionState
}

impl Transformable {
    pub fn try_set_origin(&self, new_origin: Point3<f64>) -> bool {
        match &self.translation_restriction {
            TranslationRestrictionState::Free { origin } => {
                origin.store(new_origin);
                true
            }
            _ => false
        }
    }

    pub fn set_origin(&self, new_origin: Point3<f64>) {
        if !self.try_set_origin(new_origin) {
            error!("Cannot set origin for a non-free translation restriction");
        }
    }

    pub fn try_set_length(&self, mut new_length: f64) -> bool {
        match &self.translation_restriction {
            TranslationRestrictionState::Linear { dynamic, start_origin, axis, min_length, max_length } => {
                if let Some(min_length) = min_length {
                    new_length = new_length.max(*min_length);
                }
                if let Some(max_length) = max_length {
                    new_length = new_length.min(*max_length);
                }
                let current = dynamic.load();
                let new = LinearDynamicState {
                    current_origin: current.current_origin,
                    current_length: new_length
                };
                dynamic.store(new);
                true
            }
            _ => false
        }
    }
}