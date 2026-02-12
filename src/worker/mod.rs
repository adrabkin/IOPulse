//! Worker thread implementation
//!
//! This module implements the Worker, which is the core execution unit that performs
//! IO operations. Each worker thread runs independently, executing IO operations
//! according to the configured workload and recording statistics.
//!
//! # Architecture
//!
//! The Worker orchestrates all the subsystems:
//! - **IOEngine**: Submits and polls IO operations
//! - **Target**: Provides file descriptors and manages locks
//! - **Distribution**: Generates random offsets
//! - **BufferPool**: Manages aligned IO buffers
//! - **WorkerStats**: Records operation statistics
//!
//! # Example
//!
//! ```no_run
//! use iopulse::worker::Worker;
//! use iopulse::config::{Config, WorkloadConfig};
//! use iopulse::config::workload::CompletionMode;
//! use std::sync::Arc;
//!
//! // Create configuration
//! let config = Arc::new(Config {
//!     // ... configuration fields
//! #   workload: WorkloadConfig {
//! #       read_percent: 100,
//! #       write_percent: 0,
//! #       read_distribution: vec![],
//! #       write_distribution: vec![],
//! #       queue_depth: 32,
//! #       completion_mode: CompletionMode::Duration { seconds: 10 },
//! #       distribution: iopulse::config::workload::DistributionType::Uniform,
//! #       think_time: None,
//! #       engine: iopulse::config::workload::EngineType::Sync,
//! #       direct: false,
//! #       sync: false,
//! #   },
//! #   targets: vec![],
//! #   workers: Default::default(),
//! #   output: Default::default(),
//! #   runtime: Default::default(),
//! });
//!
//! // Create and run worker
//! let mut worker = Worker::new(0, config)?;
//! let stats = worker.run()?;
//!
//! println!("Completed {} operations", stats.total_ops());
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod executor;
pub mod affinity;

use crate::config::{Config, WorkloadConfig, TargetType, workload::*};
use crate::distribution::{
    Distribution,
    uniform::UniformDistribution,
    zipf::ZipfDistribution,
    pareto::ParetoDistribution,
    gaussian::GaussianDistribution,
};
use crate::engine::{IOEngine, IOOperation, OperationType, EngineConfig};
use crate::stats::WorkerStats;
use crate::target::{Target, FileLockMode as TargetFileLockMode};
use crate::util::buffer::BufferPool;
use crate::util::fast_time::FastInstant;
use crate::Result;
use anyhow::Context;
use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Metadata for an in-flight IO operation
///
/// This structure tracks information about operations that have been submitted
/// to the IO engine but haven't completed yet. This is essential for async engines
/// to allow multiple operations to be in-flight simultaneously.
#[derive(Debug)]
#[allow(dead_code)] // Some fields used for debugging/future enhancements
struct InFlightOp {
    /// Buffer index in the buffer pool
    buf_idx: usize,
    /// Type of operation (Read, Write, etc.)
    op_type: OperationType,
    /// File offset for the operation
    offset: u64,
    /// Length of the operation in bytes
    length: usize,
    /// Start time for latency calculation
    start_time: FastInstant,
}

/// Worker thread that executes IO operations
///
/// The Worker is the core execution unit in IOPulse. It orchestrates all subsystems
/// to perform IO operations according to the configured workload, recording detailed
/// statistics about performance.
///
/// # Lifecycle
///
/// 1. **Creation**: `Worker::new()` initializes all subsystems
/// 2. **Execution**: `run()` performs the main IO loop
/// 3. **Completion**: Returns `WorkerStats` with collected statistics
///
/// # Thread Safety
///
/// Each worker owns its subsystems and is designed to run in its own thread.
/// Workers do not share mutable state and communicate only through returned statistics.
pub struct Worker {
    /// Worker ID (for identification in multi-worker scenarios)
    id: usize,
    
    /// Shared configuration
    config: Arc<Config>,
    
    /// IO engine for submitting operations
    engine: Box<dyn IOEngine>,
    
    /// Target files/devices
    targets: Vec<Box<dyn Target>>,
    
    /// Statistics collector
    stats: WorkerStats,
    
    /// Random distribution for offset generation
    distribution: Box<dyn Distribution>,
    
    /// Buffer pool for IO operations
    buffer_pool: BufferPool,
    
    /// Random number generator for operation selection
    rng: Xoshiro256PlusPlus,
    
    /// Start time for duration-based completion
    start_time: Option<Instant>,
    
    /// Total bytes transferred (for byte-based completion)
    total_bytes_transferred: u64,
    
    /// Operation counter (for think time application)
    operation_count: usize,
    
    /// Cached target file descriptor (avoid trait call overhead)
    cached_target_fd: i32,
    
    /// File list for directory layout testing (if using layout_manifest or layout_config)
    file_list: Option<Arc<Vec<std::path::PathBuf>>>,
    
    /// File range for PARTITIONED mode (start_index, end_index)
    file_range: Option<(usize, usize)>,
    
    /// Current file index for sequential file access
    current_file_index: usize,
    
    /// Currently open file (for file list mode)
    current_file: Option<Box<dyn Target>>,
    
    /// Current file descriptor (for file list mode)
    current_file_fd: i32,
    
    /// Current file size (for file list mode)
    current_file_size: u64,
    
    /// Cached target size (avoid trait call overhead)
    cached_target_size: u64,
    
    /// Shared statistics snapshots for live updates (optional)
    shared_snapshots: Option<Arc<Mutex<Vec<StatsSnapshot>>>>,
}

/// Lightweight statistics snapshot for live updates
///
/// This structure is updated by workers every 1K operations and shared with
/// the monitoring thread for live statistics display and JSON/CSV time-series output.
/// 
/// Includes metadata operation counters and histograms for complete per-second
/// storage behavior analysis. Now also includes separate read/write histograms.
/// 
/// Total size: ~11 KB (10 metadata + 2 IO histograms)
/// Cost: <0.01% overhead (verified negligible)
#[derive(Clone, Debug)]
pub struct StatsSnapshot {
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub errors: u64,
    pub avg_latency_us: f64,  // Overall average (for backward compatibility)
    
    // Separate read/write latency histograms (for detailed analysis)
    pub read_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub write_latency: crate::stats::simple_histogram::SimpleHistogram,
    
    // Metadata operation counters
    pub metadata_open_ops: u64,
    pub metadata_close_ops: u64,
    pub metadata_stat_ops: u64,
    pub metadata_setattr_ops: u64,
    pub metadata_mkdir_ops: u64,
    pub metadata_rmdir_ops: u64,
    pub metadata_unlink_ops: u64,
    pub metadata_rename_ops: u64,
    pub metadata_readdir_ops: u64,
    pub metadata_fsync_ops: u64,
    
    // Metadata operation latency histograms (for time-series analysis)
    pub metadata_open_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub metadata_close_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub metadata_stat_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub metadata_setattr_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub metadata_mkdir_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub metadata_rmdir_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub metadata_unlink_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub metadata_rename_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub metadata_readdir_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub metadata_fsync_latency: crate::stats::simple_histogram::SimpleHistogram,
}

impl Worker {
    /// Create a new worker
    ///
    /// # Arguments
    ///
    /// * `id` - Worker ID for identification
    /// * `config` - Shared configuration
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails (e.g., cannot create engine,
    /// cannot open targets, invalid configuration).
    pub fn new(id: usize, config: Arc<Config>) -> Result<Self> {
        // Create IO engine based on configuration
        let engine = Self::create_engine(&config.workload)?;
        
        // Create distribution based on configuration
        let distribution = Self::create_distribution(&config.workload)?;
        
        // Create buffer pool (size = queue_depth * 2 for safety)
        let buffer_size = if config.workload.read_distribution.is_empty() && config.workload.write_distribution.is_empty() {
            config.workload.block_size as usize // Use configured block size
        } else {
            // Use the largest block size from distributions
            let max_read = config.workload.read_distribution.iter()
                .map(|p| p.block_size)
                .max()
                .unwrap_or(config.workload.block_size);
            let max_write = config.workload.write_distribution.iter()
                .map(|p| p.block_size)
                .max()
                .unwrap_or(config.workload.block_size);
            max_read.max(max_write) as usize
        };
        
        let pool_size = config.workload.queue_depth * 2;
        let alignment = if config.workload.direct { 4096 } else { 512 };
        let mut buffer_pool = BufferPool::new(pool_size, buffer_size, alignment);
        
        // Pre-fill buffers with random data if using random write pattern
        if config.workload.write_pattern == VerifyPattern::Random && !config.runtime.verify {
            buffer_pool.prefill_random();
        }
        
        // Determine if lock tracking is needed
        let track_locks = config.targets.iter().any(|t| t.lock_mode != FileLockMode::None);
        let enable_heatmap = config.workload.heatmap;
        let stats = WorkerStats::with_heatmap(track_locks, enable_heatmap);
        
        Ok(Self {
            id,
            config,
            engine,
            targets: Vec::new(),
            stats,
            distribution,
            buffer_pool,
            rng: Xoshiro256PlusPlus::from_entropy(),
            start_time: None,
            total_bytes_transferred: 0,
            operation_count: 0,
            cached_target_fd: -1,  // Will be set after targets are opened
            cached_target_size: 0,  // Will be set after targets are opened
            shared_snapshots: None,  // Will be set by set_shared_stats() if needed
            file_list: None,  // Will be set by set_file_list() if needed
            file_range: None,  // Will be set by set_file_range() for PARTITIONED mode
            current_file_index: 0,
            current_file: None,
            current_file_fd: -1,
            current_file_size: 0,
        })
    }
    
    /// Set file list for directory layout testing
    ///
    /// This allows the worker to iterate through a list of files instead of
    /// using a single target file.
    ///
    /// # Arguments
    ///
    /// * `file_list` - Shared file list
    pub fn set_file_list(&mut self, file_list: Arc<Vec<std::path::PathBuf>>) {
        self.file_list = Some(file_list);
    }
    
    /// Set file range for PARTITIONED mode
    ///
    /// In PARTITIONED mode, each worker is assigned a range of files to access.
    ///
    /// # Arguments
    ///
    /// * `start` - Start index (inclusive)
    /// * `end` - End index (exclusive)
    pub fn set_file_range(&mut self, start: usize, end: usize) {
        self.file_range = Some((start, end));
        self.current_file_index = start;
    }
    
    /// Set shared statistics snapshots for live updates
    ///
    /// This allows the coordinator to read worker statistics during execution
    /// for live statistics display.
    ///
    /// # Arguments
    ///
    /// * `shared` - Shared statistics snapshot vector
    pub fn set_shared_stats(&mut self, shared: Arc<Mutex<Vec<StatsSnapshot>>>) {
        self.shared_snapshots = Some(shared);
    }
    
    /// Create IO engine based on configuration
    fn create_engine(workload: &WorkloadConfig) -> Result<Box<dyn IOEngine>> {
        use crate::engine::sync::SyncEngine;
        
        #[cfg(feature = "io_uring")]
        use crate::engine::io_uring::IoUringEngine;
        
        #[cfg(target_os = "linux")]
        use crate::engine::libaio::LibaioEngine;
        
        use crate::engine::mmap::MmapEngine;
        use std::sync::atomic::{AtomicBool, Ordering};
        
        // Smart engine selection: use sync for QD=1, async for QD>1
        // This avoids async overhead for single-depth queues
        let effective_engine = if workload.queue_depth == 1 {
            match workload.engine {
                EngineType::Libaio | EngineType::IoUring => {
                    // Only print message once across all workers
                    static SMART_SELECTION_NOTIFIED: AtomicBool = AtomicBool::new(false);
                    if !SMART_SELECTION_NOTIFIED.swap(true, Ordering::Relaxed) {
                        eprintln!("Note: Using sync engine for queue depth 1 (more efficient than async engines)");
                    }
                    EngineType::Sync
                }
                _ => workload.engine,
            }
        } else {
            workload.engine
        };
        
        let engine: Box<dyn IOEngine> = match effective_engine {
            EngineType::Sync => Box::new(SyncEngine::new()),
            
            #[cfg(feature = "io_uring")]
            EngineType::IoUring => Box::new(IoUringEngine::new()),
            
            #[cfg(not(feature = "io_uring"))]
            EngineType::IoUring => {
                anyhow::bail!("io_uring engine not available (feature not enabled)")
            }
            
            #[cfg(target_os = "linux")]
            EngineType::Libaio => Box::new(LibaioEngine::new()),
            
            #[cfg(not(target_os = "linux"))]
            EngineType::Libaio => {
                anyhow::bail!("libaio engine only available on Linux")
            }
            
            EngineType::Mmap => Box::new(MmapEngine::new()),
        };
        
        Ok(engine)
    }
    
    /// Create distribution based on configuration
    fn create_distribution(workload: &WorkloadConfig) -> Result<Box<dyn Distribution>> {
        // If not random, use sequential distribution
        if !workload.random {
            return Ok(Box::new(crate::distribution::sequential::SequentialDistribution::new()));
        }
        
        // Otherwise use configured random distribution
        let dist: Box<dyn Distribution> = match &workload.distribution {
            DistributionType::Uniform => {
                Box::new(UniformDistribution::new())
            }
            DistributionType::Zipf { theta } => {
                Box::new(ZipfDistribution::new(*theta))
            }
            DistributionType::Pareto { h } => {
                Box::new(ParetoDistribution::new(*h))
            }
            DistributionType::Gaussian { stddev, center } => {
                Box::new(GaussianDistribution::new(*stddev, *center))
            }
        };
        
        Ok(dist)
    }
    
    /// Main execution loop
    ///
    /// Runs the worker until the completion criterion is met. Records statistics
    /// for all operations and returns the final statistics.
    ///
    /// # Returns
    ///
    /// Returns `WorkerStats` containing all collected statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Engine initialization fails
    /// - Target opening fails
    /// - IO operation fails (unless continue-on-error is enabled)
    /// - Engine cleanup fails
    pub fn run(&mut self) -> Result<WorkerStats> {
        // Apply CPU/NUMA affinity if configured
        self.apply_affinity()
            .context("Failed to apply CPU/NUMA affinity")?;
        
        // Initialize engine
        let engine_config = self.config.workload.to_engine_config();
        self.engine.init(&engine_config)
            .context("Failed to initialize IO engine")?;
        
        // Open targets
        self.open_targets()
            .context("Failed to open targets")?;
        
        // Verify we have targets or file list
        if self.targets.is_empty() && self.file_list.is_none() {
            anyhow::bail!("No targets or file list available for IO operations");
        }
        
        // Record start time
        self.start_time = Some(Instant::now());
        
        // Start resource tracking
        self.stats.start_resource_tracking();
        
        // Main execution loop - ASYNC-AWARE
        // This loop allows multiple operations to be in-flight simultaneously for async engines
        let queue_depth = self.config.workload.queue_depth;
        let mut in_flight_ops: Vec<InFlightOp> = Vec::with_capacity(queue_depth);
        
        // Check duration every N operations to reduce clock_gettime overhead
        // At high IOPS (>100K), check every 100 ops (~1ms)
        const DURATION_CHECK_INTERVAL: usize = 100;
        let mut ops_since_duration_check = 0;
        
        // Sample resources every N operations to reduce overhead
        // Sample every ~10K operations or ~100ms at 100K IOPS
        const RESOURCE_SAMPLE_INTERVAL: usize = 10000;
        let mut ops_since_resource_sample = 0;
        
        // Update live stats frequency depends on engine performance characteristics
        // High-IOPS scenarios (>500K IOPS): Update every 1000 ops to minimize overhead
        // Low-IOPS scenarios (<500K IOPS): Update every 1 op for perfect precision
        //
        // High-IOPS scenarios:
        // - mmap engine (always fast: 1-3M IOPS)
        // - Buffered IO with any engine (page cache: 500K-2M IOPS)
        //
        // Low-IOPS scenarios:
        // - O_DIRECT with sync/io_uring/libaio (<100K IOPS typically)
        let live_stats_update_interval = if matches!(self.config.workload.engine, crate::config::workload::EngineType::Mmap) || !self.config.workload.direct {
            1000  // High-IOPS: mmap or buffered
        } else {
            1  // Low-IOPS: O_DIRECT with other engines
        };
        
        let mut ops_since_live_update = 0;
        
        loop {
            // Phase 1: Fill the queue up to queue_depth
            while in_flight_ops.len() < queue_depth && !self.should_stop() {
                // Select operation type (read or write)
                let op_type = self.select_operation_type();
                
                // Prepare and submit operation (no polling yet)
                match self.prepare_and_submit_operation(op_type) {
                    Ok(in_flight_op) => {
                        in_flight_ops.push(in_flight_op);
                        
                        // Sample queue depth after each submit (for accurate tracking)
                        self.stats.sample_queue_depth(in_flight_ops.len() as u64);
                    }
                    Err(e) => {
                        if self.config.runtime.continue_on_error {
                            // Log error and continue
                            eprintln!("Worker {}: IO error: {}", self.id, e);
                            
                            // Check max errors threshold
                            if let Some(max) = self.config.runtime.max_errors {
                                if self.stats.errors() >= max as u64 {
                                    anyhow::bail!("Maximum error threshold ({}) exceeded", max);
                                }
                            }
                        } else {
                            // Abort on error (default behavior)
                            return Err(e).context("IO operation failed");
                        }
                    }
                }
            }
            
            // Phase 2: Poll for completions (only when queue is full or stopping)
            if !in_flight_ops.is_empty() {
                if let Err(e) = self.process_completions(&mut in_flight_ops) {
                    if self.config.runtime.continue_on_error {
                        eprintln!("Worker {}: Completion error: {}", self.id, e);
                        
                        // Check max errors threshold
                        if let Some(max) = self.config.runtime.max_errors {
                            if self.stats.errors() >= max as u64 {
                                anyhow::bail!("Maximum error threshold ({}) exceeded", max);
                            }
                        }
                    } else {
                        return Err(e).context("Completion processing failed");
                    }
                }
            }
            
            // Phase 3: Check duration periodically
            ops_since_duration_check += 1;
            if ops_since_duration_check >= DURATION_CHECK_INTERVAL {
                if self.should_stop() && in_flight_ops.is_empty() {
                    if self.config.runtime.debug {
                        eprintln!("DEBUG: should_stop returned true at {} ops, elapsed={:.3}s", 
                            self.operation_count, 
                            self.start_time.unwrap().elapsed().as_secs_f64());
                    }
                    break;
                }
                ops_since_duration_check = 0;
            }
            
            // Phase 4: Sample resources periodically
            ops_since_resource_sample += 1;
            if ops_since_resource_sample >= RESOURCE_SAMPLE_INTERVAL {
                self.stats.sample_resources();
                ops_since_resource_sample = 0;
            }
            
            // Phase 5: Update live stats snapshot periodically
            ops_since_live_update += 1;
            if ops_since_live_update >= live_stats_update_interval {
                // Sample queue depth for async engines (always, not just when shared_snapshots is set)
                self.stats.sample_queue_depth(in_flight_ops.len() as u64);
                
                if let Some(ref shared) = self.shared_snapshots {
                    let avg_latency_us = self.stats.io_latency().mean().as_micros() as f64;
                    
                    if let Ok(mut snapshots) = shared.lock() {
                        snapshots[self.id] = StatsSnapshot {
                            read_ops: self.stats.read_ops(),
                            write_ops: self.stats.write_ops(),
                            read_bytes: self.stats.read_bytes(),
                            write_bytes: self.stats.write_bytes(),
                            errors: self.stats.errors(),
                            avg_latency_us,
                            // Separate read/write latency histograms (for detailed analysis)
                            read_latency: self.stats.read_latency().clone(),
                            write_latency: self.stats.write_latency().clone(),
                            // Metadata operation counters (just atomic reads, very fast)
                            metadata_open_ops: self.stats.metadata.open_ops.get(),
                            metadata_close_ops: self.stats.metadata.close_ops.get(),
                            metadata_stat_ops: self.stats.metadata.stat_ops.get(),
                            metadata_setattr_ops: self.stats.metadata.setattr_ops.get(),
                            metadata_mkdir_ops: self.stats.metadata.mkdir_ops.get(),
                            metadata_rmdir_ops: self.stats.metadata.rmdir_ops.get(),
                            metadata_unlink_ops: self.stats.metadata.unlink_ops.get(),
                            metadata_rename_ops: self.stats.metadata.rename_ops.get(),
                            metadata_readdir_ops: self.stats.metadata.readdir_ops.get(),
                            metadata_fsync_ops: self.stats.metadata.fsync_ops.get(),
                            // Metadata latency histograms (clone for time-series analysis)
                            // Cost: ~9 KB memcpy every 1K ops = <0.01% overhead
                            metadata_open_latency: self.stats.metadata.open_latency.clone(),
                            metadata_close_latency: self.stats.metadata.close_latency.clone(),
                            metadata_stat_latency: self.stats.metadata.stat_latency.clone(),
                            metadata_setattr_latency: self.stats.metadata.setattr_latency.clone(),
                            metadata_mkdir_latency: self.stats.metadata.mkdir_latency.clone(),
                            metadata_rmdir_latency: self.stats.metadata.rmdir_latency.clone(),
                            metadata_unlink_latency: self.stats.metadata.unlink_latency.clone(),
                            metadata_rename_latency: self.stats.metadata.rename_latency.clone(),
                            metadata_readdir_latency: self.stats.metadata.readdir_latency.clone(),
                            metadata_fsync_latency: self.stats.metadata.fsync_latency.clone(),
                        };
                    }
                }
                ops_since_live_update = 0;
            }
            
            // Apply think time if configured
            if let Some(ref think_time) = self.config.workload.think_time {
                if self.operation_count % think_time.apply_every_n_blocks == 0 {
                    // Use a nominal latency for think time calculation
                    // In async mode, we don't have per-operation latency readily available
                    let nominal_latency = Duration::from_micros(100);
                    self.apply_think_time(think_time, nominal_latency);
                }
            }
        }
        
        // Drain any remaining in-flight operations
        while !in_flight_ops.is_empty() {
            self.process_completions(&mut in_flight_ops)?;
        }
        
        // Fsync targets BEFORE cleanup (if not using O_DIRECT)
        // NOTE: Disabled for performance - fsync not required by default
        // Uncomment if data durability testing is needed
        /*
        if !self.config.workload.direct {
            for target in &self.targets {
                let fsync_start = Instant::now();
                
                // Perform fsync via engine
                let op = IOOperation {
                    op_type: OperationType::Fsync,
                    target_fd: target.fd(),
                    offset: 0,
                    buffer: std::ptr::null_mut(),
                    length: 0,
                    user_data: 0,
                };
                
                self.engine.submit(op)
                    .context("Failed to submit fsync")?;
                let completions = self.engine.poll_completions()
                    .context("Failed to poll fsync completion")?;
                
                let fsync_latency = fsync_start.elapsed();
                
                // Record fsync in metadata stats
                for _completion in completions {
                    self.stats.metadata.fsync_ops.add(1);
                    self.stats.metadata.fsync_latency.record(fsync_latency);
                }
            }
        }
        */
        
        // Cleanup engine
        self.engine.cleanup()
            .context("Failed to cleanup IO engine")?;
        
        // Close targets (without fsync, already done above)
        self.close_targets()
            .context("Failed to close targets")?;
        
        // Take final resource sample
        self.stats.sample_resources();
        
        // Calculate actual test duration (excludes setup time like preallocation)
        let test_duration = if let Some(start) = self.start_time {
            start.elapsed()
        } else {
            Duration::from_secs(0)
        };
        
        // Set test duration in stats before returning
        self.stats.set_test_duration(test_duration);
        
        // Return statistics
        // Create a dummy stats to replace with (matching the original config)
        let track_locks = self.config.targets.iter().any(|t| t.lock_mode != FileLockMode::None);
        let enable_heatmap = self.config.workload.heatmap;
        let replacement_stats = WorkerStats::with_heatmap(track_locks, enable_heatmap);
        
        Ok(std::mem::replace(&mut self.stats, replacement_stats))
    }
    
    /// Run worker until stop flag is set (for distributed mode)
    ///
    /// Similar to run() but checks a stop flag instead of duration/bytes.
    /// Used by node service to allow coordinator to stop the test.
    pub fn run_until_stopped(&mut self, stop_flag: &std::sync::atomic::AtomicBool) -> Result<()> {
        use std::sync::atomic::Ordering;
        
        // Apply CPU/NUMA affinity if configured
        self.apply_affinity()
            .context("Failed to apply CPU/NUMA affinity")?;
        
        // Initialize engine
        let engine_config = self.config.workload.to_engine_config();
        self.engine.init(&engine_config)
            .context("Failed to initialize IO engine")?;
        
        // Open targets
        self.open_targets()
            .context("Failed to open targets")?;
        
        // Verify we have targets or file list
        if self.targets.is_empty() && self.file_list.is_none() {
            anyhow::bail!("No targets or file list available for IO operations");
        }
        
        // Record start time
        self.start_time = Some(Instant::now());
        
        // Start resource tracking
        self.stats.start_resource_tracking();
        
        // Main execution loop
        let queue_depth = self.config.workload.queue_depth;
        let mut in_flight_ops: Vec<InFlightOp> = Vec::with_capacity(queue_depth);
        
        // Track operations for live stats updates
        // High-IOPS (mmap or buffered): Every 1000 ops
        // Low-IOPS (O_DIRECT): Every 1 op for perfect precision
        let live_stats_update_interval = if matches!(self.config.workload.engine, crate::config::workload::EngineType::Mmap) || !self.config.workload.direct {
            1000
        } else {
            1
        };
        
        let mut ops_since_live_update = 0;
        
        loop {
            // Check stop flag
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            
            // Fill the queue
            while in_flight_ops.len() < queue_depth && !stop_flag.load(Ordering::Relaxed) {
                let op_type = self.select_operation_type();
                
                match self.prepare_and_submit_operation(op_type) {
                    Ok(in_flight_op) => {
                        in_flight_ops.push(in_flight_op);
                        self.stats.sample_queue_depth(in_flight_ops.len() as u64);
                        ops_since_live_update += 1;
                    }
                    Err(e) => {
                        if self.config.runtime.continue_on_error {
                            eprintln!("Worker {}: IO error: {}", self.id, e);
                        } else {
                            return Err(e).context("IO operation failed");
                        }
                    }
                }
            }
            
            // Poll for completions
            if !in_flight_ops.is_empty() {
                if let Err(e) = self.process_completions(&mut in_flight_ops) {
                    if !self.config.runtime.continue_on_error {
                        return Err(e).context("Completion processing failed");
                    }
                }
            }
            
            // Update shared snapshots periodically (every 1K ops)
            if ops_since_live_update >= live_stats_update_interval {
                self.stats.sample_queue_depth(in_flight_ops.len() as u64);
                
                if let Some(ref shared) = self.shared_snapshots {
                    let avg_latency_us = self.stats.io_latency().mean().as_micros() as f64;
                    
                    if let Ok(mut snapshots) = shared.lock() {
                        snapshots[self.id] = StatsSnapshot {
                            read_ops: self.stats.read_ops(),
                            write_ops: self.stats.write_ops(),
                            read_bytes: self.stats.read_bytes(),
                            write_bytes: self.stats.write_bytes(),
                            errors: self.stats.errors(),
                            avg_latency_us,
                            read_latency: self.stats.read_latency().clone(),
                            write_latency: self.stats.write_latency().clone(),
                            metadata_open_ops: self.stats.metadata.open_ops.get(),
                            metadata_close_ops: self.stats.metadata.close_ops.get(),
                            metadata_stat_ops: self.stats.metadata.stat_ops.get(),
                            metadata_setattr_ops: self.stats.metadata.setattr_ops.get(),
                            metadata_mkdir_ops: self.stats.metadata.mkdir_ops.get(),
                            metadata_rmdir_ops: self.stats.metadata.rmdir_ops.get(),
                            metadata_unlink_ops: self.stats.metadata.unlink_ops.get(),
                            metadata_rename_ops: self.stats.metadata.rename_ops.get(),
                            metadata_readdir_ops: self.stats.metadata.readdir_ops.get(),
                            metadata_fsync_ops: self.stats.metadata.fsync_ops.get(),
                            metadata_open_latency: self.stats.metadata.open_latency.clone(),
                            metadata_close_latency: self.stats.metadata.close_latency.clone(),
                            metadata_stat_latency: self.stats.metadata.stat_latency.clone(),
                            metadata_setattr_latency: self.stats.metadata.setattr_latency.clone(),
                            metadata_mkdir_latency: self.stats.metadata.mkdir_latency.clone(),
                            metadata_rmdir_latency: self.stats.metadata.rmdir_latency.clone(),
                            metadata_unlink_latency: self.stats.metadata.unlink_latency.clone(),
                            metadata_rename_latency: self.stats.metadata.rename_latency.clone(),
                            metadata_readdir_latency: self.stats.metadata.readdir_latency.clone(),
                            metadata_fsync_latency: self.stats.metadata.fsync_latency.clone(),
                        };
                    }
                }
                ops_since_live_update = 0;
            }
        }
        
        // Complete remaining in-flight operations
        while !in_flight_ops.is_empty() {
            self.process_completions(&mut in_flight_ops)?;
        }
        
        // Cleanup
        self.engine.cleanup()?;
        self.close_targets()?;
        self.stats.sample_resources();
        
        // Set test duration
        if let Some(start) = self.start_time {
            self.stats.set_test_duration(start.elapsed());
        }
        
        Ok(())
    }
    
    /// Consume worker and return statistics (for distributed mode)
    pub fn into_stats(self) -> crate::stats::WorkerStats {
        self.stats
    }
    
    /// Apply CPU and NUMA affinity if configured
    fn apply_affinity(&self) -> Result<()> {
        use crate::worker::affinity;
        
        // Apply CPU affinity if configured
        if let Some(ref cpu_spec) = self.config.workers.cpu_cores {
            let cores = affinity::parse_cpu_list(cpu_spec)
                .context("Failed to parse CPU core list")?;
            
            // For multi-worker scenarios, bind to specific core based on worker ID
            // For now, bind to all specified cores (coordinator will handle distribution)
            affinity::set_cpu_affinity(&cores)
                .context("Failed to set CPU affinity")?;
        }
        
        // Apply NUMA affinity if configured
        if let Some(ref numa_spec) = self.config.workers.numa_zones {
            let nodes = affinity::parse_numa_list(numa_spec)
                .context("Failed to parse NUMA node list")?;
            
            affinity::set_numa_affinity(&nodes)
                .context("Failed to set NUMA affinity")?;
        }
        
        Ok(())
    }
    
    /// Open all targets from configuration
    fn open_targets(&mut self) -> Result<()> {
        // If we have a file list, skip opening targets here
        // Files will be opened dynamically during execution
        if self.file_list.is_some() {
            return Ok(());
        }
        
        use crate::target::file::FileTarget;
        use crate::target::block::BlockTarget;
        use crate::target::{OpenFlags, FadviseFlags as TargetFadviseFlags};
        
        for target_config in &self.config.targets {
            let mut target: Box<dyn Target> = match target_config.target_type {
                TargetType::File => {
                    let mut file_target = FileTarget::new(
                        target_config.path.clone(),
                        target_config.file_size,
                    );
                    
                    // For O_DIRECT, we MUST preallocate to ensure file exists with proper size
                    // O_DIRECT requires the file to exist and have allocated blocks
                    //
                    // Exception: In distributed mode, nodes pre-allocate via PrepareFiles.
                    // The node service sets BOTH preallocate=false AND no_refill=true to indicate
                    // "already done, don't do it again". This combination only occurs in distributed mode.
                    //
                    // In standalone mode, preallocate defaults to false, but no_refill is also false,
                    // so we can distinguish: preallocate=false + no_refill=false = "not set, force for O_DIRECT"
                    let already_preallocated = !target_config.preallocate && target_config.no_refill;
                    let force_preallocate = self.config.workload.direct && 
                                           target_config.file_size.is_some() &&
                                           !already_preallocated;
                    
                    // Set preallocate and truncate options
                    file_target.set_preallocate(target_config.preallocate || force_preallocate);
                    file_target.set_truncate_to_size(target_config.truncate_to_size);
                    file_target.set_refill(target_config.refill);
                    file_target.set_refill_pattern(target_config.refill_pattern);
                    file_target.set_using_direct_io(self.config.workload.direct);
                    
                    // Set offset range for partitioned distribution
                    // This ensures refill only fills the worker's assigned region
                    if let Some((start, end)) = self.config.workers.offset_range {
                        file_target.set_offset_range(start, end);
                    }
                    
                    Box::new(file_target)
                }
                TargetType::BlockDevice => {
                    Box::new(BlockTarget::new(target_config.path.clone()))
                }
                TargetType::Directory => {
                    // Directory tree generation will be handled by coordinator
                    // For now, skip directory targets
                    continue;
                }
            };
            
            // Open the target
            // For read-only tests on non-existent files, we need to create and fill them
            // For write tests, create if needed
            // Check if file exists for read-only tests
            let file_exists = target_config.path.exists();
            let is_read_only = self.config.workload.write_percent == 0;
            let should_create = self.config.workload.write_percent > 0 || (is_read_only && !file_exists);
            
            let flags = OpenFlags {
                direct: self.config.workload.direct,
                sync: self.config.workload.sync,
                create: should_create,
                truncate: false,
            };
            
            let open_start = Instant::now();
            let open_result = target.open(flags);
            
            // Handle open failure
            if let Err(e) = open_result {
                return Err(e).with_context(|| format!("Failed to open target: {:?}", target_config.path));
            }
            
            let open_latency = open_start.elapsed();
            
            // Record open operation in metadata stats
            self.stats.metadata.open_ops.add(1);
            self.stats.metadata.open_latency.record(open_latency);
            
            // Apply fadvise hints if any are set
            let config_fadvise = &target_config.fadvise_flags;
            if config_fadvise.sequential
                || config_fadvise.random
                || config_fadvise.willneed
                || config_fadvise.dontneed
                || config_fadvise.noreuse
            {
                // Convert config FadviseFlags to target FadviseFlags
                let target_fadvise = TargetFadviseFlags {
                    sequential: config_fadvise.sequential,
                    random: config_fadvise.random,
                    willneed: config_fadvise.willneed,
                    dontneed: config_fadvise.dontneed,
                    noreuse: config_fadvise.noreuse,
                };
                
                target.apply_fadvise(&target_fadvise)
                    .context("Failed to apply fadvise hints")?;
            }
            
            self.targets.push(target);
        }
        
        // Smart auto-refill: If reads are requested and file is empty, auto-fill it
        // This prevents silent failures where reads from empty files return 0 bytes
        if !self.targets.is_empty() && self.config.workload.read_percent > 0 {
            let target_fd = self.targets[0].fd();
            
            // Check actual file size by reading metadata
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            let stat_result = unsafe { libc::fstat(target_fd, &mut stat) };
            
            let mut actual_file_size = if stat_result == 0 {
                stat.st_size as u64
            } else {
                0
            };
            
            // If actual file is empty but we're doing reads
            if actual_file_size == 0 {
                let target_path = &self.config.targets[0].path;
                let file_size = self.config.targets[0].file_size.unwrap_or(0);
                
                if self.config.targets[0].no_refill {
                    // User explicitly disabled auto-refill, error out
                    eprintln!("\nError: Cannot read from empty file (auto-refill disabled)");
                    eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    eprintln!("File: {}", target_path.display());
                    eprintln!("Size: 0 bytes (empty)");
                    eprintln!();
                    eprintln!("The file is empty but read operations were requested.");
                    eprintln!("Auto-refill is disabled (--no-refill flag).");
                    eprintln!();
                    eprintln!("Solution: Remove --no-refill flag to enable auto-fill, or:");
                    eprintln!("  # Step 1: Write data");
                    eprintln!("  ./iopulse {} --file-size {} --duration 1s --write-percent 100 --random",
                        target_path.display(), file_size);
                    eprintln!();
                    eprintln!("  # Step 2: Read data");
                    eprintln!("  ./iopulse {} --file-size {} --duration 1s --read-percent 100 --random",
                        target_path.display(), file_size);
                    eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    
                    anyhow::bail!("Empty file with read operations requested (auto-refill disabled)");
                } else {
                    // Auto-refill the file
                    eprintln!("\n📝 File is empty. Filling with {} data...", 
                        match self.config.targets[0].refill_pattern {
                            crate::config::workload::VerifyPattern::Random => "random",
                            crate::config::workload::VerifyPattern::Zeros => "zero",
                            crate::config::workload::VerifyPattern::Ones => "one",
                            crate::config::workload::VerifyPattern::Sequential => "sequential",
                        });
                    eprintln!("   File: {}", target_path.display());
                    eprintln!("   Size: {} bytes", file_size);
                    
                    let refill_start = Instant::now();
                    
                    // Get mutable reference to target for refill
                    // We need to downcast to FileTarget to call force_refill
                    if let Some(file_target) = self.targets[0].as_any_mut().downcast_mut::<crate::target::file::FileTarget>() {
                        file_target.force_refill(self.config.targets[0].refill_pattern)
                            .context("Failed to auto-refill empty file")?;
                    } else {
                        anyhow::bail!("Auto-refill only supported for file targets");
                    }
                    
                    let refill_duration = refill_start.elapsed();
                    eprintln!("   ✅ File filled in {:.2}s", refill_duration.as_secs_f64());
                    eprintln!();
                    
                    // Re-check file size after refill
                    let mut stat_after: libc::stat = unsafe { std::mem::zeroed() };
                    let stat_result_after = unsafe { libc::fstat(target_fd, &mut stat_after) };
                    actual_file_size = if stat_result_after == 0 {
                        stat_after.st_size as u64
                    } else {
                        0
                    };
                }
            }
            
            // Check if file is too small compared to configured size
            // Require file to be at least 90% of configured size to avoid reading same data repeatedly
            let configured_size = self.config.targets[0].file_size.unwrap_or(actual_file_size);
            if configured_size > 0 && actual_file_size < (configured_size * 9) / 10 {
                let target_path = &self.config.targets[0].path;
                let percent = (actual_file_size * 100) / configured_size;
                
                eprintln!("\nError: File is too small for read test");
                eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                eprintln!("File: {}", target_path.display());
                eprintln!("Actual size: {} bytes", actual_file_size);
                eprintln!("Configured size: {} bytes", configured_size);
                eprintln!("File is only {}% of configured size", percent);
                eprintln!();
                eprintln!("The file is too small for the configured test size.");
                eprintln!("This would result in invalid profiling results (reading same data repeatedly).");
                eprintln!();
                eprintln!("Solution: Create and fill the file at the correct size:");
                eprintln!("  # Step 1: Write data at correct size");
                eprintln!("  ./iopulse {} --file-size {} --duration 1s --write-percent 100 --random",
                    target_path.display(), configured_size);
                eprintln!();
                eprintln!("  # Step 2: Read data");
                eprintln!("  ./iopulse {} --file-size {} --duration 1s --read-percent 100 --random",
                    target_path.display(), configured_size);
                eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                
                anyhow::bail!("File too small for read test");
            }
        }
        
        // mmap engine auto-fill: mmap requires non-zero file size (POSIX limitation)
        // Auto-fill empty files to make mmap work seamlessly with all workloads
        if !self.targets.is_empty() && self.config.workload.engine == crate::config::workload::EngineType::Mmap {
            let target_fd = self.targets[0].fd();
            
            // Check actual file size
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            let stat_result = unsafe { libc::fstat(target_fd, &mut stat) };
            
            let actual_file_size = if stat_result == 0 {
                stat.st_size as u64
            } else {
                0
            };
            
            // If file is empty, mmap will fail (cannot map size 0)
            if actual_file_size == 0 {
                let target_path = &self.config.targets[0].path;
                let file_size = self.config.targets[0].file_size.unwrap_or(0);
                
                if self.config.targets[0].no_refill {
                    // User explicitly disabled auto-refill, error out with helpful message
                    eprintln!("\nError: mmap engine requires non-zero file size (auto-refill disabled)");
                    eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    eprintln!("File: {}", target_path.display());
                    eprintln!("Size: 0 bytes (empty)");
                    eprintln!();
                    eprintln!("The mmap engine cannot memory-map empty files (POSIX limitation).");
                    eprintln!("Auto-refill is disabled (--no-refill flag).");
                    eprintln!();
                    eprintln!("Solutions:");
                    eprintln!("  1. Remove --no-refill flag to enable auto-fill");
                    eprintln!("  2. Use --preallocate to allocate space");
                    eprintln!("  3. Use a different engine (sync, io_uring, libaio)");
                    eprintln!("  4. Pre-fill manually:");
                    eprintln!("     ./iopulse {} --file-size {} --duration 1s --write-percent 100 --engine sync",
                        target_path.display(), file_size);
                    eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    
                    anyhow::bail!("mmap engine requires non-zero file size (auto-refill disabled)");
                } else {
                    // Auto-fill the file for mmap
                    eprintln!("\n📝 mmap engine requires non-zero file size. Filling with {} data...", 
                        match self.config.targets[0].refill_pattern {
                            crate::config::workload::VerifyPattern::Random => "random",
                            crate::config::workload::VerifyPattern::Zeros => "zero",
                            crate::config::workload::VerifyPattern::Ones => "one",
                            crate::config::workload::VerifyPattern::Sequential => "sequential",
                        });
                    eprintln!("   File: {}", target_path.display());
                    eprintln!("   Size: {} bytes", file_size);
                    eprintln!("   Reason: mmap cannot map empty files (POSIX limitation)");
                    
                    let refill_start = Instant::now();
                    
                    // Get mutable reference to target for refill
                    if let Some(file_target) = self.targets[0].as_any_mut().downcast_mut::<crate::target::file::FileTarget>() {
                        file_target.force_refill(self.config.targets[0].refill_pattern)
                            .context("Failed to auto-refill empty file for mmap engine")?;
                    } else {
                        anyhow::bail!("mmap engine auto-refill only supported for file targets");
                    }
                    
                    let refill_duration = refill_start.elapsed();
                    eprintln!("   ✅ File ready for mmap in {:.2}s", refill_duration.as_secs_f64());
                    eprintln!();
                }
            }
        }
        
        // Cache target fd and size to avoid trait call overhead in hot path
        if !self.targets.is_empty() {
            self.cached_target_fd = self.targets[0].fd();
            self.cached_target_size = self.targets[0].size();
        }
        
        Ok(())
    }
    
    /// Close all targets
    fn close_targets(&mut self) -> Result<()> {
        // Note: fsync is now done BEFORE cleanup() in run(), not here
        
        // Close all targets
        for target in &mut self.targets {
            let close_start = Instant::now();
            target.close()
                .context("Failed to close target")?;
            let close_latency = close_start.elapsed();
            
            // Record close operation in metadata stats
            self.stats.metadata.close_ops.add(1);
            self.stats.metadata.close_latency.record(close_latency);
        }
        
        Ok(())
    }
    
    /// Check if worker should stop based on completion criteria
    fn should_stop(&self) -> bool {
        match &self.config.workload.completion_mode {
            CompletionMode::Duration { seconds } => {
                if let Some(start) = self.start_time {
                    let elapsed = start.elapsed();
                    let should_stop = elapsed >= Duration::from_secs(*seconds);
                    if self.config.runtime.debug && self.operation_count % 10000 == 0 {
                        eprintln!("DEBUG should_stop: Duration mode, elapsed={:.3}s, target={}s, should_stop={}", 
                            elapsed.as_secs_f64(), seconds, should_stop);
                    }
                    should_stop
                } else {
                    false
                }
            }
            CompletionMode::TotalBytes { bytes } => {
                self.total_bytes_transferred >= *bytes
            }
            CompletionMode::RunUntilComplete => {
                // For file list mode, stop when we've processed all files in our range
                if let Some(file_list) = &self.file_list {
                    if let Some((start, end)) = self.file_range {
                        // PARTITIONED mode: stop when we've processed all files in range
                        let files_to_process = end - start;
                        let files_processed = self.operation_count;
                        let should_stop = files_processed >= files_to_process;
                        if self.config.runtime.debug && self.operation_count % 1000 == 0 {
                            eprintln!("DEBUG should_stop: RunUntilComplete (file list PARTITIONED), processed={}, target={}, should_stop={}", 
                                files_processed, files_to_process, should_stop);
                        }
                        return should_stop;
                    } else {
                        // SHARED mode with file list: stop after processing all files once
                        let files_to_process = file_list.len();
                        let files_processed = self.operation_count;
                        let should_stop = files_processed >= files_to_process;
                        if self.config.runtime.debug && self.operation_count % 1000 == 0 {
                            eprintln!("DEBUG should_stop: RunUntilComplete (file list SHARED), processed={}, target={}, should_stop={}", 
                                files_processed, files_to_process, should_stop);
                        }
                        return should_stop;
                    }
                }
                
                // Original logic for single file mode
                // Stop when we've written the target amount
                // For partitioned distribution, target is the worker's region size
                // For shared/per-worker, target is the full file size
                let target_size = if let Some((start, end)) = self.config.workers.offset_range {
                    // Partitioned mode: worker's region size
                    end - start
                } else if let Some(file_size) = self.config.targets.first().and_then(|t| t.file_size) {
                    // Shared/per-worker mode: full file size
                    file_size
                } else {
                    return false; // No target size, run forever
                };
                
                let should_stop = self.total_bytes_transferred >= target_size;
                if self.config.runtime.debug && self.operation_count % 10000 == 0 {
                    eprintln!("DEBUG should_stop: RunUntilComplete, transferred={}, target_size={}, should_stop={}", 
                        self.total_bytes_transferred, target_size, should_stop);
                }
                should_stop
            }
        }
    }
    
    /// Select operation type based on read/write percentages
    #[inline(always)]
    fn select_operation_type(&mut self) -> OperationType {
        let roll = self.rng.gen_range(0..100);
        if roll < self.config.workload.read_percent {
            OperationType::Read
        } else {
            OperationType::Write
        }
    }
    
    /// Select next file from file list (for directory layout testing)
    ///
    /// Returns the file index to use for the next operation.
    /// In PARTITIONED mode, iterates through assigned file range sequentially.
    /// In SHARED mode, selects randomly from all files.
    fn select_file_index(&mut self) -> Option<usize> {
        let file_list = self.file_list.as_ref()?;
        
        if let Some((start, end)) = self.file_range {
            // PARTITIONED mode: iterate through assigned range sequentially
            if self.current_file_index >= end {
                self.current_file_index = start;  // Wrap around
            }
            let index = self.current_file_index;
            self.current_file_index += 1;
            Some(index)
        } else {
            // SHARED mode: select randomly from all files
            let index = self.rng.gen_range(0..file_list.len());
            Some(index)
        }
    }
    
    /// Open a file from the file list
    ///
    /// Opens the file at the specified index and caches it for subsequent operations.
    fn open_file_from_list(&mut self, file_index: usize) -> Result<()> {
        let file_list = self.file_list.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No file list available"))?;
        
        if file_index >= file_list.len() {
            anyhow::bail!("File index {} out of range (total files: {})", file_index, file_list.len());
        }
        
        let file_path = &file_list[file_index];
        
        // Create FileTarget for this file
        use crate::target::file::FileTarget;
        use crate::target::Target;
        use crate::target::OpenFlags;
        
        // For files in a layout, they already exist - just open them
        // Don't specify file_size (let FileTarget detect it)
        let mut target = FileTarget::new(file_path.clone(), None);
        
        // Build open flags
        let mut flags = OpenFlags::default();
        if self.config.workload.direct {
            flags.direct = true;
        }
        if self.config.workload.sync {
            flags.sync = true;
        }
        // Don't create - files already exist from layout generation
        flags.create = false;
        
        // Open the file
        target.open(flags)?;
        
        // Cache file info
        self.current_file_fd = target.fd();
        self.current_file_size = target.size();
        self.current_file = Some(Box::new(target));
        
        Ok(())
    }
    
    /// Prepare and submit a single IO operation (without polling)
    /// 
    /// This method prepares an IO operation and submits it to the engine's queue.
    /// It does NOT poll for completions - that's done separately to allow batching.
    /// 
    /// Returns metadata about the in-flight operation for later completion processing.
    fn prepare_and_submit_operation(&mut self, op_type: OperationType) -> Result<InFlightOp> {
        // Select block size first (needs &mut self)
        let block_size = self.select_block_size(op_type);
        
        // Handle file list mode vs single file mode
        let (target_fd, target_size) = if self.file_list.is_some() {
            // File list mode: select and open file
            if let Some(file_index) = self.select_file_index() {
                self.open_file_from_list(file_index)?;
                (self.current_file_fd, self.current_file_size)
            } else {
                anyhow::bail!("Failed to select file from list");
            }
        } else {
            // Single file mode: use cached target info
            (self.cached_target_fd, self.cached_target_size)
        };
        
        let lock_mode = self.config.targets[0].lock_mode;
        
        // Generate block number using distribution, then convert to byte offset
        // This ensures offsets are naturally aligned to block size (required for O_DIRECT)
        
        let offset = if let Some((start_offset, end_offset)) = self.config.workers.offset_range {
            // Partitioned mode: constrain to assigned offset range
            let range_size = end_offset - start_offset;
            let num_blocks = range_size / (block_size as u64);
            let block_num = self.distribution.next_block(num_blocks);
            start_offset + (block_num * (block_size as u64))
        } else {
            // Shared mode: use full file
            let num_blocks = target_size / (block_size as u64);
            let block_num = self.distribution.next_block(num_blocks);
            block_num * (block_size as u64)
        };
        
        // Length is simply the block size (already aligned by design)
        let length = block_size;
        
        // Track block access statistics (only if heatmap enabled)
        // Note: Coverage and unique block tracking have ~5-10% performance overhead
        if self.config.workload.heatmap {
            let block_num = offset / (block_size as u64);
            self.stats.record_block_access(block_num);
            self.stats.record_unique_block(block_num);
        }
        
        // Get buffer from pool (remove .context() for hot path performance)
        let buf_idx = self.buffer_pool.get()
            .ok_or_else(|| anyhow::anyhow!("No buffers available"))?;
        
        // Determine actual length
        let length = {
            let buffer = self.buffer_pool.get_buffer_mut(buf_idx);
            length.min(buffer.size())
        };
        
        // Fill buffer with pattern data if writing (only for non-random patterns or verification)
        if op_type == OperationType::Write {
            let pattern = if self.config.runtime.verify {
                // If verification is enabled, use verification pattern
                self.config.runtime.verify_pattern.unwrap_or(VerifyPattern::Sequential)
            } else {
                // Otherwise use configured write pattern (default: random)
                self.config.workload.write_pattern
            };
            
            // Only fill buffer if NOT using random pattern (random buffers are pre-filled at init)
            if pattern != VerifyPattern::Random || self.config.runtime.verify {
                let buffer = self.buffer_pool.get_buffer_mut(buf_idx);
                fill_buffer_for_verification(buffer, pattern, offset, length, self.id);
            }
        }
        
        // Get buffer pointer for IO
        let buffer_ptr = {
            let buffer = self.buffer_pool.get_buffer_mut(buf_idx);
            buffer.as_mut_ptr()
        };
        
        // Acquire lock if needed
        // TODO: Lock handling with async IO needs more thought - locks are held across async operations
        // For now, we'll skip locking with async engines (QD > 1)
        let _lock_guard = if lock_mode != FileLockMode::None && self.config.workload.queue_depth == 1 {
            let lock_start = Instant::now();
            
            // Convert config FileLockMode to target FileLockMode
            let target_lock_mode = match lock_mode {
                FileLockMode::None => TargetFileLockMode::None,
                FileLockMode::Range => TargetFileLockMode::Range,
                FileLockMode::Full => TargetFileLockMode::Full,
            };
            
            // Use current_file if in file list mode, otherwise use targets[0]
            let guard = if let Some(ref mut current_file) = self.current_file {
                Some(current_file.lock(
                    target_lock_mode,
                    offset,
                    length as u64,
                )?)
            } else {
                Some(self.targets[0].lock(
                    target_lock_mode,
                    offset,
                    length as u64,
                )?)
            };
            
            let lock_latency = lock_start.elapsed();
            self.stats.record_lock_latency(lock_latency);
            
            guard
        } else {
            None
        };
        
        // Record start time for latency measurement
        let io_start = FastInstant::now();
        
        // Build and submit IO operation
        let op = IOOperation {
            op_type,
            target_fd,
            offset,
            buffer: buffer_ptr,
            length,
            user_data: buf_idx as u64,
        };
        
        // Submit to engine (does NOT poll)
        self.engine.submit(op)?;
        
        // Return metadata for completion processing
        Ok(InFlightOp {
            buf_idx,
            op_type,
            offset,
            length,
            start_time: io_start,
        })
    }
    
    /// Poll for and process IO completions
    ///
    /// This method polls the IO engine for completed operations and processes them.
    /// It updates statistics, verifies data if needed, and returns buffers to the pool.
    ///
    /// # Arguments
    ///
    /// * `in_flight_ops` - Vector of in-flight operations to match against completions
    fn process_completions(&mut self, in_flight_ops: &mut Vec<InFlightOp>) -> Result<()> {
        // Poll for completions
        let completions = self.engine.poll_completions()?;
        
        // Process each completion
        for completion in completions {
            // Find the matching in-flight operation
            let op_idx = in_flight_ops.iter()
                .position(|op| op.buf_idx == completion.user_data as usize)
                .ok_or_else(|| anyhow::anyhow!("Completion for unknown operation"))?;
            
            let in_flight_op = in_flight_ops.remove(op_idx);
            
            // Calculate latency
            let io_end = FastInstant::now();
            let io_latency = io_end.duration_since(in_flight_op.start_time);
            
            // Verify buffer if reading
            if completion.op_type == OperationType::Read && self.config.runtime.verify {
                if let Ok(bytes) = completion.result {
                    let verify_pattern = self.config.runtime.verify_pattern.unwrap_or(VerifyPattern::Sequential);
                    let buffer = self.buffer_pool.get_buffer_mut(in_flight_op.buf_idx);
                    
                    // Record verification attempt
                    self.stats.record_verification();
                    
                    if !verify_buffer_after_verification(buffer, verify_pattern, in_flight_op.offset, bytes, self.id) {
                        self.stats.record_verification_failure();
                        self.stats.record_error();
                    }
                }
            }
            
            // Return buffer to pool
            self.buffer_pool.return_buffer(in_flight_op.buf_idx);
            
            // Record statistics
            match completion.result {
                Ok(bytes) => {
                    self.stats.record_io(completion.op_type, bytes, io_latency);
                    self.total_bytes_transferred += bytes as u64;
                    self.operation_count += 1;
                }
                Err(e) => {
                    self.stats.record_error();
                    return Err(e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Select block size based on operation type and IO patterns
    #[inline(always)]
    fn select_block_size(&mut self, op_type: OperationType) -> usize {
        let patterns = match op_type {
            OperationType::Read => &self.config.workload.read_distribution,
            OperationType::Write => &self.config.workload.write_distribution,
            _ => return self.config.workload.block_size as usize, // Use configured block size for fsync
        };
        
        // If no patterns defined, use configured block size
        if patterns.is_empty() {
            return self.config.workload.block_size as usize;
        }
        
        // If only one pattern, use it
        if patterns.len() == 1 {
            return patterns[0].block_size as usize;
        }
        
        // Select pattern based on weights
        let roll = self.rng.gen_range(0..100);
        let mut cumulative = 0;
        
        for pattern in patterns {
            cumulative += pattern.weight;
            if roll < cumulative {
                return pattern.block_size as usize;
            }
        }
        
        // Fallback to last pattern
        patterns.last().unwrap().block_size as usize
    }
    
    /// Apply think time delay
    fn apply_think_time(&self, config: &ThinkTimeConfig, io_latency: Duration) {
        let duration = if let Some(pct) = config.adaptive_percent {
            // Adaptive: percentage of IO latency
            io_latency.mul_f64(pct as f64 / 100.0)
        } else {
            // Fixed duration
            Duration::from_micros(config.duration_us)
        };
        
        match config.mode {
            ThinkTimeMode::Sleep => {
                std::thread::sleep(duration);
            }
            ThinkTimeMode::Spin => {
                let start = Instant::now();
                while start.elapsed() < duration {
                    std::hint::spin_loop();
                }
            }
        }
    }
    
    /// Get worker ID
    pub fn id(&self) -> usize {
        self.id
    }
}

/// Fill buffer with verification pattern for write operations
fn fill_buffer_for_verification(
    buffer: &mut crate::util::buffer::AlignedBuffer,
    pattern: VerifyPattern,
    offset: u64,
    length: usize,
    _worker_id: usize,
) {
    use crate::util::verification::{fill_buffer, VerificationPattern as VerifyPat};
    
    let slice = unsafe {
        std::slice::from_raw_parts_mut(buffer.as_mut_ptr(), length)
    };
    
    let verify_pattern = match pattern {
        VerifyPattern::Zeros => VerifyPat::Zeros,
        VerifyPattern::Ones => VerifyPat::Ones,
        VerifyPattern::Random => VerifyPat::Random(offset),
        VerifyPattern::Sequential => VerifyPat::Sequential,
    };
    
    fill_buffer(slice, verify_pattern, offset);
}

/// Verify buffer after read operation
fn verify_buffer_after_verification(
    buffer: &mut crate::util::buffer::AlignedBuffer,
    pattern: VerifyPattern,
    offset: u64,
    bytes: usize,
    worker_id: usize,
) -> bool {
    use crate::util::verification::{verify_buffer, VerificationPattern as VerifyPat, VerificationResult};
    
    let slice = unsafe {
        std::slice::from_raw_parts(buffer.as_mut_ptr(), bytes)
    };
    
    let verify_pattern = match pattern {
        VerifyPattern::Zeros => VerifyPat::Zeros,
        VerifyPattern::Ones => VerifyPat::Ones,
        VerifyPattern::Random => VerifyPat::Random(offset),
        VerifyPattern::Sequential => VerifyPat::Sequential,
    };
    
    match verify_buffer(slice, verify_pattern, offset) {
        VerificationResult::Success => true,
        VerificationResult::Failure { offset: fail_offset, expected, actual } => {
            eprintln!(
                "Worker {}: Verification failure at buffer offset {}: expected 0x{:02x}, got 0x{:02x}",
                worker_id, fail_offset, expected, actual
            );
            false
        }
    }
}

// Extension trait for WorkloadConfig to convert to EngineConfig
#[allow(dead_code)]
trait WorkloadConfigExt {
    fn to_engine_config(&self) -> EngineConfig;
}

impl WorkloadConfigExt for WorkloadConfig {
    fn to_engine_config(&self) -> EngineConfig {
        EngineConfig {
            queue_depth: self.queue_depth,
            use_registered_buffers: false, // Will be configurable later
            use_fixed_files: false,        // Will be configurable later
            polling_mode: false,           // Will be configurable later
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TargetConfig, TargetType, WorkerConfig, OutputConfig, RuntimeConfig};
    use crate::config::workload::{IOPattern, AccessPattern};
    use std::path::PathBuf;
    
    fn create_test_config() -> Config {
        Config {
            workload: WorkloadConfig {
                read_percent: 100,
                write_percent: 0,
                read_distribution: vec![],
                write_distribution: vec![],
                block_size: 4096,
                queue_depth: 32,
                completion_mode: CompletionMode::Duration { seconds: 1 },
                random: false,
                distribution: DistributionType::Uniform,
                think_time: None,
                engine: EngineType::Sync,
                direct: false,
                sync: false,
                heatmap: false,
                heatmap_buckets: 100,
                write_pattern: VerifyPattern::Random,
            },
            targets: vec![
                TargetConfig {
                    path: PathBuf::from("/tmp/test.dat"),
                    target_type: TargetType::File,
                    file_size: Some(1024 * 1024),
                    num_files: None,
                    num_dirs: None,
                    layout_config: None,
                    layout_manifest: None,
                    export_layout_manifest: None,
                    distribution: FileDistribution::Shared,
                    fadvise_flags: FadviseFlags::default(),
                    madvise_flags: MadviseFlags::default(),
                    lock_mode: FileLockMode::None,
                    preallocate: false,
                    truncate_to_size: false,
                    refill: false,
                    refill_pattern: workload::VerifyPattern::Random,
                    no_refill: false,
                }
            ],
            workers: WorkerConfig::default(),
            output: OutputConfig::default(),
            runtime: RuntimeConfig::default(),
        }
    }
    
    #[test]
    fn test_worker_creation() {
        let config = Arc::new(create_test_config());
        let worker = Worker::new(0, config);
        assert!(worker.is_ok());
    }
    
    #[test]
    fn test_create_engine_sync() {
        let config = create_test_config();
        let engine = Worker::create_engine(&config.workload);
        assert!(engine.is_ok());
    }
    
    #[test]
    fn test_create_distribution_uniform() {
        let config = create_test_config();
        let dist = Worker::create_distribution(&config.workload);
        assert!(dist.is_ok());
    }
    
    #[test]
    fn test_create_distribution_zipf() {
        let mut config = create_test_config();
        config.workload.distribution = DistributionType::Zipf { theta: 1.2 };
        let dist = Worker::create_distribution(&config.workload);
        assert!(dist.is_ok());
    }
    
    #[test]
    fn test_select_operation_type() {
        let config = Arc::new(create_test_config());
        let mut worker = Worker::new(0, config).unwrap();
        
        // With 100% read, should always return Read
        let op = worker.select_operation_type();
        assert_eq!(op, OperationType::Read);
    }
    
    #[test]
    fn test_should_stop_duration() {
        let config = Arc::new(create_test_config());
        let mut worker = Worker::new(0, config).unwrap();
        
        // Before start, should not stop
        assert!(!worker.should_stop());
        
        // After start but within duration, should not stop
        worker.start_time = Some(Instant::now());
        assert!(!worker.should_stop());
        
        // After duration expires, should stop
        worker.start_time = Some(Instant::now() - Duration::from_secs(2));
        assert!(worker.should_stop());
    }
    
    #[test]
    fn test_should_stop_total_bytes() {
        let mut config = create_test_config();
        config.workload.completion_mode = CompletionMode::TotalBytes { bytes: 1024 };
        let config = Arc::new(config);
        let mut worker = Worker::new(0, config).unwrap();
        
        // Before reaching bytes, should not stop
        worker.total_bytes_transferred = 512;
        assert!(!worker.should_stop());
        
        // After reaching bytes, should stop
        worker.total_bytes_transferred = 1024;
        assert!(worker.should_stop());
    }
    
    #[test]
    fn test_select_block_size_default() {
        let config = Arc::new(create_test_config());
        let mut worker = Worker::new(0, config).unwrap();
        
        // With no IO patterns, should return default 4096
        let size = worker.select_block_size(OperationType::Read);
        assert_eq!(size, 4096);
    }
    
    #[test]
    fn test_select_block_size_single_pattern() {
        let mut config = create_test_config();
        config.workload.read_distribution = vec![
            IOPattern {
                weight: 100,
                access: AccessPattern::Random,
                block_size: 8192,
            }
        ];
        let config = Arc::new(config);
        let mut worker = Worker::new(0, config).unwrap();
        
        let size = worker.select_block_size(OperationType::Read);
        assert_eq!(size, 8192);
    }
    
    #[test]
    fn test_select_block_size_multiple_patterns() {
        let mut config = create_test_config();
        config.workload.read_distribution = vec![
            IOPattern {
                weight: 50,
                access: AccessPattern::Random,
                block_size: 4096,
            },
            IOPattern {
                weight: 50,
                access: AccessPattern::Sequential,
                block_size: 65536,
            }
        ];
        let config = Arc::new(config);
        let mut worker = Worker::new(0, config).unwrap();
        
        // Should return one of the two block sizes
        let size = worker.select_block_size(OperationType::Read);
        assert!(size == 4096 || size == 65536);
    }
}

