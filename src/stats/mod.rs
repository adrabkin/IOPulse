//! Statistics collection
//!
//! Lock-free statistics with latency histograms and cache-line aligned counters.
//!
//! This module provides comprehensive statistics tracking for IO operations with
//! minimal overhead. Key features:
//!
//! - **Lock-free atomic counters**: IOPS and throughput tracking without contention
//! - **Cache-line alignment**: Prevents false sharing between worker threads
//! - **HdrHistogram integration**: High-precision latency percentiles
//! - **Metadata operation tracking**: Separate statistics for filesystem operations
//! - **Lock latency tracking**: Optional tracking of file lock acquisition times
//! - **Merge operations**: Aggregate statistics from multiple workers
//!
//! # Example
//!
//! ```
//! use iopulse::stats::WorkerStats;
//! use iopulse::engine::OperationType;
//! use std::time::Duration;
//!
//! let mut stats = WorkerStats::new();
//!
//! // Record a read operation
//! stats.record_io(OperationType::Read, 4096, Duration::from_micros(100));
//!
//! // Record a write operation
//! stats.record_io(OperationType::Write, 8192, Duration::from_micros(150));
//!
//! // Get statistics
//! let read_ops = stats.read_ops();
//! let write_ops = stats.write_ops();
//! let total_bytes = stats.total_bytes();
//! ```

pub mod histogram;
pub mod simple_histogram;
pub mod aggregator;
pub mod live;

use crate::engine::OperationType;
use crate::Result;
use simple_histogram::SimpleHistogram as LatencyHistogram;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::collections::HashSet;

/// Cache-line aligned atomic counter to prevent false sharing
///
/// On most modern CPUs, cache lines are 64 bytes. When multiple threads update
/// adjacent memory locations, the entire cache line is invalidated, causing
/// performance degradation (false sharing). By aligning each counter to a cache
/// line boundary and padding to 64 bytes, we ensure each counter occupies its
/// own cache line.
///
/// # Memory Layout
///
/// ```text
/// [value: 8 bytes][padding: 56 bytes] = 64 bytes total
/// ```
#[repr(align(64))]
#[derive(Debug)]
pub struct AlignedCounter {
    value: AtomicU64,
    _padding: [u8; 56],
}

impl AlignedCounter {
    /// Create a new counter with initial value 0
    pub fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
            _padding: [0; 56],
        }
    }

    /// Create a new counter with the specified initial value
    pub fn with_value(val: u64) -> Self {
        Self {
            value: AtomicU64::new(val),
            _padding: [0; 56],
        }
    }

    /// Increment the counter by the specified amount
    ///
    /// Uses `Ordering::Relaxed` for maximum performance. This is safe because
    /// we don't need ordering guarantees between different counters.
    #[inline]
    pub fn add(&self, val: u64) {
        self.value.fetch_add(val, Ordering::Relaxed);
    }

    /// Get the current value of the counter
    ///
    /// Uses `Ordering::Relaxed` for maximum performance.
    #[inline]
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Set the counter to a specific value
    #[inline]
    pub fn set(&self, val: u64) {
        self.value.store(val, Ordering::Relaxed);
    }
}

impl Default for AlignedCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// Metadata operation statistics
///
/// Tracks IOPS and latency for filesystem metadata operations. These operations
/// are tracked separately from data IO operations because they have different
/// performance characteristics and are often the bottleneck in distributed
/// filesystems like NFS and Lustre.
///
/// # Operations Tracked
///
/// - **open**: File open operations
/// - **close**: File close operations
/// - **stat**: File stat/getattr operations
/// - **setattr**: File attribute modification (chmod, chown, utime)
/// - **mkdir**: Directory creation
/// - **rmdir**: Directory removal
/// - **unlink**: File deletion
/// - **rename**: File/directory rename
/// - **readdir**: Directory listing
/// - **fsync**: File synchronization
#[derive(Debug)]
pub struct MetadataStats {
    // Operation counters (cache-line aligned)
    pub open_ops: AlignedCounter,
    pub close_ops: AlignedCounter,
    pub stat_ops: AlignedCounter,
    pub setattr_ops: AlignedCounter,
    pub mkdir_ops: AlignedCounter,
    pub rmdir_ops: AlignedCounter,
    pub unlink_ops: AlignedCounter,
    pub rename_ops: AlignedCounter,
    pub readdir_ops: AlignedCounter,
    pub fsync_ops: AlignedCounter,

    // Latency histograms (no mutex needed - per-worker)
    pub open_latency: LatencyHistogram,
    pub close_latency: LatencyHistogram,
    pub stat_latency: LatencyHistogram,
    pub setattr_latency: LatencyHistogram,
    pub mkdir_latency: LatencyHistogram,
    pub rmdir_latency: LatencyHistogram,
    pub unlink_latency: LatencyHistogram,
    pub rename_latency: LatencyHistogram,
    pub readdir_latency: LatencyHistogram,
    pub fsync_latency: LatencyHistogram,
}

impl MetadataStats {
    /// Create a new metadata statistics tracker
    pub fn new() -> Self {
        Self {
            open_ops: AlignedCounter::new(),
            close_ops: AlignedCounter::new(),
            stat_ops: AlignedCounter::new(),
            setattr_ops: AlignedCounter::new(),
            mkdir_ops: AlignedCounter::new(),
            rmdir_ops: AlignedCounter::new(),
            unlink_ops: AlignedCounter::new(),
            rename_ops: AlignedCounter::new(),
            readdir_ops: AlignedCounter::new(),
            fsync_ops: AlignedCounter::new(),
            open_latency: LatencyHistogram::new(),
            close_latency: LatencyHistogram::new(),
            stat_latency: LatencyHistogram::new(),
            setattr_latency: LatencyHistogram::new(),
            mkdir_latency: LatencyHistogram::new(),
            rmdir_latency: LatencyHistogram::new(),
            unlink_latency: LatencyHistogram::new(),
            rename_latency: LatencyHistogram::new(),
            readdir_latency: LatencyHistogram::new(),
            fsync_latency: LatencyHistogram::new(),
        }
    }

    /// Get total metadata operations across all types
    pub fn total_ops(&self) -> u64 {
        self.open_ops.get()
            + self.close_ops.get()
            + self.stat_ops.get()
            + self.setattr_ops.get()
            + self.mkdir_ops.get()
            + self.rmdir_ops.get()
            + self.unlink_ops.get()
            + self.rename_ops.get()
            + self.readdir_ops.get()
            + self.fsync_ops.get()
    }

    /// Merge another MetadataStats into this one
    ///
    /// This is used to aggregate statistics from multiple workers. Counters are
    /// added and histograms are merged.
    pub fn merge(&mut self, other: &MetadataStats) -> Result<()> {
        // Merge counters
        self.open_ops.add(other.open_ops.get());
        self.close_ops.add(other.close_ops.get());
        self.stat_ops.add(other.stat_ops.get());
        self.setattr_ops.add(other.setattr_ops.get());
        self.mkdir_ops.add(other.mkdir_ops.get());
        self.rmdir_ops.add(other.rmdir_ops.get());
        self.unlink_ops.add(other.unlink_ops.get());
        self.rename_ops.add(other.rename_ops.get());
        self.readdir_ops.add(other.readdir_ops.get());
        self.fsync_ops.add(other.fsync_ops.get());

        // Merge histograms
        self.open_latency.merge(&other.open_latency);
        self.close_latency.merge(&other.close_latency);
        self.stat_latency.merge(&other.stat_latency);
        self.setattr_latency.merge(&other.setattr_latency);
        self.mkdir_latency.merge(&other.mkdir_latency);
        self.rmdir_latency.merge(&other.rmdir_latency);
        self.unlink_latency.merge(&other.unlink_latency);
        self.rename_latency.merge(&other.rename_latency);
        self.readdir_latency.merge(&other.readdir_latency);
        self.fsync_latency.merge(&other.fsync_latency);

        Ok(())
    }
}impl Default for MetadataStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-worker statistics with cache-line aligned counters
///
/// This structure tracks all IO statistics for a single worker thread. It uses
/// cache-line aligned atomic counters to prevent false sharing when multiple
/// workers update their statistics concurrently.
///
/// # Performance Considerations
///
/// - **Atomic counters**: Lock-free updates with `Ordering::Relaxed`
/// - **Cache-line alignment**: Each counter on its own cache line (64 bytes)
/// - **Histogram updates**: Infrequent, use `Arc<Mutex<>>` for simplicity
/// - **No allocations**: All structures pre-allocated during initialization
///
/// # Example
///
/// ```
/// use iopulse::stats::WorkerStats;
/// use iopulse::engine::OperationType;
/// use std::time::Duration;
///
/// let mut stats = WorkerStats::new();
///
/// // Record operations
/// stats.record_io(OperationType::Read, 4096, Duration::from_micros(100));
/// stats.record_io(OperationType::Write, 8192, Duration::from_micros(150));
///
/// // Get statistics
/// assert_eq!(stats.read_ops(), 1);
/// assert_eq!(stats.write_ops(), 1);
/// assert_eq!(stats.read_bytes(), 4096);
/// assert_eq!(stats.write_bytes(), 8192);
/// ```
#[derive(Debug)]
pub struct WorkerStats {
    // IO operation counters (cache-line aligned)
    read_ops: AlignedCounter,
    write_ops: AlignedCounter,
    read_bytes: AlignedCounter,
    write_bytes: AlignedCounter,
    errors: AlignedCounter,
    
    // Verification counters (when --verify is enabled)
    verify_ops: AlignedCounter,
    verify_failures: AlignedCounter,
    
    // Block size verification (min/max bytes per operation)
    min_bytes_per_op: AtomicU64,
    max_bytes_per_op: AtomicU64,
    
    // Queue depth utilization (for async engines)
    current_queue_depth: AtomicU64,
    peak_queue_depth: AtomicU64,
    queue_depth_samples: AtomicU64,
    queue_depth_sum: AtomicU64,
    
    // Error breakdown by type
    errors_read: AtomicU64,
    errors_write: AtomicU64,
    errors_metadata: AtomicU64,

    // Latency histogram for data IO operations (no mutex needed - per-worker)
    io_latency: LatencyHistogram,
    
    // Separate read/write latency histograms (for detailed analysis)
    read_latency: LatencyHistogram,
    write_latency: LatencyHistogram,

    // Metadata operation statistics
    pub metadata: MetadataStats,

    // Lock latency histogram (optional, only when locking is enabled)
    lock_latency: Option<LatencyHistogram>,
    
    // Block access heatmap (optional, only when --heatmap is enabled)
    // Maps block number to access count
    block_heatmap: Option<Arc<Mutex<std::collections::HashMap<u64, u64>>>>,
    
    // Unique block tracking (optional, tracks which blocks have been accessed)
    // Used to calculate coverage percentage and rewrite percentage
    unique_blocks: Option<Arc<Mutex<HashSet<u64>>>>,
    
    // Actual test duration (excludes setup time like preallocation)
    // Set by worker at end of test
    test_duration: Option<Duration>,
    
    // Resource utilization tracking (CPU and memory)
    resource_tracker: Arc<Mutex<crate::util::resource::ResourceTracker>>,
}

impl WorkerStats {
    /// Create a new worker statistics tracker
    ///
    /// # Arguments
    ///
    /// * `track_lock_latency` - Whether to track file lock acquisition latency
    pub fn new() -> Self {
        Self::with_lock_tracking(false)
    }

    /// Create a new worker statistics tracker with optional lock latency tracking
    ///
    /// # Arguments
    ///
    /// * `track_lock_latency` - Whether to track file lock acquisition latency
    pub fn with_lock_tracking(track_lock_latency: bool) -> Self {
        Self {
            read_ops: AlignedCounter::new(),
            write_ops: AlignedCounter::new(),
            read_bytes: AlignedCounter::new(),
            write_bytes: AlignedCounter::new(),
            errors: AlignedCounter::new(),
            verify_ops: AlignedCounter::new(),
            verify_failures: AlignedCounter::new(),
            min_bytes_per_op: AtomicU64::new(u64::MAX),
            max_bytes_per_op: AtomicU64::new(0),
            current_queue_depth: AtomicU64::new(0),
            peak_queue_depth: AtomicU64::new(0),
            queue_depth_samples: AtomicU64::new(0),
            queue_depth_sum: AtomicU64::new(0),
            errors_read: AtomicU64::new(0),
            errors_write: AtomicU64::new(0),
            errors_metadata: AtomicU64::new(0),
            io_latency: LatencyHistogram::new(),
            read_latency: LatencyHistogram::new(),
            write_latency: LatencyHistogram::new(),
            metadata: MetadataStats::new(),
            lock_latency: if track_lock_latency {
                Some(LatencyHistogram::new())
            } else {
                None
            },
            block_heatmap: None,  // Disabled by default
            unique_blocks: Some(Arc::new(Mutex::new(HashSet::new()))),  // Always enabled for coverage tracking
            test_duration: None,  // Set by worker at end of test
            resource_tracker: Arc::new(Mutex::new(crate::util::resource::ResourceTracker::new())),
        }
    }
    
    /// Create a new worker statistics tracker with heatmap tracking enabled
    ///
    /// # Arguments
    ///
    /// * `track_lock_latency` - Whether to track file lock acquisition latency
    /// * `enable_heatmap` - Whether to track per-block access counts
    pub fn with_heatmap(track_lock_latency: bool, enable_heatmap: bool) -> Self {
        Self {
            read_ops: AlignedCounter::new(),
            write_ops: AlignedCounter::new(),
            read_bytes: AlignedCounter::new(),
            write_bytes: AlignedCounter::new(),
            errors: AlignedCounter::new(),
            verify_ops: AlignedCounter::new(),
            verify_failures: AlignedCounter::new(),
            min_bytes_per_op: AtomicU64::new(u64::MAX),
            max_bytes_per_op: AtomicU64::new(0),
            current_queue_depth: AtomicU64::new(0),
            peak_queue_depth: AtomicU64::new(0),
            queue_depth_samples: AtomicU64::new(0),
            queue_depth_sum: AtomicU64::new(0),
            errors_read: AtomicU64::new(0),
            errors_write: AtomicU64::new(0),
            errors_metadata: AtomicU64::new(0),
            io_latency: LatencyHistogram::new(),
            read_latency: LatencyHistogram::new(),
            write_latency: LatencyHistogram::new(),
            metadata: MetadataStats::new(),
            lock_latency: if track_lock_latency {
                Some(LatencyHistogram::new())
            } else {
                None
            },
            block_heatmap: if enable_heatmap {
                Some(Arc::new(Mutex::new(std::collections::HashMap::new())))
            } else {
                None
            },
            unique_blocks: Some(Arc::new(Mutex::new(HashSet::new()))),  // Always enabled for coverage tracking
            test_duration: None,  // Set by worker at end of test
            resource_tracker: Arc::new(Mutex::new(crate::util::resource::ResourceTracker::new())),
        }
    }

    /// Record an IO operation
    ///
    /// Updates the appropriate counters and histogram based on the operation type.
    ///
    /// # Arguments
    ///
    /// * `op_type` - Type of operation (Read, Write, Fsync, Fdatasync)
    /// * `bytes` - Number of bytes transferred (0 for fsync operations)
    /// * `latency` - Duration of the operation
    #[inline(always)]
    pub fn record_io(&mut self, op_type: OperationType, bytes: usize, latency: Duration) {
        // Track min/max bytes per operation (for block size verification)
        let bytes_u64 = bytes as u64;
        if bytes_u64 > 0 {
            // Update min
            let mut current_min = self.min_bytes_per_op.load(Ordering::Relaxed);
            while bytes_u64 < current_min {
                match self.min_bytes_per_op.compare_exchange_weak(
                    current_min,
                    bytes_u64,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(x) => current_min = x,
                }
            }
            
            // Update max
            let mut current_max = self.max_bytes_per_op.load(Ordering::Relaxed);
            while bytes_u64 > current_max {
                match self.max_bytes_per_op.compare_exchange_weak(
                    current_max,
                    bytes_u64,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(x) => current_max = x,
                }
            }
        }
        
        match op_type {
            OperationType::Read => {
                self.read_ops.add(1);
                self.read_bytes.add(bytes as u64);
                self.read_latency.record(latency);
            }
            OperationType::Write => {
                self.write_ops.add(1);
                self.write_bytes.add(bytes as u64);
                self.write_latency.record(latency);
            }
            OperationType::Fsync | OperationType::Fdatasync => {
                self.metadata.fsync_ops.add(1);
                self.metadata.fsync_latency.record(latency);
                return; // Don't record in io_latency histogram
            }
        }

        // Record latency in combined histogram (for backward compatibility)
        self.io_latency.record(latency);
    }
    
    /// Record an error
    #[inline]
    pub fn record_error(&mut self) {
        self.errors.add(1);
    }
    
    /// Record a verification operation
    #[inline]
    pub fn record_verification(&mut self) {
        self.verify_ops.add(1);
    }
    
    /// Record a verification failure
    #[inline]
    pub fn record_verification_failure(&mut self) {
        self.verify_failures.add(1);
    }
    
    /// Record block access for heatmap
    ///
    /// Only records if heatmap tracking is enabled.
    ///
    /// # Arguments
    ///
    /// * `block_num` - Block number that was accessed
    /// Record block access for heatmap
    ///
    /// Only records if heatmap tracking is enabled.
    ///
    /// # Arguments
    ///
    /// * `block_num` - Block number that was accessed
    #[inline]
    pub fn record_block_access(&self, block_num: u64) {
        if let Some(ref heatmap) = self.block_heatmap {
            if let Ok(mut map) = heatmap.lock() {
                *map.entry(block_num).or_insert(0) += 1;
            }
        }
    }
    
    /// Record unique block access for coverage tracking
    ///
    /// Tracks which blocks have been accessed at least once.
    /// Used to calculate coverage percentage and rewrite percentage.
    ///
    /// # Arguments
    ///
    /// * `block_num` - Block number that was accessed
    #[inline]
    pub fn record_unique_block(&self, block_num: u64) {
        if let Some(ref unique) = self.unique_blocks {
            if let Ok(mut set) = unique.lock() {
                set.insert(block_num);
            }
        }
    }
    
    /// Get the number of unique blocks accessed
    ///
    /// Returns the count of distinct blocks that have been accessed at least once.
    pub fn unique_blocks_count(&self) -> u64 {
        if let Some(ref unique) = self.unique_blocks {
            if let Ok(set) = unique.lock() {
                return set.len() as u64;
            }
        }
        0
    }
    
    /// Calculate coverage percentage
    ///
    /// Returns the percentage of total blocks that have been accessed.
    ///
    /// # Arguments
    ///
    /// * `total_blocks` - Total number of blocks in the file/device
    ///
    /// # Returns
    ///
    /// Coverage percentage (0.0 - 100.0)
    pub fn coverage_percent(&self, total_blocks: u64) -> f64 {
        if total_blocks == 0 {
            return 0.0;
        }
        let unique = self.unique_blocks_count();
        (unique as f64 / total_blocks as f64) * 100.0
    }
    
    /// Calculate rewrite percentage
    ///
    /// Returns the percentage of operations that accessed previously-accessed blocks.
    ///
    /// # Returns
    ///
    /// Rewrite percentage (0.0 - 100.0)
    pub fn rewrite_percent(&self) -> f64 {
        let total_ops = self.total_ops();
        if total_ops == 0 {
            return 0.0;
        }
        let unique = self.unique_blocks_count();
        if unique >= total_ops {
            return 0.0;  // No rewrites if unique >= total
        }
        let rewrites = total_ops - unique;
        (rewrites as f64 / total_ops as f64) * 100.0
    }

    /// Record lock acquisition latency
    ///
    /// Only records if lock latency tracking is enabled.
    ///
    /// # Arguments
    ///
    /// * `latency` - Duration of lock acquisition
    #[inline]
    pub fn record_lock_latency(&mut self, latency: Duration) {
        if let Some(ref mut hist) = self.lock_latency {
            hist.record(latency);
        }
    }

    /// Get the number of read operations
    #[inline]
    pub fn read_ops(&self) -> u64 {
        self.read_ops.get()
    }

    /// Get the number of write operations
    #[inline]
    pub fn write_ops(&self) -> u64 {
        self.write_ops.get()
    }

    /// Get the number of bytes read
    #[inline]
    pub fn read_bytes(&self) -> u64 {
        self.read_bytes.get()
    }

    /// Get the number of bytes written
    #[inline]
    pub fn write_bytes(&self) -> u64 {
        self.write_bytes.get()
    }

    /// Get the total number of bytes transferred (read + write)
    #[inline]
    pub fn total_bytes(&self) -> u64 {
        self.read_bytes.get() + self.write_bytes.get()
    }

    /// Get the total number of IO operations (read + write)
    #[inline]
    pub fn total_ops(&self) -> u64 {
        self.read_ops.get() + self.write_ops.get()
    }

    /// Get the number of errors
    #[inline]
    pub fn errors(&self) -> u64 {
        self.errors.get()
    }
    
    /// Get the number of read errors
    #[inline]
    pub fn errors_read(&self) -> u64 {
        self.errors_read.load(Ordering::Relaxed)
    }
    
    /// Get the number of write errors
    #[inline]
    pub fn errors_write(&self) -> u64 {
        self.errors_write.load(Ordering::Relaxed)
    }
    
    /// Get the number of metadata errors
    #[inline]
    pub fn errors_metadata(&self) -> u64 {
        self.errors_metadata.load(Ordering::Relaxed)
    }
    
    /// Get the number of verification operations
    #[inline]
    pub fn verify_ops(&self) -> u64 {
        self.verify_ops.get()
    }
    
    /// Get the number of verification failures
    #[inline]
    pub fn verify_failures(&self) -> u64 {
        self.verify_failures.get()
    }
    
    /// Get minimum bytes per operation
    #[inline]
    pub fn min_bytes_per_op(&self) -> u64 {
        let val = self.min_bytes_per_op.load(Ordering::Relaxed);
        if val == u64::MAX { 0 } else { val }
    }
    
    /// Get maximum bytes per operation
    #[inline]
    pub fn max_bytes_per_op(&self) -> u64 {
        self.max_bytes_per_op.load(Ordering::Relaxed)
    }
    
    /// Sample current queue depth (for async engines)
    #[inline]
    pub fn sample_queue_depth(&self, in_flight: u64) {
        self.current_queue_depth.store(in_flight, Ordering::Relaxed);
        self.queue_depth_samples.fetch_add(1, Ordering::Relaxed);
        self.queue_depth_sum.fetch_add(in_flight, Ordering::Relaxed);
        
        // Update peak
        let mut current_peak = self.peak_queue_depth.load(Ordering::Relaxed);
        while in_flight > current_peak {
            match self.peak_queue_depth.compare_exchange_weak(
                current_peak,
                in_flight,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_peak = x,
            }
        }
    }
    
    /// Get peak queue depth
    #[inline]
    pub fn peak_queue_depth(&self) -> u64 {
        self.peak_queue_depth.load(Ordering::Relaxed)
    }
    
    /// Get average queue depth
    #[inline]
    pub fn avg_queue_depth(&self) -> f64 {
        let samples = self.queue_depth_samples.load(Ordering::Relaxed);
        if samples > 0 {
            let sum = self.queue_depth_sum.load(Ordering::Relaxed);
            sum as f64 / samples as f64
        } else {
            0.0
        }
    }
    
    /// Set the test duration (actual IO time, excludes setup like preallocation)
    pub fn set_test_duration(&mut self, duration: Duration) {
        self.test_duration = Some(duration);
    }
    
    /// Get the test duration (actual IO time, excludes setup like preallocation)
    /// Returns None if not set
    pub fn test_duration(&self) -> Option<Duration> {
        self.test_duration
    }

    /// Get a reference to the IO latency histogram
    pub fn io_latency(&self) -> &LatencyHistogram {
        &self.io_latency
    }
    
    /// Get a reference to the read latency histogram
    pub fn read_latency(&self) -> &LatencyHistogram {
        &self.read_latency
    }
    
    /// Get a reference to the write latency histogram
    pub fn write_latency(&self) -> &LatencyHistogram {
        &self.write_latency
    }

    /// Get a reference to the lock latency histogram (if enabled)
    pub fn lock_latency(&self) -> Option<&LatencyHistogram> {
        self.lock_latency.as_ref()
    }
    
    /// Get the block access heatmap (if enabled)
    ///
    /// Returns a sorted vector of (block_num, access_count) pairs
    pub fn get_heatmap(&self) -> Option<Vec<(u64, u64)>> {
        if let Some(ref heatmap) = self.block_heatmap {
            if let Ok(map) = heatmap.lock() {
                let mut entries: Vec<(u64, u64)> = map.iter()
                    .map(|(&block, &count)| (block, count))
                    .collect();
                entries.sort_by_key(|&(block, _)| block);
                return Some(entries);
            }
        }
        None
    }
    
    /// Generate heatmap summary showing distribution of accesses
    ///
    /// Divides the file into buckets and shows operations per bucket.
    /// Returns None if heatmap tracking is not enabled.
    ///
    /// # Arguments
    ///
    /// * `num_buckets` - Number of buckets to divide file into (default: 100)
    /// * `total_blocks` - Total number of blocks in file
    pub fn heatmap_summary(&self, num_buckets: usize, total_blocks: u64) -> Option<String> {
        let entries = self.get_heatmap()?;
        
        if entries.is_empty() {
            return Some("No block accesses recorded".to_string());
        }
        
        // Create buckets
        let blocks_per_bucket = (total_blocks as f64 / num_buckets as f64).ceil() as u64;
        let mut buckets = vec![0u64; num_buckets];
        
        // Fill buckets with access counts
        for (block, count) in entries.iter() {
            let bucket_idx = (*block / blocks_per_bucket).min((num_buckets - 1) as u64) as usize;
            buckets[bucket_idx] += count;
        }
        
        // Calculate total operations
        let total_ops: u64 = buckets.iter().sum();
        
        // Find max for scaling
        let max_ops = *buckets.iter().max().unwrap_or(&1);
        
        // Generate output
        let mut output = String::new();
        output.push_str(&format!("\nBlock Access Heatmap ({} buckets):\n", num_buckets));
        output.push_str(&format!("Total operations: {}\n\n", total_ops));
        
        for (i, &ops) in buckets.iter().enumerate() {
            let start_block = i as u64 * blocks_per_bucket;
            let end_block = ((i + 1) as u64 * blocks_per_bucket).min(total_blocks) - 1;
            let percentage = (ops as f64 / total_ops as f64) * 100.0;
            
            // Create bar (scale to 50 chars max)
            let bar_len = ((ops as f64 / max_ops as f64) * 50.0) as usize;
            let bar = "█".repeat(bar_len);
            
            output.push_str(&format!(
                "[{:8}-{:8}] {:50} {:8} ops ({:5.2}%)\n",
                start_block, end_block, bar, ops, percentage
            ));
        }
        
        // Calculate top 20% vs bottom 80% split
        let split_point = (num_buckets as f64 * 0.2).ceil() as usize;
        let top_20_ops: u64 = buckets[..split_point].iter().sum();
        let bottom_80_ops: u64 = buckets[split_point..].iter().sum();
        
        output.push_str(&format!("\nDistribution Analysis:\n"));
        output.push_str(&format!("Top 20% of file:    {:8} ops ({:5.2}%)\n", 
            top_20_ops, (top_20_ops as f64 / total_ops as f64) * 100.0));
        output.push_str(&format!("Bottom 80% of file: {:8} ops ({:5.2}%)\n",
            bottom_80_ops, (bottom_80_ops as f64 / total_ops as f64) * 100.0));
        
        Some(output)
    }

    /// Merge another WorkerStats into this one
    ///
    /// This is used to aggregate statistics from multiple workers. All counters
    /// are added and histograms are merged.
    ///
    /// # Arguments
    ///
    /// * `other` - The statistics to merge into this one
    ///
    /// # Errors
    ///
    /// Returns an error if histogram merging fails.
    pub fn merge(&mut self, other: &WorkerStats) -> Result<()> {
        // Merge counters
        self.read_ops.add(other.read_ops.get());
        self.write_ops.add(other.write_ops.get());
        self.read_bytes.add(other.read_bytes.get());
        self.write_bytes.add(other.write_bytes.get());
        self.errors.add(other.errors.get());
        self.verify_ops.add(other.verify_ops.get());
        self.verify_failures.add(other.verify_failures.get());
        
        // Merge min/max bytes per op
        let other_min = other.min_bytes_per_op.load(Ordering::Relaxed);
        if other_min != u64::MAX {
            let mut current_min = self.min_bytes_per_op.load(Ordering::Relaxed);
            while other_min < current_min {
                match self.min_bytes_per_op.compare_exchange_weak(
                    current_min,
                    other_min,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(x) => current_min = x,
                }
            }
        }
        
        let other_max = other.max_bytes_per_op.load(Ordering::Relaxed);
        let mut current_max = self.max_bytes_per_op.load(Ordering::Relaxed);
        while other_max > current_max {
            match self.max_bytes_per_op.compare_exchange_weak(
                current_max,
                other_max,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }
        
        // Merge queue depth stats
        let other_peak = other.peak_queue_depth.load(Ordering::Relaxed);
        let mut current_peak = self.peak_queue_depth.load(Ordering::Relaxed);
        while other_peak > current_peak {
            match self.peak_queue_depth.compare_exchange_weak(
                current_peak,
                other_peak,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_peak = x,
            }
        }
        self.queue_depth_samples.fetch_add(other.queue_depth_samples.load(Ordering::Relaxed), Ordering::Relaxed);
        self.queue_depth_sum.fetch_add(other.queue_depth_sum.load(Ordering::Relaxed), Ordering::Relaxed);
        
        // Merge error breakdown
        self.errors_read.fetch_add(other.errors_read.load(Ordering::Relaxed), Ordering::Relaxed);
        self.errors_write.fetch_add(other.errors_write.load(Ordering::Relaxed), Ordering::Relaxed);
        self.errors_metadata.fetch_add(other.errors_metadata.load(Ordering::Relaxed), Ordering::Relaxed);

        // Merge IO latency histogram
        self.io_latency.merge(&other.io_latency);
        self.read_latency.merge(&other.read_latency);
        self.write_latency.merge(&other.write_latency);

        // Merge metadata statistics
        self.metadata.merge(&other.metadata)?;

        // Merge lock latency histogram if both have it
        if let (Some(ref mut self_lock), Some(ref other_lock)) =
            (&mut self.lock_latency, &other.lock_latency)
        {
            self_lock.merge(other_lock);
        }
        
        // Merge heatmaps if both have them
        if let (Some(ref self_heatmap), Some(ref other_heatmap)) =
            (&self.block_heatmap, &other.block_heatmap)
        {
            let mut self_map = self_heatmap.lock().unwrap();
            let other_map = other_heatmap.lock().unwrap();
            for (&block, &count) in other_map.iter() {
                *self_map.entry(block).or_insert(0) += count;
            }
        }
        
        // Merge unique blocks if both have them
        if let (Some(ref self_unique), Some(ref other_unique)) =
            (&self.unique_blocks, &other.unique_blocks)
        {
            let mut self_set = self_unique.lock().unwrap();
            let other_set = other_unique.lock().unwrap();
            for &block in other_set.iter() {
                self_set.insert(block);
            }
        }
        
        // Merge test duration (use max duration across all workers)
        // This ensures we use the longest worker's duration for IOPS calculation
        if let Some(other_duration) = other.test_duration {
            self.test_duration = Some(
                self.test_duration
                    .map(|d| d.max(other_duration))
                    .unwrap_or(other_duration)
            );
        }
        
        // For resource tracking, use the first worker's tracker that has data
        // All workers track the same process, so any worker's data is valid
        if self.resource_stats().is_none() && other.resource_stats().is_some() {
            // Copy the resource tracker from other to self
            if let Ok(other_tracker) = other.resource_tracker.lock() {
                if let Ok(mut self_tracker) = self.resource_tracker.lock() {
                    *self_tracker = other_tracker.clone();
                }
            }
        }

        Ok(())
    }
    
    /// Start resource tracking
    ///
    /// Takes an initial snapshot of CPU and memory usage.
    /// Call this at the start of the test.
    pub fn start_resource_tracking(&self) {
        if let Ok(mut tracker) = self.resource_tracker.lock() {
            tracker.start();
        }
    }
    
    /// Sample current resource utilization
    ///
    /// Takes a snapshot of current CPU and memory usage.
    /// Call this periodically during the test (e.g., every second).
    pub fn sample_resources(&self) {
        if let Ok(mut tracker) = self.resource_tracker.lock() {
            tracker.sample();
        }
    }
    
    /// Get resource utilization statistics
    ///
    /// Returns CPU percentage and memory usage, or None if tracking is not supported
    /// or no samples were taken.
    pub fn resource_stats(&self) -> Option<crate::util::resource::ResourceStats> {
        if let Ok(tracker) = self.resource_tracker.lock() {
            tracker.stats()
        } else {
            None
        }
    }
    
    /// Set statistics from a distributed WorkerStatsSnapshot
    ///
    /// This is used to reconstruct WorkerStats from network-serialized data.
    /// Used by distributed coordinator to rebuild stats for print_results().
    #[allow(clippy::too_many_arguments)]
    pub fn set_from_snapshot(
        &mut self,
        snapshot: &crate::distributed::protocol::WorkerStatsSnapshot,
        io_latency: crate::stats::simple_histogram::SimpleHistogram,
        read_latency: crate::stats::simple_histogram::SimpleHistogram,
        write_latency: crate::stats::simple_histogram::SimpleHistogram,
        metadata_open_latency: crate::stats::simple_histogram::SimpleHistogram,
        metadata_close_latency: crate::stats::simple_histogram::SimpleHistogram,
        metadata_stat_latency: crate::stats::simple_histogram::SimpleHistogram,
        metadata_setattr_latency: crate::stats::simple_histogram::SimpleHistogram,
        metadata_mkdir_latency: crate::stats::simple_histogram::SimpleHistogram,
        metadata_rmdir_latency: crate::stats::simple_histogram::SimpleHistogram,
        metadata_unlink_latency: crate::stats::simple_histogram::SimpleHistogram,
        metadata_rename_latency: crate::stats::simple_histogram::SimpleHistogram,
        metadata_readdir_latency: crate::stats::simple_histogram::SimpleHistogram,
        metadata_fsync_latency: crate::stats::simple_histogram::SimpleHistogram,
        lock_latency: Option<crate::stats::simple_histogram::SimpleHistogram>,
    ) -> Result<()> {
        // Set basic counters
        self.read_ops.set(snapshot.read_ops);
        self.write_ops.set(snapshot.write_ops);
        self.read_bytes.set(snapshot.read_bytes);
        self.write_bytes.set(snapshot.write_bytes);
        self.errors.set(snapshot.errors);
        
        // Set error breakdown
        self.errors_read.store(snapshot.errors_read, std::sync::atomic::Ordering::Relaxed);
        self.errors_write.store(snapshot.errors_write, std::sync::atomic::Ordering::Relaxed);
        self.errors_metadata.store(snapshot.errors_metadata, std::sync::atomic::Ordering::Relaxed);
        
        // Set verification stats
        self.verify_ops.set(snapshot.verify_ops);
        self.verify_failures.set(snapshot.verify_failures);
        
        // Set block size verification
        self.min_bytes_per_op.store(snapshot.min_bytes_per_op, std::sync::atomic::Ordering::Relaxed);
        self.max_bytes_per_op.store(snapshot.max_bytes_per_op, std::sync::atomic::Ordering::Relaxed);
        
        // Set queue depth stats
        self.peak_queue_depth.store(snapshot.peak_queue_depth, std::sync::atomic::Ordering::Relaxed);
        // Reconstruct queue_depth_sum and samples from average
        if snapshot.avg_queue_depth > 0.0 {
            // Use a reasonable sample count for reconstruction
            let samples = snapshot.read_ops + snapshot.write_ops;
            self.queue_depth_samples.store(samples, std::sync::atomic::Ordering::Relaxed);
            self.queue_depth_sum.store((snapshot.avg_queue_depth * samples as f64) as u64, std::sync::atomic::Ordering::Relaxed);
        }
        
        // Set latency histograms
        self.io_latency = io_latency;
        self.read_latency = read_latency;
        self.write_latency = write_latency;
        
        // Set metadata counters
        self.metadata.open_ops.set(snapshot.metadata_open_ops);
        self.metadata.close_ops.set(snapshot.metadata_close_ops);
        self.metadata.stat_ops.set(snapshot.metadata_stat_ops);
        self.metadata.setattr_ops.set(snapshot.metadata_setattr_ops);
        self.metadata.mkdir_ops.set(snapshot.metadata_mkdir_ops);
        self.metadata.rmdir_ops.set(snapshot.metadata_rmdir_ops);
        self.metadata.unlink_ops.set(snapshot.metadata_unlink_ops);
        self.metadata.rename_ops.set(snapshot.metadata_rename_ops);
        self.metadata.readdir_ops.set(snapshot.metadata_readdir_ops);
        self.metadata.fsync_ops.set(snapshot.metadata_fsync_ops);
        
        // Set metadata latency histograms
        self.metadata.open_latency = metadata_open_latency;
        self.metadata.close_latency = metadata_close_latency;
        self.metadata.stat_latency = metadata_stat_latency;
        self.metadata.setattr_latency = metadata_setattr_latency;
        self.metadata.mkdir_latency = metadata_mkdir_latency;
        self.metadata.rmdir_latency = metadata_rmdir_latency;
        self.metadata.unlink_latency = metadata_unlink_latency;
        self.metadata.rename_latency = metadata_rename_latency;
        self.metadata.readdir_latency = metadata_readdir_latency;
        self.metadata.fsync_latency = metadata_fsync_latency;
        
        // Set lock latency if present
        self.lock_latency = lock_latency;
        
        // Set test duration
        if snapshot.test_duration_ns > 0 {
            self.test_duration = Some(std::time::Duration::from_nanos(snapshot.test_duration_ns));
        }
        
        // Set coverage data (unique_blocks)
        if snapshot.unique_blocks > 0 {
            if let Some(ref unique_blocks_set) = self.unique_blocks {
                if let Ok(mut set) = unique_blocks_set.lock() {
                    // We can't reconstruct the exact set, but we can set the count
                    // This is sufficient for coverage_percent() calculation
                    // Note: This is a limitation - we lose the actual block numbers
                    set.clear();
                    for i in 0..snapshot.unique_blocks {
                        set.insert(i);
                    }
                }
            }
        }
        
        // Set resource stats by creating synthetic stats in the tracker
        if let Ok(mut tracker) = self.resource_tracker.lock() {
            tracker.set_synthetic_stats(snapshot.cpu_percent, snapshot.memory_bytes, snapshot.peak_memory_bytes);
        }
        
        Ok(())
    }
}

impl Default for WorkerStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aligned_counter_size() {
        // Verify cache-line alignment
        assert_eq!(std::mem::size_of::<AlignedCounter>(), 64);
        assert_eq!(std::mem::align_of::<AlignedCounter>(), 64);
    }

    #[test]
    fn test_aligned_counter_operations() {
        let counter = AlignedCounter::new();
        assert_eq!(counter.get(), 0);

        counter.add(10);
        assert_eq!(counter.get(), 10);

        counter.add(5);
        assert_eq!(counter.get(), 15);

        counter.set(100);
        assert_eq!(counter.get(), 100);
    }

    #[test]
    fn test_worker_stats_new() {
        let stats = WorkerStats::new();
        assert_eq!(stats.read_ops(), 0);
        assert_eq!(stats.write_ops(), 0);
        assert_eq!(stats.read_bytes(), 0);
        assert_eq!(stats.write_bytes(), 0);
        assert_eq!(stats.errors(), 0);
        assert_eq!(stats.total_ops(), 0);
        assert_eq!(stats.total_bytes(), 0);
        assert!(stats.lock_latency().is_none());
    }

    #[test]
    fn test_worker_stats_with_lock_tracking() {
        let stats = WorkerStats::with_lock_tracking(true);
        assert!(stats.lock_latency().is_some());
    }

    #[test]
    fn test_record_read() {
        let mut stats = WorkerStats::new();
        stats.record_io(OperationType::Read, 4096, Duration::from_micros(100));

        assert_eq!(stats.read_ops(), 1);
        assert_eq!(stats.read_bytes(), 4096);
        assert_eq!(stats.write_ops(), 0);
        assert_eq!(stats.write_bytes(), 0);
        assert_eq!(stats.total_ops(), 1);
        assert_eq!(stats.total_bytes(), 4096);
    }

    #[test]
    fn test_record_write() {
        let mut stats = WorkerStats::new();
        stats.record_io(OperationType::Write, 8192, Duration::from_micros(150));

        assert_eq!(stats.write_ops(), 1);
        assert_eq!(stats.write_bytes(), 8192);
        assert_eq!(stats.read_ops(), 0);
        assert_eq!(stats.read_bytes(), 0);
        assert_eq!(stats.total_ops(), 1);
        assert_eq!(stats.total_bytes(), 8192);
    }

    #[test]
    fn test_record_mixed_operations() {
        let mut stats = WorkerStats::new();
        stats.record_io(OperationType::Read, 4096, Duration::from_micros(100));
        stats.record_io(OperationType::Write, 8192, Duration::from_micros(150));
        stats.record_io(OperationType::Read, 2048, Duration::from_micros(80));

        assert_eq!(stats.read_ops(), 2);
        assert_eq!(stats.write_ops(), 1);
        assert_eq!(stats.read_bytes(), 6144);
        assert_eq!(stats.write_bytes(), 8192);
        assert_eq!(stats.total_ops(), 3);
        assert_eq!(stats.total_bytes(), 14336);
    }

    #[test]
    fn test_record_fsync() {
        let mut stats = WorkerStats::new();
        stats.record_io(OperationType::Fsync, 0, Duration::from_micros(200));

        assert_eq!(stats.read_ops(), 0);
        assert_eq!(stats.write_ops(), 0);
        assert_eq!(stats.metadata.fsync_ops.get(), 1);
    }

    #[test]
    fn test_record_error() {
        let mut stats = WorkerStats::new();
        stats.record_error();
        stats.record_error();

        assert_eq!(stats.errors(), 2);
    }

    #[test]
    fn test_record_lock_latency() {
        let mut stats = WorkerStats::with_lock_tracking(true);
        stats.record_lock_latency(Duration::from_micros(50));

        // Verify histogram has data
        let hist = stats.lock_latency().unwrap();
        assert!(hist.len() > 0);
    }

    #[test]
    fn test_record_lock_latency_disabled() {
        let mut stats = WorkerStats::new();
        // Should not panic when lock tracking is disabled
        stats.record_lock_latency(Duration::from_micros(50));
    }

    #[test]
    fn test_merge_worker_stats() {
        let mut stats1 = WorkerStats::new();
        stats1.record_io(OperationType::Read, 4096, Duration::from_micros(100));
        stats1.record_io(OperationType::Write, 8192, Duration::from_micros(150));

        let mut stats2 = WorkerStats::new();
        stats2.record_io(OperationType::Read, 2048, Duration::from_micros(80));
        stats2.record_io(OperationType::Write, 4096, Duration::from_micros(120));
        stats2.record_error();

        stats1.merge(&stats2).unwrap();

        assert_eq!(stats1.read_ops(), 2);
        assert_eq!(stats1.write_ops(), 2);
        assert_eq!(stats1.read_bytes(), 6144);
        assert_eq!(stats1.write_bytes(), 12288);
        assert_eq!(stats1.errors(), 1);
        assert_eq!(stats1.total_ops(), 4);
        assert_eq!(stats1.total_bytes(), 18432);
    }

    #[test]
    fn test_metadata_stats_new() {
        let stats = MetadataStats::new();
        assert_eq!(stats.total_ops(), 0);
        assert_eq!(stats.open_ops.get(), 0);
        assert_eq!(stats.close_ops.get(), 0);
    }

    #[test]
    fn test_metadata_stats_counters() {
        let stats = MetadataStats::new();
        stats.open_ops.add(5);
        stats.close_ops.add(3);
        stats.mkdir_ops.add(2);

        assert_eq!(stats.open_ops.get(), 5);
        assert_eq!(stats.close_ops.get(), 3);
        assert_eq!(stats.mkdir_ops.get(), 2);
        assert_eq!(stats.total_ops(), 10);
    }

    #[test]
    fn test_merge_metadata_stats() {
        let mut stats1 = MetadataStats::new();
        stats1.open_ops.add(5);
        stats1.close_ops.add(3);

        let stats2 = MetadataStats::new();
        stats2.open_ops.add(2);
        stats2.mkdir_ops.add(4);

        stats1.merge(&stats2).unwrap();

        assert_eq!(stats1.open_ops.get(), 7);
        assert_eq!(stats1.close_ops.get(), 3);
        assert_eq!(stats1.mkdir_ops.get(), 4);
        assert_eq!(stats1.total_ops(), 14);
    }
}
