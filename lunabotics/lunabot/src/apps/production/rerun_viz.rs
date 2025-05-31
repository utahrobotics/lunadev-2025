use std::{process::Command, sync::OnceLock, time::{Duration, Instant}};

use crossbeam::atomic::AtomicCell;
use rerun_ipc_common::{RerunViz, RerunLevel, QueuedRecorder};
use serde::Deserialize;
use tracing::{error, info, debug};

pub const ROBOT: &str = "/robot";
pub const ROBOT_STRUCTURE: &str = "/robot/structure";

pub static RECORDER: OnceLock<QueuedRecorder> = OnceLock::new();

// Global static to hold the rerun-ipc process ID
static RERUN_IPC_PID: OnceLock<Option<u32>> = OnceLock::new();

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
    debug!("🔧 DEBUG: init_rerun called with rerun_viz: {:?}", rerun_viz);
    
    // Start the rerun-ipc process if visualization is enabled
    let process_pid = match &rerun_viz {
        RerunViz::Enabled(level) => {
            debug!("🔧 DEBUG: Rerun enabled with level: {:?}", level);
            // Try to run rerun-ipc using cargo from workspace root
            let spawn_result = Command::new("cargo")
                .args(&["run", "--release", "-p", "rerun-ipc"])
                .current_dir(".")
                .spawn()
                .or_else(|_| {
                    // Fallback: try running from target directory if cargo build was done
                    info!("Cargo run failed, trying to run prebuilt binary...");
                    Command::new("./target/release/rerun-ipc")
                        .spawn()
                })
                .or_else(|_| {
                    // Another fallback: try with full cargo path
                    info!("Direct binary failed, trying with full cargo path...");
                    Command::new("/usr/bin/cargo")
                        .args(&["run", "--release", "-p", "rerun-ipc"])
                        .current_dir(".")
                        .spawn()
                });

            match spawn_result {
                Ok(child) => {
                    let pid = child.id();
                    info!("Started rerun-ipc process with PID: {}", pid);
                    debug!("🔧 DEBUG: Successfully started rerun-ipc process");
                    Some(pid)
                }
                Err(e) => {
                    error!("Failed to start rerun-ipc process: {e}");
                    error!("Make sure cargo is in PATH and you're running from the workspace root");
                    debug!("🔧 DEBUG: Failed to start rerun-ipc process: {}", e);
                    None
                }
            }
        }
        RerunViz::Disabled => {
            debug!("🔧 DEBUG: Rerun disabled");
            None
        }
    };

    // Store the process ID
    if let Err(_) = RERUN_IPC_PID.set(process_pid) {
        error!("Failed to set global rerun-ipc process ID - already initialized");
    }
    
    // Initialize the recorder as before
    debug!("🔧 DEBUG: About to call rerun_ipc_common::init_rerun");
    match rerun_ipc_common::init_rerun(rerun_viz) {
        Ok(recorder) => {
            debug!("🔧 DEBUG: Successfully created recorder from rerun_ipc_common::init_rerun");
            debug!("🔧 DEBUG: Recorder enabled: {}", recorder.is_enabled());
            debug!("🔧 DEBUG: Recorder log level: {:?}", recorder.get_log_level());
            
            if let Err(_) = RECORDER.set(recorder) {
                error!("Failed to set global recorder - already initialized");
                debug!("🔧 DEBUG: Failed to set global recorder - already initialized");
            } else {
                debug!("🔧 DEBUG: Successfully set global recorder");
            }
        },
        Err(e) => {
            error!("Failed to initialize rerun IPC recorder: {e}");
            debug!("🔧 DEBUG: Failed to initialize rerun IPC recorder: {}", e);
        }
    }
    
    debug!("🔧 DEBUG: init_rerun completed");
}

/// Get the global recorder for logging, returns None if not initialized or disabled
pub fn get_recorder() -> Option<&'static QueuedRecorder> {
    let recorder = RECORDER.get();
    debug!("🔧 DEBUG: get_recorder called, returning: {}", 
           if recorder.is_some() { "Some(recorder)" } else { "None" });
    
    if let Some(rec) = recorder {
        debug!("🔧 DEBUG: Recorder is enabled: {}", rec.is_enabled());
        debug!("🔧 DEBUG: Recorder log level: {:?}", rec.get_log_level());
    }
    
    recorder
}

/// Get the log level if recorder is available
pub fn get_log_level() -> Option<RerunLevel> {
    let level = RECORDER.get().and_then(|r| r.get_log_level());
    debug!("🔧 DEBUG: get_log_level called, returning: {:?}", level);
    level
}

/// Get the obstacle map throttle if recorder is available
pub fn get_obstacle_map_throttle() -> Option<&'static AtomicCell<Instant>> {
    let throttle = RECORDER.get().map(|r| r.get_obstacle_map_throttle());
    debug!("🔧 DEBUG: get_obstacle_map_throttle called, returning: {}", 
           if throttle.is_some() { "Some(throttle)" } else { "None" });
    throttle
}

/// Cleanup function to terminate the rerun-ipc process
pub fn cleanup_rerun() {
    debug!("🔧 DEBUG: cleanup_rerun called");
    if let Some(Some(pid)) = RERUN_IPC_PID.get() {
        info!("Terminating rerun-ipc process with PID: {}...", pid);
        debug!("🔧 DEBUG: About to terminate process with PID: {}", pid);
        match Command::new("kill")
            .args(&["-TERM", &pid.to_string()])
            .output()
        {
            Ok(_) => {
                info!("Successfully sent termination signal to rerun-ipc process");
                debug!("🔧 DEBUG: Successfully sent termination signal");
            }
            Err(e) => {
                error!("Failed to terminate rerun-ipc process: {e}");
                debug!("🔧 DEBUG: Failed to terminate rerun-ipc process: {}", e);
            }
        }
    } else {
        debug!("🔧 DEBUG: No rerun-ipc process to terminate");
    }
}