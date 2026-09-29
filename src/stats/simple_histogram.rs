//! Simple latency histogram
//!
//! A fast, fixed-size histogram for latency tracking with logarithmic buckets.
//! Uses logarithmic buckets for efficient storage and fast bucket calculation.
//!
//! This is much faster than HdrHistogram for our use case:
//! - Fixed 112-bucket array (no dynamic allocation)
//! - Fast bucket calculation: log2(latency) * 4
//! - Simple array increment (no complex logic)
//! - Lock-free for single-threaded use

use std::time::Duration;

/// Number of buckets in the histogram
/// 28 log2 levels * 4 sub-buckets per level = 112 buckets
/// Covers latencies from 0 to 2^28 microseconds (~268 seconds)
const NUM_BUCKETS: usize = 112;

/// Bucket fraction: 4 means 1/4 = 0.25 increments between buckets
const BUCKET_FRACTION: usize = 4;

/// Simple latency histogram with logarithmic buckets
///
/// Optimized for performance with fast bucket calculation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimpleHistogram {
    /// Histogram buckets (counts per latency range)
    #[serde(with = "serde_arrays")]
    buckets: [u64; NUM_BUCKETS],
    
    /// Total number of samples
    num_samples: u64,
    
    /// Sum of all latencies in nanoseconds
    total_nanos: u64,
    
    /// Minimum latency in nanoseconds
    min_nanos: u64,
    
    /// Maximum latency in nanoseconds
    max_nanos: u64,
}

// Helper module for serializing large arrays
mod serde_arrays {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    
    pub fn serialize<S>(arr: &[u64; 112], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        arr.as_slice().serialize(serializer)
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u64; 112], D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<u64> = Vec::deserialize(deserializer)?;
        if vec.len() != 112 {
            return Err(serde::de::Error::custom(format!("Expected 112 elements, got {}", vec.len())));
        }
        let mut arr = [0u64; 112];
        arr.copy_from_slice(&vec);
        Ok(arr)
    }
}

impl SimpleHistogram {
    /// Create a new empty histogram
    pub fn new() -> Self {
        Self {
            buckets: [0; NUM_BUCKETS],
            num_samples: 0,
            total_nanos: 0,
            min_nanos: u64::MAX,
            max_nanos: 0,
        }
    }
    
    /// Record a latency sample
    ///
    /// This is the hot path - optimized for speed.
    #[inline(always)]
    pub fn record(&mut self, latency: Duration) {
        let nanos = latency.as_nanos() as u64;
        
        // Update counters
        self.num_samples += 1;
        self.total_nanos += nanos;
        
        // Update min/max
        if nanos < self.min_nanos {
            self.min_nanos = nanos;
        }
        if nanos > self.max_nanos {
            self.max_nanos = nanos;
        }
        
        // Calculate bucket index
        // Convert to microseconds for bucket calculation
        let micros = nanos / 1000;
        
        let bucket_idx = if micros == 0 {
            0  // Special case: log2(0) doesn't exist
        } else {
            // Calculate log2 level (floor of log2)
            let log2_val = 63 - micros.leading_zeros() as usize;
            
            // Calculate base value for this log2 level (2^log2_val)
            let base = 1u64 << log2_val;
            
            // Calculate offset within this log2 level
            let offset_in_level = micros - base;
            
            // Each log2 level is divided into BUCKET_FRACTION sub-buckets
            // Calculate which sub-bucket (0 to BUCKET_FRACTION-1) this value falls into
            let level_size = base;
            let sub_bucket = ((offset_in_level * BUCKET_FRACTION as u64) / level_size) as usize;
            
            // Final bucket index = (log2_level * BUCKET_FRACTION) + sub_bucket
            let idx = log2_val * BUCKET_FRACTION + sub_bucket;
            idx.min(NUM_BUCKETS - 1)  // Clamp to max bucket
        };
        
        self.buckets[bucket_idx] += 1;
    }
    
    /// Get the number of samples
    pub fn len(&self) -> u64 {
        self.num_samples
    }
    
    /// Check if histogram is empty
    pub fn is_empty(&self) -> bool {
        self.num_samples == 0
    }
    
    /// Get minimum latency
    pub fn min(&self) -> Duration {
        if self.num_samples == 0 {
            Duration::from_nanos(0)
        } else {
            Duration::from_nanos(self.min_nanos)
        }
    }
    
    /// Get maximum latency
    pub fn max(&self) -> Duration {
        if self.num_samples == 0 {
            Duration::from_nanos(0)
        } else {
            Duration::from_nanos(self.max_nanos)
        }
    }
    
    /// Get mean latency
    pub fn mean(&self) -> Duration {
        if self.num_samples == 0 {
            Duration::from_nanos(0)
        } else {
            Duration::from_nanos(self.total_nanos / self.num_samples)
        }
    }
    
    /// Calculate a percentile value
    ///
    /// # Arguments
    ///
    /// * `percentile` - Percentile to calculate (0.0 to 100.0)
    ///
    /// # Returns
    ///
    /// The latency value at the given percentile
    pub fn percentile(&self, percentile: f64) -> Duration {
        if self.num_samples == 0 {
            return Duration::from_nanos(0);
        }
        
        let target_count = ((percentile / 100.0) * self.num_samples as f64) as u64;
        let mut cumulative = 0u64;
        
        for (idx, &count) in self.buckets.iter().enumerate() {
            cumulative += count;
            if cumulative >= target_count {
                // Special handling for bucket 0 (sub-microsecond latencies)
                if idx == 0 {
                    // Bucket 0 represents 0-999ns
                    // Return 500ns as the midpoint for better display
                    return Duration::from_nanos(500);
                }
                
                // Convert bucket index back to microseconds
                let micros = bucket_idx_to_micros(idx);
                return Duration::from_micros(micros);
            }
        }
        
        // Shouldn't reach here, but return max if we do
        self.max()
    }
    
    /// Merge another histogram into this one
    ///
    /// Used for aggregating statistics from multiple workers.
    pub fn merge(&mut self, other: &SimpleHistogram) {
        for (i, &count) in other.buckets.iter().enumerate() {
            self.buckets[i] += count;
        }
        
        self.num_samples += other.num_samples;
        self.total_nanos += other.total_nanos;
        self.min_nanos = self.min_nanos.min(other.min_nanos);
        self.max_nanos = self.max_nanos.max(other.max_nanos);
    }
    
    /// Reset the histogram
    pub fn reset(&mut self) {
        self.buckets = [0; NUM_BUCKETS];
        self.num_samples = 0;
        self.total_nanos = 0;
        self.min_nanos = u64::MAX;
        self.max_nanos = 0;
    }
    
    /// Get bucket count at index
    ///
    /// Returns the number of samples in the specified bucket.
    pub fn bucket_count(&self, index: usize) -> u64 {
        if index < NUM_BUCKETS {
            self.buckets[index]
        } else {
            0
        }
    }
    
    /// Get all buckets as a slice
    pub fn buckets(&self) -> &[u64; NUM_BUCKETS] {
        &self.buckets
    }
}

impl Default for SimpleHistogram {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert bucket index back to microseconds (approximate)
///
/// Returns the midpoint value for the bucket range.
pub fn bucket_idx_to_micros(idx: usize) -> u64 {
    if idx == 0 {
        // Bucket 0 represents sub-microsecond latencies (0-999ns)
        // Return 0.5 microseconds (500ns) as the midpoint
        // But since we return u64 microseconds, we return 0
        // The caller should handle this specially for display
        return 0;
    }
    
    // Reverse the bucket calculation
    // Each log2 level has BUCKET_FRACTION sub-buckets
    let log2_val = idx / BUCKET_FRACTION;
    let sub_bucket = idx % BUCKET_FRACTION;
    
    // Base value for this log2 level
    let base = 1u64 << log2_val;
    
    // Add fractional increment within the level
    // Each sub-bucket represents 1/BUCKET_FRACTION of the range
    let range = base;
    let increment = (range * sub_bucket as u64) / BUCKET_FRACTION as u64;
    
    base + increment
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_histogram_basic() {
        let mut hist = SimpleHistogram::new();
        
        assert_eq!(hist.len(), 0);
        assert!(hist.is_empty());
        
        hist.record(Duration::from_micros(10));
        assert_eq!(hist.len(), 1);
        assert!(!hist.is_empty());
    }
    
    #[test]
    fn test_simple_histogram_min_max() {
        let mut hist = SimpleHistogram::new();
        
        hist.record(Duration::from_micros(5));
        hist.record(Duration::from_micros(10));
        hist.record(Duration::from_micros(3));
        
        assert_eq!(hist.min().as_micros(), 3);
        assert_eq!(hist.max().as_micros(), 10);
        assert_eq!(hist.len(), 3);
    }
    
    #[test]
    fn test_simple_histogram_mean() {
        let mut hist = SimpleHistogram::new();
        
        hist.record(Duration::from_micros(10));
        hist.record(Duration::from_micros(20));
        hist.record(Duration::from_micros(30));
        
        assert_eq!(hist.mean().as_micros(), 20);
    }
    
    #[test]
    fn test_simple_histogram_percentile() {
        let mut hist = SimpleHistogram::new();
        
        // Add 100 samples from 1-100 microseconds
        for i in 1..=100 {
            hist.record(Duration::from_micros(i));
        }
        
        // p50 should be around 50 microseconds
        let p50 = hist.percentile(50.0);
        assert!(p50.as_micros() >= 32 && p50.as_micros() <= 64);
        
        // p99 should be around 99 microseconds
        let p99 = hist.percentile(99.0);
        assert!(p99.as_micros() >= 64 && p99.as_micros() <= 128);
    }
    
    #[test]
    fn test_simple_histogram_merge() {
        let mut hist1 = SimpleHistogram::new();
        let mut hist2 = SimpleHistogram::new();
        
        hist1.record(Duration::from_micros(10));
        hist1.record(Duration::from_micros(20));
        
        hist2.record(Duration::from_micros(30));
        hist2.record(Duration::from_micros(40));
        
        hist1.merge(&hist2);
        
        assert_eq!(hist1.len(), 4);
        assert_eq!(hist1.min().as_micros(), 10);
        assert_eq!(hist1.max().as_micros(), 40);
        assert_eq!(hist1.mean().as_micros(), 25);
    }
    
    #[test]
    fn test_simple_histogram_zero_latency() {
        let mut hist = SimpleHistogram::new();
        
        hist.record(Duration::from_nanos(0));
        hist.record(Duration::from_nanos(500));
        
        assert_eq!(hist.len(), 2);
        assert_eq!(hist.min().as_nanos(), 0);
    }
}
