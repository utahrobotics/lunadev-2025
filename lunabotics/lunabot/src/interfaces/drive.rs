use byteable::IntoBytesSlice;
use common::lunasim::FromLunasimbot;
use lunabot_ai::{DriveComponent, FailedToDrive};

use crate::sim::{FromLunasimRef, LunasimStdin};

pub struct SimMotors {
    lunasim_stdin: LunasimStdin,
}

impl SimMotors {
    pub fn new(lunasim_stdin: LunasimStdin, _from_lunasim_ref: FromLunasimRef) -> Self {
        Self { lunasim_stdin }
    }
}

impl DriveComponent for SimMotors {
    fn traverse_path(
        &mut self,
        _path: &[nalgebra::Vector2<f64>],
    ) -> impl std::future::Future<Output = Result<(), FailedToDrive>> {
        async { todo!() }
    }

    fn manual_drive(&mut self, steering: common::Steering) {
        let (left, right) = steering.get_left_and_right();
        FromLunasimbot::Drive {
            left: left as f32,
            right: right as f32,
        }
        .into_bytes_slice(|bytes| {
            self.lunasim_stdin.write(bytes);
        });
    }

    fn had_drive_error(&mut self) -> bool {
        false
    }
}
