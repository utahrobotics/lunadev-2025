use std::process::Command;
use crate::apps::production::rerun_viz::get_recorder;
use rerun_ipc_common::TextLog;

pub fn start_heat_logger() -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut iteration = 0;
        loop {
            iteration += 1;
            println!("🔧 DEBUG: Heat logger iteration #{}", iteration);
            
            let sensor_output = Command::new("sensors").output();
            let out = match sensor_output {
                Ok(out) => {
                    format!("stdout: {}, stderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
                }
                Err(err) => {
                    format!("heat sensor err: {}", err)
                }
            };
            
            println!("🔧 DEBUG: Heat logger getting recorder...");
            if let Some(recorder) = get_recorder() {
                println!("🔧 DEBUG: Heat logger got recorder, logging text...");
                let result = recorder.log_static("/robot/cpu_sensors", TextLog::new(out).with_level("INFO".into()));
                match result {
                    Ok(_) => println!("🔧 DEBUG: Heat logger successfully logged text"),
                    Err(e) => println!("🔧 DEBUG: Heat logger failed to log text: {}", e),
                }
            } else {
                println!("🔧 DEBUG: Heat logger could not get recorder");
            }
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    })
}