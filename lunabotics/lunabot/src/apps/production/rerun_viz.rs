use std::sync::OnceLock;

use rerun::{Boxes3D, RecordingStream, RecordingStreamResult, ViewCoordinates};
use tracing::error;


pub static RECORDER: OnceLock<RecorderData> = OnceLock::new();

pub struct RecorderData {
    pub recorder: RecordingStream,
}

pub fn init_rerun(rerun_spawn_process: bool) {
    let recorder = if rerun_spawn_process {
        match rerun::RecordingStreamBuilder::new("lunabot").spawn() {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to start rerun process: {e}");
                return;
            }
        }

    } else {
        match rerun::RecordingStreamBuilder::new("lunabot").save("recording.rrd") {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to start rerun file logging: {e}");
                return;
            }
        }
    };
    let result: RecordingStreamResult<()> = try {
        recorder.log_static(
            "/",
            &ViewCoordinates::RIGHT_HAND_Y_UP()
        )?;
        recorder.log_static(
            "/robot/structure/mesh",
            &Boxes3D::from_centers_and_sizes(
                [(0.0, 0.0, 0.0)],
                [(1.0, 1.0, 1.0)]
            )
        )?;
        recorder.log_static(
            "/xyz",
            &rerun::Arrows3D::from_vectors(
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            )
            .with_colors([[255, 0, 0], [0, 255, 0], [0, 0, 255]]),
        )?;
        recorder.log_static(
            "/robot/structure/xyz",
            &rerun::Arrows3D::from_vectors(
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            )
            .with_colors([[255, 0, 0], [0, 255, 0], [0, 0, 255]]),
        )?;
    };
    if let Err(e) = result {
        error!("Failed to setup rerun environment: {e}");
    }
    
    let _ = RECORDER.set(RecorderData {
        recorder,
    });
}