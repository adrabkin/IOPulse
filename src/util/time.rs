//! High-precision timing utilities
//!
//! This module provides utilities for high-precision timing measurements,
//! which are essential for accurate latency tracking in IO operations.

use std::time::{Duration, Instant};

/// High-precision timestamp for latency measurements
///
/// This is a thin wrapper around `std::time::Instant` that provides
/// convenience methods for latency tracking.
#[derive(Debug, Clone, Copy)]
pub struct Timestamp {
    instant: Instant,
}

impl Timestamp {
    /// Create a new timestamp representing the current time
    #[inline]
    pub fn now() -> Self {
        Self {
            instant: Instant::now(),
        }
    }

    /// Get the elapsed time since this timestamp
    #[inline]
    pub fn elapsed(&self) -> Duration {
        self.instant.elapsed()
    }

    /// Get the elapsed time in nanoseconds
    #[inline]
    pub fn elapsed_nanos(&self) -> u64 {
        self.elapsed().as_nanos() as u64
    }

    /// Get the elapsed time in microseconds
    #[inline]
    pub fn elapsed_micros(&self) -> u64 {
        self.elapsed().as_micros() as u64
    }

    /// Get the elapsed time in milliseconds
    #[inline]
    pub fn elapsed_millis(&self) -> u64 {
        self.elapsed().as_millis() as u64
    }

    /// Get the duration between this timestamp and another
    #[inline]
    pub fn duration_since(&self, earlier: Timestamp) -> Duration {
        self.instant.duration_since(earlier.instant)
    }
}

impl Default for Timestamp {
    fn default() -> Self {
        Self::now()
    }
}

/// Format a duration in human-readable form
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use iopulse::util::time::format_duration;
///
/// assert_eq!(format_duration(Duration::from_nanos(500)), "500ns");
/// assert_eq!(format_duration(Duration::from_nanos(1500)), "1.50us");
/// assert_eq!(format_duration(Duration::from_micros(2500)), "2.50ms");
/// assert_eq!(format_duration(Duration::from_secs(5)), "5.00s");
/// ```
pub fn format_duration(duration: Duration) -> String {
    let nanos = duration.as_nanos();
    
    if nanos < 1_000 {
        format!("{}ns", nanos)
    } else if nanos < 1_000_000 {
        format!("{:.2}us", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2}ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.2}s", nanos as f64 / 1_000_000_000.0)
    }
}

/// Format a rate (operations per second)
///
/// # Examples
///
/// ```
/// use iopulse::util::time::format_rate;
///
/// assert_eq!(format_rate(500.0), "500");
/// assert_eq!(format_rate(1500.0), "1.50K");
/// assert_eq!(format_rate(2_500_000.0), "2.50M");
/// ```
pub fn format_rate(rate: f64) -> String {
    if rate < 1_000.0 {
        format!("{:.0}", rate)
    } else if rate < 1_000_000.0 {
        format!("{:.2}K", rate / 1_000.0)
    } else if rate < 1_000_000_000.0 {
        format!("{:.2}M", rate / 1_000_000.0)
    } else {
        format!("{:.2}G", rate / 1_000_000_000.0)
    }
}

/// Calculate IOPS from operation count and duration
///
/// # Arguments
///
/// * `operations` - Number of operations completed
/// * `duration` - Time duration over which operations were performed
///
/// # Returns
///
/// Operations per second as a floating point number
pub fn calculate_iops(operations: u64, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds > 0.0 {
        operations as f64 / seconds
    } else {
        0.0
    }
}

/// Calculate throughput from bytes transferred and duration
///
/// # Arguments
///
/// * `bytes` - Number of bytes transferred
/// * `duration` - Time duration over which transfer occurred
///
/// # Returns
///
/// Bytes per second as a floating point number
pub fn calculate_throughput(bytes: u64, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds > 0.0 {
        bytes as f64 / seconds
    } else {
        0.0
    }
}

/// Format throughput in human-readable form (B/s, KB/s, MB/s, GB/s)
///
/// # Examples
///
/// ```
/// use iopulse::util::time::format_throughput;
///
/// assert_eq!(format_throughput(500.0), "500.00 B/s");
/// assert_eq!(format_throughput(1536.0), "1.50 KB/s");
/// assert_eq!(format_throughput(2_621_440.0), "2.50 MB/s");
/// assert_eq!(format_throughput(2_684_354_560.0), "2.50 GB/s");
/// ```
pub fn format_throughput(bytes_per_sec: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    if bytes_per_sec >= TB {
        format!("{:.2} TB/s", bytes_per_sec / TB)
    } else if bytes_per_sec >= GB {
        format!("{:.2} GB/s", bytes_per_sec / GB)
    } else if bytes_per_sec >= MB {
        format!("{:.2} MB/s", bytes_per_sec / MB)
    } else if bytes_per_sec >= KB {
        format!("{:.2} KB/s", bytes_per_sec / KB)
    } else {
        format!("{:.2} B/s", bytes_per_sec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_timestamp_elapsed() {
        let start = Timestamp::now();
        thread::sleep(Duration::from_millis(10));
        let elapsed = start.elapsed();
        
        assert!(elapsed >= Duration::from_millis(10));
        assert!(elapsed < Duration::from_millis(50)); // Allow some slack
    }

    #[test]
    fn test_timestamp_elapsed_nanos() {
        let start = Timestamp::now();
        thread::sleep(Duration::from_millis(1));
        let nanos = start.elapsed_nanos();
        
        assert!(nanos >= 1_000_000); // At least 1ms
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_nanos(500)), "500ns");
        assert_eq!(format_duration(Duration::from_nanos(1500)), "1.50us");
        assert_eq!(format_duration(Duration::from_micros(1500)), "1.50ms");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.50s");
        assert_eq!(format_duration(Duration::from_secs(5)), "5.00s");
    }

    #[test]
    fn test_format_rate() {
        assert_eq!(format_rate(500.0), "500");
        assert_eq!(format_rate(1500.0), "1.50K");
        assert_eq!(format_rate(1_500_000.0), "1.50M");
        assert_eq!(format_rate(1_500_000_000.0), "1.50G");
    }

    #[test]
    fn test_calculate_iops() {
        let duration = Duration::from_secs(10);
        let iops = calculate_iops(1000, duration);
        assert_eq!(iops, 100.0);
    }

    #[test]
    fn test_calculate_iops_zero_duration() {
        let duration = Duration::from_secs(0);
        let iops = calculate_iops(1000, duration);
        assert_eq!(iops, 0.0);
    }

    #[test]
    fn test_calculate_throughput() {
        let duration = Duration::from_secs(10);
        let throughput = calculate_throughput(1024 * 1024 * 10, duration); // 10MB in 10s
        assert_eq!(throughput, 1024.0 * 1024.0); // 1MB/s
    }

    #[test]
    fn test_format_throughput() {
        assert_eq!(format_throughput(500.0), "500.00 B/s");
        assert_eq!(format_throughput(1536.0), "1.50 KB/s");
        assert_eq!(format_throughput(1536.0 * 1024.0), "1.50 MB/s");
        assert_eq!(format_throughput(1536.0 * 1024.0 * 1024.0), "1.50 GB/s");
        assert_eq!(format_throughput(1536.0 * 1024.0 * 1024.0 * 1024.0), "1.50 TB/s");
    }

    #[test]
    fn test_timestamp_duration_since() {
        let t1 = Timestamp::now();
        thread::sleep(Duration::from_millis(10));
        let t2 = Timestamp::now();
        
        let duration = t2.duration_since(t1);
        assert!(duration >= Duration::from_millis(10));
        assert!(duration < Duration::from_millis(50));
    }
}
