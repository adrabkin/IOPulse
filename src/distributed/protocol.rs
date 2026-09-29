//! Distributed mode protocol
//!
//! This module defines the protocol for communication between coordinator and worker nodes
//! in distributed mode. The protocol uses MessagePack (rmp-serde) for efficient binary 
//! serialization with full serde feature support.
//!
//! # Protocol Version
//!
//! Current version: 1
//!
//! # Serialization Format
//!
//! MessagePack was chosen over bincode because:
//! - Supports all serde features (rename_all, default, etc.)
//! - Compact binary format (~10-20% larger than bincode, still much smaller than JSON)
//! - Fast serialization/deserialization
//! - Wide language support (for future interoperability)
//!
//! # Message Flow
//!
//! ```text
//! Coordinator                     Worker Node
//!     |                              |
//!     |-------- CONFIG ------------->|
//!     |                              |
//!     |<------- READY ---------------|
//!     |                              |
//!     |-- START(timestamp) --------->|
//!     |                              |
//!     |<----- HEARTBEAT(stats) ------|
//!     |-- HEARTBEAT_ACK ------------>|
//!     |                              |
//!     |-------- STOP --------------->|
//!     |                              |
//!     |<----- RESULTS(stats) --------|
//! ```
//!
//! # Message Framing
//!
//! Each message is prefixed with a 4-byte length field (little-endian u32):
//!
//! ```text
//! [4 bytes: message length][N bytes: bincode-serialized message]
//! ```

use serde::{Deserialize, Serialize};
use crate::config::Config;
use crate::stats::WorkerStats;
use anyhow::{Context, Result};

/// Protocol version
///
/// Increment this when making breaking changes to the protocol.
/// Coordinator and workers must have matching protocol versions.
pub const PROTOCOL_VERSION: u32 = 2;

/// Serializable worker statistics snapshot
///
/// This is a comprehensive version of WorkerStats that can be serialized
/// and sent over the network. It contains all statistics needed for
/// complete distributed mode output matching standalone mode.
///
/// Histograms are serialized using bincode for efficient network transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerStatsSnapshot {
    // Basic IO counters
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub errors: u64,
    pub test_duration_ns: u64,
    
    // Error breakdown
    pub errors_read: u64,
    pub errors_write: u64,
    pub errors_metadata: u64,
    
    // Verification statistics
    pub verify_ops: u64,
    pub verify_failures: u64,
    
    // Block size verification
    pub min_bytes_per_op: u64,
    pub max_bytes_per_op: u64,
    
    // Queue depth statistics
    pub avg_queue_depth: f64,
    pub peak_queue_depth: u64,
    
    // Latency histograms (bincode-serialized SimpleHistogram)
    pub io_latency_histogram: Vec<u8>,
    pub read_latency_histogram: Vec<u8>,
    pub write_latency_histogram: Vec<u8>,
    
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
    
    // Metadata latency histograms (bincode-serialized)
    pub metadata_open_latency: Vec<u8>,
    pub metadata_close_latency: Vec<u8>,
    pub metadata_stat_latency: Vec<u8>,
    pub metadata_setattr_latency: Vec<u8>,
    pub metadata_mkdir_latency: Vec<u8>,
    pub metadata_rmdir_latency: Vec<u8>,
    pub metadata_unlink_latency: Vec<u8>,
    pub metadata_rename_latency: Vec<u8>,
    pub metadata_readdir_latency: Vec<u8>,
    pub metadata_fsync_latency: Vec<u8>,
    
    // Resource utilization
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub peak_memory_bytes: u64,
    
    // Coverage data (only when heatmap enabled)
    pub unique_blocks: u64,
    pub total_blocks: u64,
    
    // Lock latency histogram (optional, only when locking enabled)
    pub lock_latency_histogram: Option<Vec<u8>>,
}

impl WorkerStatsSnapshot {
    /// Create from StatsSnapshot (lightweight snapshot for heartbeats)
    ///
    /// This is used for per-worker time-series collection during the test.
    /// It only includes basic counters and latency histograms (no heatmap/coverage).
    pub fn from_stats_snapshot(snapshot: &crate::worker::StatsSnapshot) -> Result<Self> {
        // Serialize histograms using bincode
        let io_latency_histogram = Vec::new();  // Not available in StatsSnapshot
        let read_latency_histogram = bincode::serialize(&snapshot.read_latency)
            .context("Failed to serialize read_latency histogram")?;
        let write_latency_histogram = bincode::serialize(&snapshot.write_latency)
            .context("Failed to serialize write_latency histogram")?;
        
        // Serialize metadata latency histograms
        let metadata_open_latency = bincode::serialize(&snapshot.metadata_open_latency)
            .context("Failed to serialize metadata_open_latency")?;
        let metadata_close_latency = bincode::serialize(&snapshot.metadata_close_latency)
            .context("Failed to serialize metadata_close_latency")?;
        let metadata_stat_latency = bincode::serialize(&snapshot.metadata_stat_latency)
            .context("Failed to serialize metadata_stat_latency")?;
        let metadata_setattr_latency = bincode::serialize(&snapshot.metadata_setattr_latency)
            .context("Failed to serialize metadata_setattr_latency")?;
        let metadata_mkdir_latency = bincode::serialize(&snapshot.metadata_mkdir_latency)
            .context("Failed to serialize metadata_mkdir_latency")?;
        let metadata_rmdir_latency = bincode::serialize(&snapshot.metadata_rmdir_latency)
            .context("Failed to serialize metadata_rmdir_latency")?;
        let metadata_unlink_latency = bincode::serialize(&snapshot.metadata_unlink_latency)
            .context("Failed to serialize metadata_unlink_latency")?;
        let metadata_rename_latency = bincode::serialize(&snapshot.metadata_rename_latency)
            .context("Failed to serialize metadata_rename_latency")?;
        let metadata_readdir_latency = bincode::serialize(&snapshot.metadata_readdir_latency)
            .context("Failed to serialize metadata_readdir_latency")?;
        let metadata_fsync_latency = bincode::serialize(&snapshot.metadata_fsync_latency)
            .context("Failed to serialize metadata_fsync_latency")?;
        
        Ok(Self {
            read_ops: snapshot.read_ops,
            write_ops: snapshot.write_ops,
            read_bytes: snapshot.read_bytes,
            write_bytes: snapshot.write_bytes,
            errors: snapshot.errors,
            test_duration_ns: 0,  // Not available in StatsSnapshot
            errors_read: 0,  // Not tracked in StatsSnapshot
            errors_write: 0,  // Not tracked in StatsSnapshot
            errors_metadata: 0,  // Not tracked in StatsSnapshot
            verify_ops: 0,  // Not tracked in StatsSnapshot
            verify_failures: 0,  // Not tracked in StatsSnapshot
            min_bytes_per_op: 0,  // Not tracked in StatsSnapshot
            max_bytes_per_op: 0,  // Not tracked in StatsSnapshot
            avg_queue_depth: 0.0,  // Not tracked in StatsSnapshot
            peak_queue_depth: 0,  // Not tracked in StatsSnapshot
            io_latency_histogram,
            read_latency_histogram,
            write_latency_histogram,
            metadata_open_ops: snapshot.metadata_open_ops,
            metadata_close_ops: snapshot.metadata_close_ops,
            metadata_stat_ops: snapshot.metadata_stat_ops,
            metadata_setattr_ops: snapshot.metadata_setattr_ops,
            metadata_mkdir_ops: snapshot.metadata_mkdir_ops,
            metadata_rmdir_ops: snapshot.metadata_rmdir_ops,
            metadata_unlink_ops: snapshot.metadata_unlink_ops,
            metadata_rename_ops: snapshot.metadata_rename_ops,
            metadata_readdir_ops: snapshot.metadata_readdir_ops,
            metadata_fsync_ops: snapshot.metadata_fsync_ops,
            metadata_open_latency,
            metadata_close_latency,
            metadata_stat_latency,
            metadata_setattr_latency,
            metadata_mkdir_latency,
            metadata_rmdir_latency,
            metadata_unlink_latency,
            metadata_rename_latency,
            metadata_readdir_latency,
            metadata_fsync_latency,
            cpu_percent: 0.0,  // Not tracked per-worker in StatsSnapshot
            memory_bytes: 0,  // Not tracked per-worker in StatsSnapshot
            peak_memory_bytes: 0,  // Not tracked per-worker in StatsSnapshot
            unique_blocks: 0,  // Not available in StatsSnapshot
            total_blocks: 0,  // Not available in StatsSnapshot
            lock_latency_histogram: None,  // Not tracked in StatsSnapshot
        })
    }
    
    /// Create from WorkerStats with complete statistics
    ///
    /// Serializes histograms using bincode for efficient network transfer.
    /// Calculates total_blocks from file_size and block_size if heatmap is enabled.
    pub fn from_worker_stats(stats: &WorkerStats, file_size: Option<u64>, block_size: u64) -> Result<Self> {
        // Serialize histograms using bincode
        let io_latency_histogram = bincode::serialize(stats.io_latency())
            .context("Failed to serialize io_latency histogram")?;
        let read_latency_histogram = bincode::serialize(stats.read_latency())
            .context("Failed to serialize read_latency histogram")?;
        let write_latency_histogram = bincode::serialize(stats.write_latency())
            .context("Failed to serialize write_latency histogram")?;
        
        // Serialize metadata latency histograms
        let metadata_open_latency = bincode::serialize(&stats.metadata.open_latency)
            .context("Failed to serialize metadata_open_latency")?;
        let metadata_close_latency = bincode::serialize(&stats.metadata.close_latency)
            .context("Failed to serialize metadata_close_latency")?;
        let metadata_stat_latency = bincode::serialize(&stats.metadata.stat_latency)
            .context("Failed to serialize metadata_stat_latency")?;
        let metadata_setattr_latency = bincode::serialize(&stats.metadata.setattr_latency)
            .context("Failed to serialize metadata_setattr_latency")?;
        let metadata_mkdir_latency = bincode::serialize(&stats.metadata.mkdir_latency)
            .context("Failed to serialize metadata_mkdir_latency")?;
        let metadata_rmdir_latency = bincode::serialize(&stats.metadata.rmdir_latency)
            .context("Failed to serialize metadata_rmdir_latency")?;
        let metadata_unlink_latency = bincode::serialize(&stats.metadata.unlink_latency)
            .context("Failed to serialize metadata_unlink_latency")?;
        let metadata_rename_latency = bincode::serialize(&stats.metadata.rename_latency)
            .context("Failed to serialize metadata_rename_latency")?;
        let metadata_readdir_latency = bincode::serialize(&stats.metadata.readdir_latency)
            .context("Failed to serialize metadata_readdir_latency")?;
        let metadata_fsync_latency = bincode::serialize(&stats.metadata.fsync_latency)
            .context("Failed to serialize metadata_fsync_latency")?;
        
        // Serialize lock latency if present
        let lock_latency_histogram = if let Some(ref lock_hist) = stats.lock_latency() {
            Some(bincode::serialize(lock_hist)
                .context("Failed to serialize lock_latency histogram")?)
        } else {
            None
        };
        
        // Get resource stats
        let (cpu_percent, memory_bytes, peak_memory_bytes) = if let Some(resource_stats) = stats.resource_stats() {
            (resource_stats.cpu_percent, resource_stats.memory_bytes, resource_stats.peak_memory_bytes)
        } else {
            (0.0, 0, 0)
        };
        
        // Calculate total_blocks for coverage
        let total_blocks = if let Some(fs) = file_size {
            if block_size > 0 {
                fs / block_size
            } else {
                0
            }
        } else {
            0
        };
        
        Ok(Self {
            read_ops: stats.read_ops(),
            write_ops: stats.write_ops(),
            read_bytes: stats.read_bytes(),
            write_bytes: stats.write_bytes(),
            errors: stats.errors(),
            test_duration_ns: stats.test_duration()
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
            errors_read: stats.errors_read(),
            errors_write: stats.errors_write(),
            errors_metadata: stats.errors_metadata(),
            verify_ops: stats.verify_ops(),
            verify_failures: stats.verify_failures(),
            min_bytes_per_op: stats.min_bytes_per_op(),
            max_bytes_per_op: stats.max_bytes_per_op(),
            avg_queue_depth: stats.avg_queue_depth(),
            peak_queue_depth: stats.peak_queue_depth(),
            io_latency_histogram,
            read_latency_histogram,
            write_latency_histogram,
            metadata_open_ops: stats.metadata.open_ops.get(),
            metadata_close_ops: stats.metadata.close_ops.get(),
            metadata_stat_ops: stats.metadata.stat_ops.get(),
            metadata_setattr_ops: stats.metadata.setattr_ops.get(),
            metadata_mkdir_ops: stats.metadata.mkdir_ops.get(),
            metadata_rmdir_ops: stats.metadata.rmdir_ops.get(),
            metadata_unlink_ops: stats.metadata.unlink_ops.get(),
            metadata_rename_ops: stats.metadata.rename_ops.get(),
            metadata_readdir_ops: stats.metadata.readdir_ops.get(),
            metadata_fsync_ops: stats.metadata.fsync_ops.get(),
            metadata_open_latency,
            metadata_close_latency,
            metadata_stat_latency,
            metadata_setattr_latency,
            metadata_mkdir_latency,
            metadata_rmdir_latency,
            metadata_unlink_latency,
            metadata_rename_latency,
            metadata_readdir_latency,
            metadata_fsync_latency,
            cpu_percent,
            memory_bytes,
            peak_memory_bytes,
            unique_blocks: stats.unique_blocks_count(),
            total_blocks,
            lock_latency_histogram,
        })
    }
    
    /// Convert back to WorkerStats for use with print_results()
    ///
    /// Deserializes histograms and reconstructs a WorkerStats instance.
    /// This allows reusing standalone's print_results() function.
    pub fn to_worker_stats(&self, enable_heatmap: bool, track_locks: bool) -> Result<WorkerStats> {
        use crate::stats::simple_histogram::SimpleHistogram;
        
        // Deserialize histograms
        let io_latency: SimpleHistogram = bincode::deserialize(&self.io_latency_histogram)
            .context("Failed to deserialize io_latency histogram")?;
        let read_latency: SimpleHistogram = bincode::deserialize(&self.read_latency_histogram)
            .context("Failed to deserialize read_latency histogram")?;
        let write_latency: SimpleHistogram = bincode::deserialize(&self.write_latency_histogram)
            .context("Failed to deserialize write_latency histogram")?;
        
        // Deserialize metadata latency histograms
        let metadata_open_latency: SimpleHistogram = bincode::deserialize(&self.metadata_open_latency)
            .context("Failed to deserialize metadata_open_latency")?;
        let metadata_close_latency: SimpleHistogram = bincode::deserialize(&self.metadata_close_latency)
            .context("Failed to deserialize metadata_close_latency")?;
        let metadata_stat_latency: SimpleHistogram = bincode::deserialize(&self.metadata_stat_latency)
            .context("Failed to deserialize metadata_stat_latency")?;
        let metadata_setattr_latency: SimpleHistogram = bincode::deserialize(&self.metadata_setattr_latency)
            .context("Failed to deserialize metadata_setattr_latency")?;
        let metadata_mkdir_latency: SimpleHistogram = bincode::deserialize(&self.metadata_mkdir_latency)
            .context("Failed to deserialize metadata_mkdir_latency")?;
        let metadata_rmdir_latency: SimpleHistogram = bincode::deserialize(&self.metadata_rmdir_latency)
            .context("Failed to deserialize metadata_rmdir_latency")?;
        let metadata_unlink_latency: SimpleHistogram = bincode::deserialize(&self.metadata_unlink_latency)
            .context("Failed to deserialize metadata_unlink_latency")?;
        let metadata_rename_latency: SimpleHistogram = bincode::deserialize(&self.metadata_rename_latency)
            .context("Failed to deserialize metadata_rename_latency")?;
        let metadata_readdir_latency: SimpleHistogram = bincode::deserialize(&self.metadata_readdir_latency)
            .context("Failed to deserialize metadata_readdir_latency")?;
        let metadata_fsync_latency: SimpleHistogram = bincode::deserialize(&self.metadata_fsync_latency)
            .context("Failed to deserialize metadata_fsync_latency")?;
        
        // Deserialize lock latency if present
        let lock_latency = if let Some(ref lock_hist_bytes) = self.lock_latency_histogram {
            Some(bincode::deserialize(lock_hist_bytes)
                .context("Failed to deserialize lock_latency histogram")?)
        } else {
            None
        };
        
        // Build WorkerStats and set from snapshot
        let mut stats = WorkerStats::with_heatmap(track_locks, enable_heatmap);
        
        stats.set_from_snapshot(
            self,
            io_latency,
            read_latency,
            write_latency,
            metadata_open_latency,
            metadata_close_latency,
            metadata_stat_latency,
            metadata_setattr_latency,
            metadata_mkdir_latency,
            metadata_rmdir_latency,
            metadata_unlink_latency,
            metadata_rename_latency,
            metadata_readdir_latency,
            metadata_fsync_latency,
            lock_latency,
        )?;
        
        Ok(stats)
    }
}

impl From<&WorkerStats> for WorkerStatsSnapshot {
    fn from(stats: &WorkerStats) -> Self {
        // Use the new comprehensive method
        // For backward compatibility, we'll use default values for file_size/block_size
        Self::from_worker_stats(stats, None, 4096)
            .unwrap_or_else(|_| {
                // Fallback to basic snapshot if serialization fails
                Self {
                    read_ops: stats.read_ops(),
                    write_ops: stats.write_ops(),
                    read_bytes: stats.read_bytes(),
                    write_bytes: stats.write_bytes(),
                    errors: stats.errors(),
                    test_duration_ns: stats.test_duration()
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or(0),
                    errors_read: stats.errors_read(),
                    errors_write: stats.errors_write(),
                    errors_metadata: stats.errors_metadata(),
                    verify_ops: stats.verify_ops(),
                    verify_failures: stats.verify_failures(),
                    min_bytes_per_op: stats.min_bytes_per_op(),
                    max_bytes_per_op: stats.max_bytes_per_op(),
                    avg_queue_depth: stats.avg_queue_depth(),
                    peak_queue_depth: stats.peak_queue_depth(),
                    io_latency_histogram: Vec::new(),
                    read_latency_histogram: Vec::new(),
                    write_latency_histogram: Vec::new(),
                    metadata_open_ops: 0,
                    metadata_close_ops: 0,
                    metadata_stat_ops: 0,
                    metadata_setattr_ops: 0,
                    metadata_mkdir_ops: 0,
                    metadata_rmdir_ops: 0,
                    metadata_unlink_ops: 0,
                    metadata_rename_ops: 0,
                    metadata_readdir_ops: 0,
                    metadata_fsync_ops: 0,
                    metadata_open_latency: Vec::new(),
                    metadata_close_latency: Vec::new(),
                    metadata_stat_latency: Vec::new(),
                    metadata_setattr_latency: Vec::new(),
                    metadata_mkdir_latency: Vec::new(),
                    metadata_rmdir_latency: Vec::new(),
                    metadata_unlink_latency: Vec::new(),
                    metadata_rename_latency: Vec::new(),
                    metadata_readdir_latency: Vec::new(),
                    metadata_fsync_latency: Vec::new(),
                    cpu_percent: 0.0,
                    memory_bytes: 0,
                    peak_memory_bytes: 0,
                    unique_blocks: 0,
                    total_blocks: 0,
                    lock_latency_histogram: None,
                }
            })
    }
}

/// Protocol message
///
/// All messages exchanged between coordinator and worker nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    /// Prepare files message (Coordinator → Node)
    ///
    /// Sent before CONFIG to have nodes create/fill their assigned files.
    /// Used for distributed file creation with large datasets.
    PrepareFiles(PrepareFilesMessage),
    
    /// Files ready message (Node → Coordinator)
    ///
    /// Sent by node when it has finished creating/filling its assigned files.
    FilesReady(FilesReadyMessage),
    
    /// Configuration message (Coordinator → Node)
    ///
    /// Sent by coordinator to configure the test on a worker node.
    /// Contains complete test configuration including workload, targets, and worker settings.
    Config(ConfigMessage),
    
    /// Ready message (Node → Coordinator)
    ///
    /// Sent by node when it has prepared all workers and is ready to start.
    Ready(ReadyMessage),
    
    /// Start message (Coordinator → Node)
    ///
    /// Sent by coordinator to begin the test at a specific timestamp.
    /// Nodes wait until their local time reaches this timestamp before starting IO.
    Start(StartMessage),
    
    /// Stop message (Coordinator → Node)
    ///
    /// Sent by coordinator to stop the test.
    /// Nodes complete in-flight operations and send final results.
    Stop,
    
    /// Heartbeat message (Node → Coordinator)
    ///
    /// Sent periodically (every 1 second) with current statistics.
    /// Allows coordinator to monitor progress and detect node failures.
    Heartbeat(HeartbeatMessage),
    
    /// Heartbeat acknowledgment (Coordinator → Node)
    ///
    /// Sent by coordinator in response to heartbeat.
    /// Nodes use this as a dead man's switch (self-stop if no ACK for 10 seconds).
    HeartbeatAck,
    
    /// Results message (Node → Coordinator)
    ///
    /// Sent by node with final statistics after test completes.
    Results(ResultsMessage),
    
    /// Error message (Node → Coordinator)
    ///
    /// Sent by node when an error occurs.
    /// Coordinator aborts the test and reports the error.
    Error(ErrorMessage),
}

/// Prepare files message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareFilesMessage {
    /// Protocol version
    pub protocol_version: u32,
    
    /// Node identifier
    pub node_id: String,
    
    /// File list to create/fill
    pub file_list: Vec<std::path::PathBuf>,
    
    /// File size for each file (or region size for partitioned pre-allocation)
    pub file_size: u64,
    
    /// Start offset for partitioned pre-allocation (0 for full file)
    pub start_offset: u64,
    
    /// Pattern to use for filling
    pub fill_pattern: crate::config::workload::VerifyPattern,
    
    /// Whether files need to be filled (true) or just created (false)
    pub fill_files: bool,
}

/// Files ready message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesReadyMessage {
    /// Protocol version
    pub protocol_version: u32,
    
    /// Node identifier
    pub node_id: String,
    
    /// Number of files created
    pub files_created: usize,
    
    /// Number of files filled
    pub files_filled: usize,
    
    /// Time taken (nanoseconds)
    pub duration_ns: u64,
}

/// Configuration message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigMessage {
    /// Protocol version (must match)
    pub protocol_version: u32,
    
    /// Node identifier (IP address or hostname)
    pub node_id: String,
    
    /// Complete test configuration
    pub config: Config,
    
    /// Worker ID range for this node
    ///
    /// For global worker partitioning:
    /// - Node 0: workers 0-15
    /// - Node 1: workers 16-31
    /// - Node 2: workers 32-47
    pub worker_id_start: usize,
    pub worker_id_end: usize,
    
    /// File list (if using layout_manifest)
    ///
    /// For PARTITIONED mode, this is the subset of files assigned to this node.
    /// For SHARED mode, this is the complete file list.
    pub file_list: Option<Vec<std::path::PathBuf>>,
    
    /// File range for PARTITIONED mode
    ///
    /// Specifies which files this node should process.
    /// For SHARED mode, this is None (all workers access all files).
    pub file_range: Option<(usize, usize)>,
    
    /// Skip pre-allocation (coordinator already did it)
    ///
    /// For SHARED files in distributed mode, coordinator pre-allocates once,
    /// and nodes skip it to avoid redundant work.
    pub skip_preallocation: bool,
}

/// Ready message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyMessage {
    /// Protocol version
    pub protocol_version: u32,
    
    /// Node identifier
    pub node_id: String,
    
    /// Number of worker threads on this node
    pub num_workers: usize,
    
    /// Node is ready to start
    pub ready: bool,
}

/// Start message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartMessage {
    /// Timestamp (nanoseconds since epoch) when IO should begin
    ///
    /// Nodes wait until their local time reaches this timestamp.
    /// This ensures synchronized start across all nodes.
    pub start_timestamp_ns: u64,
}

/// Heartbeat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    /// Node identifier
    pub node_id: String,
    
    /// Elapsed time since test start (nanoseconds)
    ///
    /// Using elapsed time instead of absolute time avoids clock skew issues.
    pub elapsed_ns: u64,
    
    /// Current aggregate statistics for this node
    pub stats: WorkerStatsSnapshot,
    
    /// Optional per-worker snapshots (only when --per-worker-output is enabled)
    pub per_worker_stats: Option<Vec<WorkerStatsSnapshot>>,
}

/// Results message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultsMessage {
    /// Node identifier
    pub node_id: String,
    
    /// Test duration (nanoseconds)
    pub duration_ns: u64,
    
    /// Per-worker statistics
    pub per_worker_stats: Vec<WorkerStatsSnapshot>,
    
    /// Aggregate statistics for this node
    pub aggregate_stats: WorkerStatsSnapshot,
}

/// Error message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMessage {
    /// Node identifier
    pub node_id: String,
    
    /// Error description
    pub error: String,
    
    /// Error occurred at (elapsed nanoseconds)
    pub elapsed_ns: u64,
}

/// Serialize a message to bytes
///
/// Uses bincode for efficient binary serialization.
/// Prepends a 4-byte length field for framing.
///
/// # Message Format
///
/// ```text
/// [4 bytes: message length (little-endian u32)][N bytes: bincode message]
/// ```
pub fn serialize_message(msg: &Message) -> Result<Vec<u8>> {
    // Serialize message with MessagePack (supports all serde features)
    let msg_bytes = rmp_serde::to_vec(msg)
        .context("Failed to serialize message")?;
    
    // Prepend length field
    let msg_len = msg_bytes.len() as u32;
    let mut framed = Vec::with_capacity(4 + msg_bytes.len());
    framed.extend_from_slice(&msg_len.to_le_bytes());
    framed.extend_from_slice(&msg_bytes);
    
    Ok(framed)
}

/// Deserialize a message from bytes
///
/// Expects a 4-byte length prefix followed by MessagePack-serialized message.
///
/// # Returns
///
/// Returns (message, bytes_consumed) where bytes_consumed includes the length prefix.
pub fn deserialize_message(buf: &[u8]) -> Result<(Message, usize)> {
    // Need at least 4 bytes for length
    if buf.len() < 4 {
        anyhow::bail!("Buffer too small for message length (need 4 bytes, got {})", buf.len());
    }
    
    // Read length field
    let msg_len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    
    // Check if we have the complete message
    if buf.len() < 4 + msg_len {
        anyhow::bail!("Incomplete message (need {} bytes, got {})", 4 + msg_len, buf.len());
    }
    
    // Deserialize message
    let msg = rmp_serde::from_slice(&buf[4..4 + msg_len])
        .context("Failed to deserialize message")?;
    
    Ok((msg, 4 + msg_len))
}

/// Read a complete message from a TCP stream
///
/// Reads the length prefix, then reads the complete message.
/// Handles partial reads and buffering.
pub async fn read_message(stream: &mut tokio::net::TcpStream) -> Result<Message> {
    use tokio::io::AsyncReadExt;
    
    // Read length field (4 bytes)
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await
        .context("Failed to read message length")?;
    
    let msg_len = u32::from_le_bytes(len_buf) as usize;
    
    // Sanity check: reject messages > 100MB
    if msg_len > 100 * 1024 * 1024 {
        anyhow::bail!("Message too large: {} bytes (max 100MB)", msg_len);
    }
    
    // Read message body
    let mut msg_buf = vec![0u8; msg_len];
    stream.read_exact(&mut msg_buf).await
        .context("Failed to read message body")?;
    
    // Deserialize
    let msg = rmp_serde::from_slice(&msg_buf)
        .context("Failed to deserialize message")?;
    
    Ok(msg)
}

/// Write a message to a TCP stream
///
/// Serializes the message with length prefix and writes to stream.
pub async fn write_message(stream: &mut tokio::net::TcpStream, msg: &Message) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    
    // Serialize with length prefix
    let framed = serialize_message(msg)?;
    
    // Write to stream
    stream.write_all(&framed).await
        .context("Failed to write message")?;
    
    // Flush to ensure message is sent immediately
    stream.flush().await
        .context("Failed to flush stream")?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{WorkloadConfig, TargetConfig, WorkerConfig, OutputConfig, RuntimeConfig};
    
    #[test]
    fn test_serialize_deserialize_ready() {
        let msg = Message::Ready(ReadyMessage {
            protocol_version: PROTOCOL_VERSION,
            node_id: "10.0.1.10".to_string(),
            num_workers: 16,
            ready: true,
        });
        
        let bytes = serialize_message(&msg).unwrap();
        let (deserialized, consumed) = deserialize_message(&bytes).unwrap();
        
        assert_eq!(consumed, bytes.len());
        
        match deserialized {
            Message::Ready(ready) => {
                assert_eq!(ready.protocol_version, PROTOCOL_VERSION);
                assert_eq!(ready.node_id, "10.0.1.10");
                assert_eq!(ready.num_workers, 16);
                assert!(ready.ready);
            }
            _ => panic!("Wrong message type"),
        }
    }
    
    #[test]
    fn test_serialize_deserialize_start() {
        let msg = Message::Start(StartMessage {
            start_timestamp_ns: 1234567890,
        });
        
        let bytes = serialize_message(&msg).unwrap();
        let (deserialized, consumed) = deserialize_message(&bytes).unwrap();
        
        assert_eq!(consumed, bytes.len());
        
        match deserialized {
            Message::Start(start) => {
                assert_eq!(start.start_timestamp_ns, 1234567890);
            }
            _ => panic!("Wrong message type"),
        }
    }
    
    #[test]
    fn test_serialize_deserialize_stop() {
        let msg = Message::Stop;
        
        let bytes = serialize_message(&msg).unwrap();
        let (deserialized, consumed) = deserialize_message(&bytes).unwrap();
        
        assert_eq!(consumed, bytes.len());
        
        match deserialized {
            Message::Stop => {}
            _ => panic!("Wrong message type"),
        }
    }
    
    #[test]
    fn test_serialize_deserialize_error() {
        let msg = Message::Error(ErrorMessage {
            node_id: "10.0.1.10".to_string(),
            error: "Test error".to_string(),
            elapsed_ns: 5000000000,
        });
        
        let bytes = serialize_message(&msg).unwrap();
        let (deserialized, consumed) = deserialize_message(&bytes).unwrap();
        
        assert_eq!(consumed, bytes.len());
        
        match deserialized {
            Message::Error(err) => {
                assert_eq!(err.node_id, "10.0.1.10");
                assert_eq!(err.error, "Test error");
                assert_eq!(err.elapsed_ns, 5000000000);
            }
            _ => panic!("Wrong message type"),
        }
    }
    
    #[test]
    fn test_protocol_version() {
        assert_eq!(PROTOCOL_VERSION, 2);
    }
    
    #[test]
    fn test_message_framing() {
        let msg = Message::Stop;
        let bytes = serialize_message(&msg).unwrap();
        
        // Check length prefix
        assert!(bytes.len() >= 4);
        let msg_len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        assert_eq!(bytes.len(), 4 + msg_len);
    }
}
