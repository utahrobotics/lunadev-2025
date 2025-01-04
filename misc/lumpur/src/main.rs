use lumpur::{define_configuration, LumpurBuilder};
use tracing::{error, warn};

define_configuration! {
    #[derive(Debug)]
    pub enum TestCommand {
        Test {
            #[env(TestParam1)]
            param1: String,
            param2: i32
        },
        Test2 {
            param1: String,
            param2: i32
        }
    }
}

fn main() {
    let config: TestCommand = LumpurBuilder::new().copy_file("misc/lumpur/src/lib.rs").init();
    std::thread::spawn(|| loop {
        error!("{:?}", (23, 33));
        std::thread::sleep(std::time::Duration::from_secs(3));
    });
    tracing::debug!("Hello");
    loop {
        warn!("{config:?}");
        println!("{:?}", config);
        // warn!("HGELLO");
        std::thread::sleep(std::time::Duration::from_secs(1));
        panic!("Panic");
    }
}
