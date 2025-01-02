use tracing::{error, warn};

fn main() -> anyhow::Result<()> {
    lumpur::init()?;
    std::thread::spawn(|| loop {
        error!("{:?}", (23, 33));
        std::thread::sleep(std::time::Duration::from_secs(3));
    });
    loop {
        println!("A");
        // warn!("HGELLO");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
