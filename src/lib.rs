//! IOPulse - High-performance IO profiling tool
//!
//! IOPulse is a high-performance IO load generation and profiling tool designed for
//! storage benchmarking with realistic workload patterns and precise measurements.
//!
//! # Architecture
//!
//! - **Modular IO engines**: io_uring, libaio, sync, mmap
//! - **Flexible targets**: Files, block devices, network filesystems
//! - **Advanced distributions**: Zipf, Pareto, Gaussian for realistic workloads
//! - **Distributed mode**: Coordinate multiple hosts for aggregate load
//! - **Comprehensive stats**: Latency histograms, metadata ops, per-worker metrics

pub mod config;
pub mod coordinator;
pub mod distributed;
pub mod distribution;
pub mod engine;
pub mod network;
pub mod output;
pub mod stats;
pub mod target;
pub mod util;
pub mod worker;

// Re-export commonly used types
pub use config::Config;
pub use engine::IOEngine;
// pub use worker::Worker; // TODO: Uncomment when Worker is implemented

/// Result type used throughout IOPulse
pub type Result<T> = anyhow::Result<T>;
