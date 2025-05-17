use std::{f32::consts::PI, sync::{OnceLock, RwLock}, time::Instant};

use crossbeam::atomic::AtomicCell;
use nalgebra::{UnitQuaternion, Vector3};
use rerun::{Asset3D, RecordingStream, RecordingStreamResult, SpawnOptions, ViewCoordinates};
use serde::Deserialize;
use tracing::{error, info};

pub const ROBOT: &str = "/robot";
pub const ROBOT_STRUCTURE: &str = "/robot/structure";

pub static RECORDER: OnceLock<RecorderData> = OnceLock::new();

pub struct RecorderData {
    pub recorder: RecordingStream,
    pub level: Level,
    pub last_logged_obstacle_map: AtomicCell<Instant> // used to throttle the logging to conserve bandwidth
}

#[derive(Deserialize, Default, Debug)]
pub enum RerunViz {
    Grpc(Level,String),
    Log(Level),
    Viz(Level),
    #[default]
    Disabled,
}

#[derive(Deserialize, Default, Debug, PartialEq)]
pub enum Level {
    /// Only logs robots isometry, expanded obstacle map, and april tags.
    #[default]
    Minimal,
    /// Logs everything including height maps and depth camera point cloud.
    All,
}

impl Level {
    /// returns true if the log level is All
    pub fn is_all(&self) -> bool {
        *self == Level::All
    }
}

pub fn init_rerun(rerun_viz: RerunViz) {
    let opts = SpawnOptions {
        memory_limit: "25%".to_string(),
        ..Default::default()
    };
    let (recorder, level) = match rerun_viz {
        RerunViz::Viz(level) => (
            match rerun::RecordingStreamBuilder::new("lunabot").spawn_opts(&opts, None) {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to start rerun process: {e}");
                    return;
                }
            },
            level,
        ),
        RerunViz::Grpc(level, url) => (
            match rerun::RecordingStreamBuilder::new("lunabot").connect_grpc_opts(&url, None) {
                Ok(x) => {
                    info!("Streaming to rerun on: {url}");
                    x
                },
                Err(e) => {
                    error!("Failed to make recording stream: {e}");
                    return;
                }
            },
            level,
        ),
        RerunViz::Log(level) => (
            match rerun::RecordingStreamBuilder::new("lunabot").save("recording.rrd") {
                Ok(x) => x,
                Err(e) => {
                    error!("Failed to start rerun file logging: {e}");
                    return;
                }
            },
            level,
        ),
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

    let _ = RECORDER.set(RecorderData { recorder, level, last_logged_obstacle_map: AtomicCell::new(Instant::now())});

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
