use std::{f32::consts::PI, sync::OnceLock};

use nalgebra::{UnitQuaternion, Vector3};
use rerun::{Asset3D, RecordingStream, RecordingStreamResult, ViewCoordinates};
use serde::Deserialize;
use tracing::error;

pub const ROBOT: &str = "/robot";
pub const ROBOT_STRUCTURE: &str = "/robot/structure";

pub static RECORDER: OnceLock<RecorderData> = OnceLock::new();

pub struct RecorderData {
    pub recorder: RecordingStream,
}

#[derive(Deserialize, Default)]
pub enum RerunViz {
    Log,
    Viz,
    #[default]
    Disabled,
}

pub fn init_rerun(rerun_viz: RerunViz) {
    let recorder = match rerun_viz {
        RerunViz::Viz => match rerun::RecordingStreamBuilder::new("lunabot").spawn() {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to start rerun process: {e}");
                return;
            }
        },
        RerunViz::Log => {
            match rerun::RecordingStreamBuilder::new("lunabot").save("recording.rrd") {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to start rerun file logging: {e}");
                    return;
                }
            }
        }
        RerunViz::Disabled => {
            return;
        }
    };
    let result: RecordingStreamResult<()> = try {
        recorder.log_static("/", &ViewCoordinates::RIGHT_HAND_Y_UP())?;
        recorder.log_static(
            format!("{ROBOT_STRUCTURE}/xyz"),
            &rerun::Arrows3D::from_vectors([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
                .with_colors([[255, 0, 0], [0, 255, 0], [0, 0, 255]]),
        )?;
    };
    if let Err(e) = result {
        error!("Failed to setup rerun environment: {e}");
    }

    let _ = RECORDER.set(RecorderData { recorder });

    std::thread::spawn(|| {
        let recorder = &RECORDER.get().unwrap().recorder;

        let asset = match Asset3D::from_file("3d-models/lunabot.stl") {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to open 3d-models/lunabot.stl: {e}");
                return;
            }
        };

        if let Err(e) = recorder.log_static(format!("{ROBOT_STRUCTURE}/mesh"), &asset) {
            error!("Failed to log robot structure mesh: {e}");
            return;
        }
        let rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), PI / 2.0)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -PI / 2.0);
        if let Err(e) = recorder.log(
            format!("{ROBOT_STRUCTURE}/mesh"),
            &rerun::Transform3D::from_rotation(rerun::Quaternion::from_xyzw(
                rotation.as_vector().cast::<f32>().data.0[0],
            )),
        ) {
            error!("Failed to log mesh transform: {e}");
        }
    });
}
