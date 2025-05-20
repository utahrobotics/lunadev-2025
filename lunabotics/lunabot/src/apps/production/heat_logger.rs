use std::process::Command;
use crate::apps::RECORDER;
pub fn start_heat_logger() -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            let sensor_output = Command::new("sensors").output();
            let out = match sensor_output {
                Ok(out) => {
                    format!("stdout: {}, stderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
                }
                Err(err) => {
                    format!("heat sensor err: {}", err)
                }
            };
            if let Some(rec) = RECORDER.get() {
                let _ = rec.recorder.log_static("/robot/cpu_sensors", &rerun::TextLog::new(out).with_level("INFO"));
            } else {
            }
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    })
}