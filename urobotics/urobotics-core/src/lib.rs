#![feature(never_type)]
#![feature(buf_read_has_data_left)]
//! Core library for URobotics
//!
//! Contains utilities for logging and concurrency with tokio.

pub mod cabinet;
pub mod log;
pub mod service;

pub use tasker::*;
pub use tokio;
