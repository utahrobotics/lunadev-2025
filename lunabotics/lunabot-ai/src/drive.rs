use std::future::Future;

use common::Steering;
use nalgebra::Vector2;

#[derive(Debug, Clone, Copy)]
pub struct FailedToDrive;

impl std::fmt::Display for FailedToDrive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to drive")
    }
}

pub trait DriveComponent {
    /// Drives across the given path.
    ///
    /// The returned future will resolve when the path has been traversed and when
    /// the future is dropped, the robot must stop.
    fn traverse_path(
        &mut self,
        path: &[Vector2<f64>],
    ) -> impl Future<Output = Result<(), FailedToDrive>>;

    /// Drives the robot manually.
    ///
    /// Awaiting the returned future should *not* be necessary to issue the steering
    /// and should only be used to wait for a result from the underlying drive implementation.
    fn manual_drive(
        &mut self,
        steering: Steering,
    ) -> impl Future<Output = Result<(), FailedToDrive>>;
}
