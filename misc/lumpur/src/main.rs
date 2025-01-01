fn main() -> anyhow::Result<()> {
    lumpur::init()?;
    loop {
        eprintln!("HGELLO");
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    Ok(())
}