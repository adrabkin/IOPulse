//! Latency histogram using HdrHistogram
//!
//! This module provides a wrapper around the HdrHistogram library for tracking
//! IO operation latencies with high precision and low overhead.
//!
//! # Features
//!
//! - **High precision**: Sub-microsecond resolution for low latencies
//! - **Wide range**: Tracks latencies from nanoseconds to seconds
//! - **Low overhead**: Constant-time recording and percentile queries
//! - **Accurate percentiles**: Corrected percentile calculations
//!
//! # Example
//!
//! ```
//! use iopulse::stats::histogram::LatencyHistogram;
//! use std::time::Duration;
//!
//! let mut hist = LatencyHistogram::new();
//!
//! // Record some latencies
//! hist.record(Duration::from_micros(100));
//! hist.record(Duration::from_micros(150));
//! hist.record(Duration::from_micros(200));
//!
//! // Get percentiles
//! let p50 = hist.percentile(50.0);
//! let p99 = hist.percentile(99.0);
//!
//! println!("p50: {:?}, p99: {:?}", p50, p99);
//! ```

use crate::Result;
use hdrhistogram::Histogram;
use std::time::Duration;

/// Latency histogram wrapper
///
/// Wraps HdrHistogram with a convenient interface for recording and querying
/// IO operation latencies. The histogram is configured to track latencies from
/// 1 nanosecond to 1 hour with 3 significant digits of precision.
///
/// # Precision
///
/// The histogram uses 3 significant digits, which means:
/// - Values are accurate to within 0.1% of the actual value
/// - Memory usage is approximately 2KB per histogram
/// - Recording and querying are O(1) operations
///
/// # Range
///
/// - **Minimum**: 1 nanosecond
/// - **Maximum**: 3,600,000,000,000 nanoseconds (1 hour)
/// - **Resolution**: Sub-microsecond for low latencies
#[derive(Debug)]
pub struct LatencyHistogram {
    histogram: Histogram<u64>,
}

impl LatencyHistogram {
    /// Create a new latency histogram
    ///
    /// The histogram is configured to track latencies from 1ns to 1 hour with
    /// 3 significant digits of precision.
    pub fn new() -> Self {
        // Create histogram with:
        // - Minimum value: 1 (1 nanosecond)
        // - Maximum value: 3,600,000,000,000 (1 hour in nanoseconds)
        // - Significant digits: 3 (0.1% precision)
        let histogram = Histogram::new_with_bounds(1, 3_600_000_000_000, 3)
            .expect("Failed to create histogram with valid bounds");

        Self { histogram }
    }

    /// Record a latency sample
    ///
    /// Converts the duration to nanoseconds and records it in the histogram.
    /// If the value is outside the histogram's range, it is clamped to the
    /// nearest valid value.
    ///
    /// # Arguments
    ///
    /// * `latency` - The latency duration to record
    ///
    /// # Example
    ///
    /// ```
    /// use iopulse::stats::histogram::LatencyHistogram;
    /// use std::time::Duration;
    ///
    /// let mut hist = LatencyHistogram::new();
    /// hist.record(Duration::from_micros(100));
    /// hist.record(Duration::from_millis(5));
    /// ```
    #[inline]
    pub fn record(&mut self, latency: Duration) {
        let nanos = latency.as_nanos() as u64;
        // Clamp to valid range (1ns to 1 hour)
        let value = nanos.max(1).min(3_600_000_000_000);
        // Saturating record - if value is out of range, it's clamped
        let _ = self.histogram.record(value);
    }

    /// Get the value at a specific percentile
    ///
    /// Returns the latency value at the specified percentile. Uses corrected
    /// percentile calculation for accurate results.
    ///
    /// # Arguments
    ///
    /// * `percentile` - The percentile to query (0.0 - 100.0)
    ///
    /// # Returns
    ///
    /// The latency at the specified percentile, or None if the histogram is empty.
    ///
    /// # Example
    ///
    /// ```
    /// use iopulse::stats::histogram::LatencyHistogram;
    /// use std::time::Duration;
    ///
    /// let mut hist = LatencyHistogram::new();
    /// hist.record(Duration::from_micros(100));
    /// hist.record(Duration::from_micros(200));
    ///
    /// if let Some(p50) = hist.percentile(50.0) {
    ///     println!("Median latency: {:?}", p50);
    /// }
    /// ```
    pub fn percentile(&self, percentile: f64) -> Option<Duration> {
        if self.histogram.len() == 0 {
            return None;
        }

        let value = self.histogram.value_at_percentile(percentile);
        Some(Duration::from_nanos(value))
    }

    /// Get the minimum recorded latency
    ///
    /// # Returns
    ///
    /// The minimum latency, or None if the histogram is empty.
    pub fn min(&self) -> Option<Duration> {
        if self.histogram.len() == 0 {
            return None;
        }
        Some(Duration::from_nanos(self.histogram.min()))
    }

    /// Get the maximum recorded latency
    ///
    /// # Returns
    ///
    /// The maximum latency, or None if the histogram is empty.
    pub fn max(&self) -> Option<Duration> {
        if self.histogram.len() == 0 {
            return None;
        }
        Some(Duration::from_nanos(self.histogram.max()))
    }

    /// Get the mean (average) latency
    ///
    /// # Returns
    ///
    /// The mean latency, or None if the histogram is empty.
    pub fn mean(&self) -> Option<Duration> {
        if self.histogram.len() == 0 {
            return None;
        }
        Some(Duration::from_nanos(self.histogram.mean() as u64))
    }

    /// Get the standard deviation of latencies
    ///
    /// # Returns
    ///
    /// The standard deviation, or None if the histogram is empty.
    pub fn stddev(&self) -> Option<Duration> {
        if self.histogram.len() == 0 {
            return None;
        }
        Some(Duration::from_nanos(self.histogram.stdev() as u64))
    }

    /// Get the number of samples recorded
    ///
    /// # Returns
    ///
    /// The total number of latency samples recorded.
    pub fn len(&self) -> u64 {
        self.histogram.len()
    }

    /// Check if the histogram is empty
    ///
    /// # Returns
    ///
    /// True if no samples have been recorded, false otherwise.
    pub fn is_empty(&self) -> bool {
        self.histogram.len() == 0
    }

    /// Merge another histogram into this one
    ///
    /// Combines the samples from another histogram into this one. This is used
    /// to aggregate statistics from multiple workers.
    ///
    /// # Arguments
    ///
    /// * `other` - The histogram to merge into this one
    ///
    /// # Errors
    ///
    /// Returns an error if the histograms have incompatible configurations.
    ///
    /// # Example
    ///
    /// ```
    /// use iopulse::stats::histogram::LatencyHistogram;
    /// use std::time::Duration;
    ///
    /// let mut hist1 = LatencyHistogram::new();
    /// hist1.record(Duration::from_micros(100));
    ///
    /// let mut hist2 = LatencyHistogram::new();
    /// hist2.record(Duration::from_micros(200));
    ///
    /// hist1.merge(&hist2).unwrap();
    /// assert_eq!(hist1.len(), 2);
    /// ```
    pub fn merge(&mut self, other: &LatencyHistogram) -> Result<()> {
        self.histogram
            .add(&other.histogram)
            .map_err(|e| anyhow::anyhow!("Failed to merge histograms: {}", e))?;
        Ok(())
    }

    /// Reset the histogram to empty state
    ///
    /// Clears all recorded samples. This is useful for resetting statistics
    /// between test phases.
    pub fn reset(&mut self) {
        self.histogram.reset();
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_histogram() {
        let hist = LatencyHistogram::new();
        assert_eq!(hist.len(), 0);
        assert!(hist.is_empty());
    }

    #[test]
    fn test_record_single() {
        let mut hist = LatencyHistogram::new();
        hist.record(Duration::from_micros(100));

        assert_eq!(hist.len(), 1);
        assert!(!hist.is_empty());
    }

    #[test]
    fn test_record_multiple() {
        let mut hist = LatencyHistogram::new();
        hist.record(Duration::from_micros(100));
        hist.record(Duration::from_micros(200));
        hist.record(Duration::from_micros(300));

        assert_eq!(hist.len(), 3);
    }

    #[test]
    fn test_percentile() {
        let mut hist = LatencyHistogram::new();
        for i in 1..=100 {
            hist.record(Duration::from_micros(i * 10));
        }

        let p50 = hist.percentile(50.0).unwrap();
        let p99 = hist.percentile(99.0).unwrap();

        // p50 should be around 500 microseconds
        assert!(p50.as_micros() >= 450 && p50.as_micros() <= 550);
        // p99 should be around 990 microseconds
        assert!(p99.as_micros() >= 940 && p99.as_micros() <= 1040);
    }

    #[test]
    fn test_percentile_empty() {
        let hist = LatencyHistogram::new();
        assert!(hist.percentile(50.0).is_none());
    }

    #[test]
    fn test_min_max() {
        let mut hist = LatencyHistogram::new();
        hist.record(Duration::from_micros(100));
        hist.record(Duration::from_micros(500));
        hist.record(Duration::from_micros(200));

        let min = hist.min().unwrap();
        let max = hist.max().unwrap();

        assert!(min.as_micros() >= 95 && min.as_micros() <= 105);
        assert!(max.as_micros() >= 495 && max.as_micros() <= 505);
    }

    #[test]
    fn test_mean() {
        let mut hist = LatencyHistogram::new();
        hist.record(Duration::from_micros(100));
        hist.record(Duration::from_micros(200));
        hist.record(Duration::from_micros(300));

        let mean = hist.mean().unwrap();
        // Mean should be around 200 microseconds
        assert!(mean.as_micros() >= 190 && mean.as_micros() <= 210);
    }

    #[test]
    fn test_stddev() {
        let mut hist = LatencyHistogram::new();
        hist.record(Duration::from_micros(100));
        hist.record(Duration::from_micros(200));
        hist.record(Duration::from_micros(300));

        let stddev = hist.stddev().unwrap();
        // Standard deviation should be around 81.6 microseconds
        assert!(stddev.as_micros() >= 70 && stddev.as_micros() <= 90);
    }

    #[test]
    fn test_merge() {
        let mut hist1 = LatencyHistogram::new();
        hist1.record(Duration::from_micros(100));
        hist1.record(Duration::from_micros(200));

        let mut hist2 = LatencyHistogram::new();
        hist2.record(Duration::from_micros(300));
        hist2.record(Duration::from_micros(400));

        hist1.merge(&hist2).unwrap();

        assert_eq!(hist1.len(), 4);
        let mean = hist1.mean().unwrap();
        // Mean should be around 250 microseconds
        assert!(mean.as_micros() >= 240 && mean.as_micros() <= 260);
    }

    #[test]
    fn test_reset() {
        let mut hist = LatencyHistogram::new();
        hist.record(Duration::from_micros(100));
        hist.record(Duration::from_micros(200));

        assert_eq!(hist.len(), 2);

        hist.reset();

        assert_eq!(hist.len(), 0);
        assert!(hist.is_empty());
    }

    #[test]
    fn test_nanosecond_precision() {
        let mut hist = LatencyHistogram::new();
        hist.record(Duration::from_nanos(500));
        hist.record(Duration::from_nanos(1000));

        assert_eq!(hist.len(), 2);
        let min = hist.min().unwrap();
        assert!(min.as_nanos() >= 450 && min.as_nanos() <= 550);
    }

    #[test]
    fn test_millisecond_range() {
        let mut hist = LatencyHistogram::new();
        hist.record(Duration::from_millis(1));
        hist.record(Duration::from_millis(10));
        hist.record(Duration::from_millis(100));

        assert_eq!(hist.len(), 3);
        let max = hist.max().unwrap();
        assert!(max.as_millis() >= 99 && max.as_millis() <= 101);
    }

    #[test]
    fn test_second_range() {
        let mut hist = LatencyHistogram::new();
        hist.record(Duration::from_secs(1));
        hist.record(Duration::from_secs(5));

        assert_eq!(hist.len(), 2);
        let max = hist.max().unwrap();
        assert!(max.as_secs() >= 4 && max.as_secs() <= 6);
    }
}
