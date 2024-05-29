use urobotics::{app::command, python, runtime::RuntimeBuilder, serial};

fn main() {
    RuntimeBuilder::default().start(|context| async move {
        command!()
            .add_async_function::<serial::SerialConnection>()
            .add_async_function::<python::PythonVenvBuilder>()
            .get_matches(context.clone())
            .await;
        context.wait_for_exit().await;
    });
}
