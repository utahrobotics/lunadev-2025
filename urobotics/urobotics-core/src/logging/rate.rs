//! Simple primitives for tracking the rate at which events occur.

use std::{borrow::Cow, time::Instant};

use log::info;

/// Tracks the number of times its `increment` method is called within a specific duration and logs it.
pub struct RateLogger {
    start: Instant,
    epoch: usize,
    count: usize,
    /// The duration of time in seconds to track `increment` for.
    ///
    /// Logs are produced every `window_size` seconds. You are advised
    /// to not change this after `increment` has been called.
    pub window_size: f32,
    /// The target to log to.
    ///
    /// This is essentially the name that will be added to the log.
    pub target: Cow<'static, str>,
}

impl Default for RateLogger {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            epoch: 0,
            count: 0,
            window_size: 1.0,
            target: "rate-logger".into(),
        }
    }
}

impl RateLogger {
    /// Increments the number of calls per duration of time.
    pub fn increment(&mut self) {
        let current_epoch = (self.start.elapsed().as_secs_f32() / self.window_size) as usize;
        if self.epoch != current_epoch {
            info!(target: &self.target, "{:.4} Hz", self.count as f32 / self.window_size);
            self.count = 0;
            self.epoch = current_epoch;
        }
        self.count += 1;
    }
}
