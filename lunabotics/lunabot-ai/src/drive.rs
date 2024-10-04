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
    /// the future is dropped, the robot must stop. If `Err` is returned, `had_drive_error`
    /// will return `true`.
    fn traverse_path(
        &mut self,
        path: &[Vector2<f64>],
    ) -> impl Future<Output = Result<(), FailedToDrive>>;

    /// Drives the robot manually.
    ///
    /// If an error was asyncronously encountered, `had_drive_error` will return `true`.
    fn manual_drive(&mut self, steering: Steering);

    /// Returns `true` if an error was encountered while driving.
    ///
    /// Calling this method will reset the error flag.
    fn had_drive_error(&mut self) -> bool;
}
