//! Live statistics updates
//!
//! This module provides real-time statistics display during test execution.
//! Live statistics are updated at configurable intervals and can be displayed
//! in various formats (console, CSV, JSON).
//!
//! # Features
//!
//! - **Periodic updates**: Configurable interval (default 1 second)
//! - **Console display**: Human-readable single-line or multi-line format
//! - **CSV output**: Time-series data for analysis
//! - **JSON output**: Structured data for programmatic consumption
//! - **Instantaneous metrics**: IOPS and throughput since last update
//! - **Per-worker stats**: Optional per-worker breakdown
//!
//! # Example
//!
//! ```no_run
//! use iopulse::stats::live::LiveStats;
//! use iopulse::stats::WorkerStats;
//! use std::time::Duration;
//!
//! let mut live = LiveStats::new(Duration::from_secs(1));
//!
//! // Periodically update with current statistics
//! let stats = WorkerStats::new();
//! if live.should_update() {
//!     live.update(&stats);
//!     live.display_console();
//! }
//! ```

use crate::stats::WorkerStats;
use crate::util::time::{calculate_iops, calculate_throughput, format_rate, format_throughput};
use std::time::{Duration, Instant};

/// Live statistics tracker
///
/// Tracks statistics over time and provides periodic updates. Calculates
/// instantaneous metrics (IOPS, throughput) since the last update.
#[derive(Debug)]
pub struct LiveStats {
    /// Update interval
    interval: Duration,
    
    /// Last update time
    last_update: Instant,
    
    /// Statistics at last update
    last_stats: LiveSnapshot,
    
    /// Current statistics
    current_stats: LiveSnapshot,
    
    /// Update counter
    update_count: u64,
    
    /// Test start time (for elapsed time display)
    test_start: Instant,
}

/// Snapshot of statistics at a point in time
#[derive(Debug, Clone)]
struct LiveSnapshot {
    timestamp: Instant,
    read_ops: u64,
    write_ops: u64,
    read_bytes: u64,
    write_bytes: u64,
    errors: u64,
    avg_latency_us: f64,
}

impl LiveSnapshot {
    fn from_stats(stats: &WorkerStats) -> Self {
        let avg_latency_us = stats.io_latency().mean().as_micros() as f64;
        
        Self {
            timestamp: Instant::now(),
            read_ops: stats.read_ops(),
            write_ops: stats.write_ops(),
            read_bytes: stats.read_bytes(),
            write_bytes: stats.write_bytes(),
            errors: stats.errors(),
            avg_latency_us,
        }
    }
    
    fn zero() -> Self {
        Self {
            timestamp: Instant::now(),
            read_ops: 0,
            write_ops: 0,
            read_bytes: 0,
            write_bytes: 0,
            errors: 0,
            avg_latency_us: 0.0,
        }
    }
}

impl LiveStats {
    /// Create a new live statistics tracker
    ///
    /// # Arguments
    ///
    /// * `interval` - Update interval
    pub fn new(interval: Duration) -> Self {
        let now = Instant::now();
        Self {
            interval,
            last_update: now,
            last_stats: LiveSnapshot::zero(),
            current_stats: LiveSnapshot::zero(),
            update_count: 0,
            test_start: now,
        }
    }
    
    /// Check if it's time to update
    ///
    /// Returns true if the interval has elapsed since the last update.
    pub fn should_update(&self) -> bool {
        self.last_update.elapsed() >= self.interval
    }
    
    /// Update with current statistics
    ///
    /// Records the current statistics and prepares for display.
    ///
    /// # Arguments
    ///
    /// * `stats` - Current worker statistics
    pub fn update(&mut self, stats: &WorkerStats) {
        self.last_stats = self.current_stats.clone();
        self.current_stats = LiveSnapshot::from_stats(stats);
        self.last_update = Instant::now();
        self.update_count += 1;
    }
    
    /// Update with raw snapshot data
    ///
    /// Records statistics from raw counters (for aggregated snapshots).
    ///
    /// # Arguments
    ///
    /// * `read_ops` - Total read operations
    /// * `write_ops` - Total write operations
    /// * `read_bytes` - Total bytes read
    /// * `write_bytes` - Total bytes written
    /// * `errors` - Total errors
    /// * `avg_latency_us` - Average latency in microseconds
    pub fn update_from_snapshot(&mut self, read_ops: u64, write_ops: u64, read_bytes: u64, write_bytes: u64, errors: u64, avg_latency_us: f64) {
        self.last_stats = self.current_stats.clone();
        self.current_stats = LiveSnapshot {
            timestamp: Instant::now(),
            read_ops,
            write_ops,
            read_bytes,
            write_bytes,
            errors,
            avg_latency_us,
        };
        self.last_update = Instant::now();
        self.update_count += 1;
    }
    
    /// Display statistics to console (single-line format)
    ///
    /// Prints a single line with current IOPS, throughput, average latency, and errors.
    /// Suitable for terminal output with live updates.
    pub fn display_console(&self) {
        let elapsed = self.current_stats.timestamp
            .duration_since(self.last_stats.timestamp);
        
        if elapsed.as_secs() == 0 {
            return; // Avoid division by zero
        }
        
        // Calculate instantaneous metrics
        let read_ops_delta = self.current_stats.read_ops - self.last_stats.read_ops;
        let write_ops_delta = self.current_stats.write_ops - self.last_stats.write_ops;
        let read_bytes_delta = self.current_stats.read_bytes - self.last_stats.read_bytes;
        let write_bytes_delta = self.current_stats.write_bytes - self.last_stats.write_bytes;
        
        let read_iops = calculate_iops(read_ops_delta, elapsed);
        let write_iops = calculate_iops(write_ops_delta, elapsed);
        let read_throughput = calculate_throughput(read_bytes_delta, elapsed);
        let write_throughput = calculate_throughput(write_bytes_delta, elapsed);
        
        // Calculate actual elapsed time from test start
        let total_elapsed = self.test_start.elapsed().as_secs();
        
        // Print single-line update with actual elapsed time
        print!("\r[{:3}s] ", total_elapsed);
        print!("R: {} ({}) ", format_rate(read_iops), format_throughput(read_throughput));
        print!("W: {} ({}) ", format_rate(write_iops), format_throughput(write_throughput));
        
        // Show average latency
        if self.current_stats.avg_latency_us > 0.0 {
            print!("Lat: {:.0}µs ", self.current_stats.avg_latency_us);
        }
        
        if self.current_stats.errors > 0 {
            print!("Errors: {} ", self.current_stats.errors);
        }
        
        // Flush to ensure immediate display
        use std::io::{self, Write};
        io::stdout().flush().ok();
    }
    
    /// Display statistics to console (newline format)
    ///
    /// Prints statistics on a new line. Suitable for logging or when
    /// terminal doesn't support carriage return.
    pub fn display_console_newline(&self) {
        let elapsed = self.current_stats.timestamp
            .duration_since(self.last_stats.timestamp);
        
        if elapsed.as_secs() == 0 {
            return;
        }
        
        let read_ops_delta = self.current_stats.read_ops - self.last_stats.read_ops;
        let write_ops_delta = self.current_stats.write_ops - self.last_stats.write_ops;
        let read_bytes_delta = self.current_stats.read_bytes - self.last_stats.read_bytes;
        let write_bytes_delta = self.current_stats.write_bytes - self.last_stats.write_bytes;
        
        let read_iops = calculate_iops(read_ops_delta, elapsed);
        let write_iops = calculate_iops(write_ops_delta, elapsed);
        let read_throughput = calculate_throughput(read_bytes_delta, elapsed);
        let write_throughput = calculate_throughput(write_bytes_delta, elapsed);
        
        print!("[{:3}s] ", self.update_count);
        print!("R: {} ({}) ", format_rate(read_iops), format_throughput(read_throughput));
        print!("W: {} ({}) ", format_rate(write_iops), format_throughput(write_throughput));
        
        if self.current_stats.avg_latency_us > 0.0 {
            print!("Lat: {:.0}µs ", self.current_stats.avg_latency_us);
        }
        
        println!("Errors: {}", self.current_stats.errors);
    }
    
    /// Get CSV header
    ///
    /// Returns the CSV header row for live statistics output.
    pub fn csv_header() -> String {
        "timestamp,read_iops,write_iops,read_throughput,write_throughput,total_read_ops,total_write_ops,total_read_bytes,total_write_bytes,errors".to_string()
    }
    
    /// Format current statistics as CSV row
    ///
    /// Returns a CSV row with current statistics.
    pub fn to_csv(&self) -> String {
        let elapsed = self.current_stats.timestamp
            .duration_since(self.last_stats.timestamp);
        
        let read_ops_delta = self.current_stats.read_ops - self.last_stats.read_ops;
        let write_ops_delta = self.current_stats.write_ops - self.last_stats.write_ops;
        let read_bytes_delta = self.current_stats.read_bytes - self.last_stats.read_bytes;
        let write_bytes_delta = self.current_stats.write_bytes - self.last_stats.write_bytes;
        
        let read_iops = if elapsed.as_secs() > 0 {
            calculate_iops(read_ops_delta, elapsed)
        } else {
            0.0
        };
        let write_iops = if elapsed.as_secs() > 0 {
            calculate_iops(write_ops_delta, elapsed)
        } else {
            0.0
        };
        let read_throughput = if elapsed.as_secs() > 0 {
            calculate_throughput(read_bytes_delta, elapsed)
        } else {
            0.0
        };
        let write_throughput = if elapsed.as_secs() > 0 {
            calculate_throughput(write_bytes_delta, elapsed)
        } else {
            0.0
        };
        
        format!(
            "{},{:.2},{:.2},{:.2},{:.2},{},{},{},{},{}",
            self.update_count,
            read_iops,
            write_iops,
            read_throughput,
            write_throughput,
            self.current_stats.read_ops,
            self.current_stats.write_ops,
            self.current_stats.read_bytes,
            self.current_stats.write_bytes,
            self.current_stats.errors
        )
    }
    
    /// Get update count
    pub fn update_count(&self) -> u64 {
        self.update_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::OperationType;
    
    #[test]
    fn test_live_stats_new() {
        let live = LiveStats::new(Duration::from_secs(1));
        assert_eq!(live.update_count(), 0);
    }
    
    #[test]
    fn test_should_update() {
        let live = LiveStats::new(Duration::from_millis(100));
        
        // Immediately after creation, should not update
        assert!(!live.should_update());
        
        // After interval, should update
        std::thread::sleep(Duration::from_millis(150));
        assert!(live.should_update());
    }
    
    #[test]
    fn test_update() {
        let mut live = LiveStats::new(Duration::from_secs(1));
        
        let mut stats = WorkerStats::new();
        stats.record_io(OperationType::Read, 4096, Duration::from_micros(100));
        
        live.update(&stats);
        assert_eq!(live.update_count(), 1);
        
        live.update(&stats);
        assert_eq!(live.update_count(), 2);
    }
    
    #[test]
    fn test_csv_header() {
        let header = LiveStats::csv_header();
        assert!(header.contains("timestamp"));
        assert!(header.contains("read_iops"));
        assert!(header.contains("write_iops"));
        assert!(header.contains("errors"));
    }
    
    #[test]
    fn test_to_csv() {
        let mut live = LiveStats::new(Duration::from_secs(1));
        
        let mut stats = WorkerStats::new();
        stats.record_io(OperationType::Read, 4096, Duration::from_micros(100));
        
        live.update(&stats);
        
        let csv = live.to_csv();
        assert!(csv.contains("1,")); // Update count
        assert!(csv.contains(",0")); // Errors
    }
    
    #[test]
    fn test_display_console() {
        let mut live = LiveStats::new(Duration::from_secs(1));
        
        let mut stats = WorkerStats::new();
        stats.record_io(OperationType::Read, 4096, Duration::from_micros(100));
        
        live.update(&stats);
        
        // Should not panic
        live.display_console();
    }
    
    #[test]
    fn test_display_console_newline() {
        let mut live = LiveStats::new(Duration::from_secs(1));
        
        let mut stats = WorkerStats::new();
        stats.record_io(OperationType::Write, 8192, Duration::from_micros(150));
        
        live.update(&stats);
        
        // Should not panic
        live.display_console_newline();
    }
}

