use urobotics::{app::application, camera, python, serial};

fn main() {
    application!()
        .add_app::<serial::SerialConnection>()
        .add_app::<python::PythonVenvBuilder>()
        .add_app::<camera::CameraConnection>()
        .run();
}
