use std::{f32::consts::PI, fs::File, sync::OnceLock};

use nalgebra::{UnitQuaternion, Vector3};
use rerun::{datatypes::UVec3D, Mesh3D, RecordingStream, RecordingStreamResult, ViewCoordinates};
use stl_io::{read_stl, IndexedTriangle};
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

    std::thread::spawn(|| {
        let recorder = &RECORDER.get().unwrap().recorder;

        let mut file = match File::open("3d-models/lunabot.stl") {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to open 3d-models/lunabot.stl: {e}");
                return;
            }
        };

        let mesh = match read_stl(&mut file) {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to parse 3d-models/lunabot.stl: {e}");
                return;
            }
        };

        if let Err(e) = recorder.log_static(
            "/robot/structure/mesh",
            &Mesh3D::new(
                mesh.vertices.into_iter().map(|v| (v[0], v[1], v[2])),
            )
            .with_triangle_indices(
                mesh.faces
                    .into_iter()
                    .map(|IndexedTriangle { vertices: [i, j, k], .. }| UVec3D::new(i as u32, j as u32, k as u32))
            )
        ) {
            error!("Failed to log robot structure mesh: {e}");
            return;
        }
        let rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), PI / 2.0) * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -PI / 2.0);
        if let Err(e) = recorder.log(
            "/robot/structure/mesh",
            &rerun::Transform3D::from_rotation(
                rerun::Quaternion::from_xyzw(rotation.as_vector().cast::<f32>().data.0[0]),
            )
        ) {
            error!("Failed to log mesh transform: {e}");
        }
    });
}