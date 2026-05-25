//! JSON output formatting
//!
//! This module provides JSON serialization for IOPulse statistics with support for:
//! - Time-series data at configurable intervals
//! - Per-worker detail (optional)
//! - Histogram data (percentiles + optional raw buckets)
//! - Metadata operations tracking
//! - Resource utilization (CPU, memory)
//! - Coverage data (when heatmap enabled)
//! - Distributed mode (per-node files + aggregate)

use crate::stats::{WorkerStats, MetadataStats};
use crate::stats::simple_histogram::SimpleHistogram;
use crate::util::resource::ResourceStats;
use serde::{Serialize, Deserialize};
use std::time::Duration;
use std::path::Path;
use std::fs::File;
use crate::Result;

/// Duration with both microseconds and human-readable format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonDuration {
    pub micros: u64,
    pub human: String,
}

impl JsonDuration {
    pub fn from_duration(d: Duration) -> Self {
        let micros = d.as_micros() as u64;
        let human = format_duration_human(d);
        Self { micros, human }
    }
}

/// Throughput with bytes/sec and human-readable format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonThroughput {
    pub bytes_per_sec: u64,
    pub human: String,
}

impl JsonThroughput {
    pub fn new(bytes_per_sec: u64) -> Self {
        let human = format_throughput(bytes_per_sec);
        Self { bytes_per_sec, human }
    }
}

/// Latency statistics with percentiles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonLatency {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<JsonDuration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<JsonDuration>,
    pub mean: JsonDuration,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p50: Option<JsonDuration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p90: Option<JsonDuration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95: Option<JsonDuration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p99: Option<JsonDuration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p99_9: Option<JsonDuration>,
}

/// Metadata operation latency statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonMetadataLatency {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<JsonLatencySimple>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close: Option<JsonLatencySimple>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stat: Option<JsonLatencySimple>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setattr: Option<JsonLatencySimple>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mkdir: Option<JsonLatencySimple>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rmdir: Option<JsonLatencySimple>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlink: Option<JsonLatencySimple>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rename: Option<JsonLatencySimple>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readdir: Option<JsonLatencySimple>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fsync: Option<JsonLatencySimple>,
}

/// Simple latency stats (mean + p99 only, for brevity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonLatencySimple {
    pub mean: JsonDuration,
    pub p99: JsonDuration,
}

/// Metadata operation statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonMetadataOps {
    pub open_ops: u64,
    pub close_ops: u64,
    pub stat_ops: u64,
    pub setattr_ops: u64,
    pub mkdir_ops: u64,
    pub rmdir_ops: u64,
    pub unlink_ops: u64,
    pub rename_ops: u64,
    pub readdir_ops: u64,
    pub fsync_ops: u64,
    pub total_ops: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<JsonMetadataLatency>,
}

/// Resource utilization statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonResourceUtil {
    pub cpu_percent_total: f64,  // Total CPU across all threads (can exceed 100%)
    pub cpu_percent_per_worker: f64,  // Average CPU per worker thread
    pub cpu_percent_system: f64,  // Percentage of total system CPU capacity
    pub num_workers: usize,  // Number of worker threads
    pub num_system_cpus: Option<usize>,  // Total system CPUs
    pub memory_bytes: u64,
    pub memory_human: String,
}

/// Coverage statistics (only when heatmap enabled)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonCoverage {
    pub unique_blocks: u64,
    pub total_blocks: u64,
    pub coverage_percent: f64,
    pub rewrite_percent: f64,
}

/// Aggregate statistics for a time interval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonAggregateStats {
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub total_ops: u64,
    pub total_bytes: u64,
    pub read_iops: u64,
    pub write_iops: u64,
    pub total_iops: u64,
    pub read_throughput: JsonThroughput,
    pub write_throughput: JsonThroughput,
    pub total_throughput: JsonThroughput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<JsonLatency>,  // Overall latency (only in final summary, not time-series)
    pub read_latency: JsonLatency,  // Read-specific latency
    pub write_latency: JsonLatency,  // Write-specific latency
    pub errors: u64,
    pub errors_read: u64,
    pub errors_write: u64,
    pub errors_metadata: u64,
    pub resource_utilization: JsonResourceUtil,
    pub metadata_operations: JsonMetadataOps,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<JsonCoverage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_size_verification: Option<JsonBlockSizeVerification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_depth_stats: Option<JsonQueueDepthStats>,
}

/// Queue depth utilization statistics (for async engines)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonQueueDepthStats {
    pub avg_queue_depth: f64,
    pub peak_queue_depth: u64,
    pub configured_queue_depth: usize,
    pub utilization_percent: f64,
}

/// Block size verification data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonBlockSizeVerification {
    pub min_bytes: u64,
    pub max_bytes: u64,
    pub configured_block_size: u64,
}

/// Per-worker statistics (simplified for time-series)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonWorkerStats {
    pub worker_id: usize,
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_iops: u64,  // NEW: IOPS for this interval
    pub write_iops: u64,  // NEW: IOPS for this interval
    pub read_latency_mean: JsonDuration,
    pub write_latency_mean: JsonDuration,
    // Metadata operations
    pub metadata_open_ops: u64,
    pub metadata_close_ops: u64,
    pub metadata_fsync_ops: u64,
}

/// Per-worker statistics for final summary (includes full latency percentiles)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonWorkerStatsFinal {
    pub node_id: String,  // Which node this worker is on
    pub worker_id: usize,  // Worker ID within that node
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub latency: JsonLatency,
}

/// Per-node time-series statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonNodeTimeSeriesStats {
    pub node_id: String,
    pub stats: JsonAggregateStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workers: Option<Vec<JsonWorkerStats>>,  // Per-worker detail for this node (if --json-per-worker)
}

/// Time-series snapshot at a polling interval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonSnapshot {
    pub timestamp: String,
    pub elapsed: JsonDuration,
    pub nodes: Vec<JsonNodeTimeSeriesStats>,
    pub aggregate: JsonAggregateStats,
}

/// Test configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTestConfig {
    pub threads: usize,
    pub block_size: usize,
    pub file_size: u64,
    pub engine: String,
    pub queue_depth: usize,
    pub read_percent: u32,
    pub write_percent: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distribution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zipf_theta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pareto_h: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gaussian_stddev: Option<f64>,
}

/// Test information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTestInfo {
    pub node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    pub start_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<JsonDuration>,
    pub config: JsonTestConfig,
}

/// Complete per-node JSON output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonNodeOutput {
    pub test_info: JsonTestInfo,
    pub time_series: Vec<JsonSnapshot>,
    pub final_summary: JsonFinalSummary,
}

/// Final summary statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonFinalSummary {
    pub total_duration: JsonDuration,
    pub aggregate: JsonAggregateStats,
    pub per_worker: Vec<JsonWorkerStatsFinal>,
}


/// Format duration in human-readable format
fn format_duration_human(d: Duration) -> String {
    let micros = d.as_micros() as u64;
    
    if micros == 0 {
        return "0µs".to_string();
    }
    
    if micros < 1000 {
        format!("{}µs", micros)
    } else if micros < 1_000_000 {
        format!("{:.3}ms", micros as f64 / 1000.0)
    } else if micros < 60_000_000 {
        format!("{:.3}s", micros as f64 / 1_000_000.0)
    } else if micros < 3_600_000_000 {
        format!("{:.2}m", micros as f64 / 60_000_000.0)
    } else {
        format!("{:.2}h", micros as f64 / 3_600_000_000.0)
    }
}

/// Format throughput in human-readable format
fn format_throughput(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 {
        return "0 B/s".to_string();
    }
    
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    
    if bytes_per_sec >= GB {
        format!("{:.2} GB/s", bytes_per_sec as f64 / GB as f64)
    } else if bytes_per_sec >= MB {
        format!("{:.1} MB/s", bytes_per_sec as f64 / MB as f64)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB/s", bytes_per_sec as f64 / KB as f64)
    } else {
        format!("{} B/s", bytes_per_sec)
    }
}

/// Convert WorkerStats to JsonLatency (full percentiles for final summary)
fn extract_latency(stats: &WorkerStats) -> JsonLatency {
    extract_latency_from_histogram(stats.io_latency())
}

/// Extract JsonLatency from a histogram
fn extract_latency_from_histogram(hist: &crate::stats::simple_histogram::SimpleHistogram) -> JsonLatency {
    JsonLatency {
        min: Some(JsonDuration::from_duration(hist.min())),
        max: Some(JsonDuration::from_duration(hist.max())),
        mean: JsonDuration::from_duration(hist.mean()),
        p50: Some(JsonDuration::from_duration(hist.percentile(50.0))),
        p90: Some(JsonDuration::from_duration(hist.percentile(90.0))),
        p95: Some(JsonDuration::from_duration(hist.percentile(95.0))),
        p99: Some(JsonDuration::from_duration(hist.percentile(99.0))),
        p99_9: Some(JsonDuration::from_duration(hist.percentile(99.9))),
    }
}

/// Create JsonLatency with only mean (for time-series snapshots)
fn latency_mean_only(mean_micros: f64) -> JsonLatency {
    JsonLatency {
        min: None,
        max: None,
        mean: JsonDuration {
            micros: mean_micros as u64,
            human: format_duration_human(Duration::from_micros(mean_micros as u64)),
        },
        p50: None,
        p90: None,
        p95: None,
        p99: None,
        p99_9: None,
    }
}

/// Convert WorkerStats to JsonLatencySimple (mean + p99 only)
#[allow(dead_code)]
fn extract_latency_simple(stats: &WorkerStats) -> JsonLatencySimple {
    let hist = stats.io_latency();
    
    JsonLatencySimple {
        mean: JsonDuration::from_duration(hist.mean()),
        p99: JsonDuration::from_duration(hist.percentile(99.0)),
    }
}


/// Extract metadata operation statistics
fn extract_metadata_ops(metadata: &MetadataStats) -> JsonMetadataOps {
    let total_ops = metadata.total_ops();
    
    // Only include latency if there are operations
    let latency = if total_ops > 0 {
        Some(JsonMetadataLatency {
            open: if metadata.open_ops.get() > 0 {
                Some(JsonLatencySimple {
                    mean: JsonDuration::from_duration(metadata.open_latency.mean()),
                    p99: JsonDuration::from_duration(metadata.open_latency.percentile(99.0)),
                })
            } else {
                None
            },
            close: if metadata.close_ops.get() > 0 {
                Some(JsonLatencySimple {
                    mean: JsonDuration::from_duration(metadata.close_latency.mean()),
                    p99: JsonDuration::from_duration(metadata.close_latency.percentile(99.0)),
                })
            } else {
                None
            },
            stat: if metadata.stat_ops.get() > 0 {
                Some(JsonLatencySimple {
                    mean: JsonDuration::from_duration(metadata.stat_latency.mean()),
                    p99: JsonDuration::from_duration(metadata.stat_latency.percentile(99.0)),
                })
            } else {
                None
            },
            setattr: if metadata.setattr_ops.get() > 0 {
                Some(JsonLatencySimple {
                    mean: JsonDuration::from_duration(metadata.setattr_latency.mean()),
                    p99: JsonDuration::from_duration(metadata.setattr_latency.percentile(99.0)),
                })
            } else {
                None
            },
            mkdir: if metadata.mkdir_ops.get() > 0 {
                Some(JsonLatencySimple {
                    mean: JsonDuration::from_duration(metadata.mkdir_latency.mean()),
                    p99: JsonDuration::from_duration(metadata.mkdir_latency.percentile(99.0)),
                })
            } else {
                None
            },
            rmdir: if metadata.rmdir_ops.get() > 0 {
                Some(JsonLatencySimple {
                    mean: JsonDuration::from_duration(metadata.rmdir_latency.mean()),
                    p99: JsonDuration::from_duration(metadata.rmdir_latency.percentile(99.0)),
                })
            } else {
                None
            },
            unlink: if metadata.unlink_ops.get() > 0 {
                Some(JsonLatencySimple {
                    mean: JsonDuration::from_duration(metadata.unlink_latency.mean()),
                    p99: JsonDuration::from_duration(metadata.unlink_latency.percentile(99.0)),
                })
            } else {
                None
            },
            rename: if metadata.rename_ops.get() > 0 {
                Some(JsonLatencySimple {
                    mean: JsonDuration::from_duration(metadata.rename_latency.mean()),
                    p99: JsonDuration::from_duration(metadata.rename_latency.percentile(99.0)),
                })
            } else {
                None
            },
            readdir: if metadata.readdir_ops.get() > 0 {
                Some(JsonLatencySimple {
                    mean: JsonDuration::from_duration(metadata.readdir_latency.mean()),
                    p99: JsonDuration::from_duration(metadata.readdir_latency.percentile(99.0)),
                })
            } else {
                None
            },
            fsync: if metadata.fsync_ops.get() > 0 {
                Some(JsonLatencySimple {
                    mean: JsonDuration::from_duration(metadata.fsync_latency.mean()),
                    p99: JsonDuration::from_duration(metadata.fsync_latency.percentile(99.0)),
                })
            } else {
                None
            },
        })
    } else {
        None
    };
    
    JsonMetadataOps {
        open_ops: metadata.open_ops.get(),
        close_ops: metadata.close_ops.get(),
        stat_ops: metadata.stat_ops.get(),
        setattr_ops: metadata.setattr_ops.get(),
        mkdir_ops: metadata.mkdir_ops.get(),
        rmdir_ops: metadata.rmdir_ops.get(),
        unlink_ops: metadata.unlink_ops.get(),
        rename_ops: metadata.rename_ops.get(),
        readdir_ops: metadata.readdir_ops.get(),
        fsync_ops: metadata.fsync_ops.get(),
        total_ops,
        latency,
    }
}

/// Extract resource utilization statistics
fn extract_resource_util(resource_stats: Option<ResourceStats>, num_workers: usize) -> JsonResourceUtil {
    if let Some(stats) = resource_stats {
        let cpu_percent_total = stats.cpu_percent;
        let cpu_percent_per_worker = cpu_percent_total / num_workers as f64;
        let num_system_cpus = crate::util::resource::ResourceSnapshot::num_cpus();
        let cpu_percent_system = if let Some(cpus) = num_system_cpus {
            cpu_percent_total / cpus as f64  // Don't multiply by 100 - already a percentage
        } else {
            0.0
        };
        
        JsonResourceUtil {
            cpu_percent_total,
            cpu_percent_per_worker,
            cpu_percent_system,
            num_workers,
            num_system_cpus,
            memory_bytes: stats.memory_bytes,
            memory_human: format_memory(stats.memory_bytes),
        }
    } else {
        JsonResourceUtil {
            cpu_percent_total: 0.0,
            cpu_percent_per_worker: 0.0,
            cpu_percent_system: 0.0,
            num_workers,
            num_system_cpus: crate::util::resource::ResourceSnapshot::num_cpus(),
            memory_bytes: 0,
            memory_human: "0 B".to_string(),
        }
    }
}

/// Format memory in human-readable format
fn format_memory(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}


/// Convert WorkerStats to JsonAggregateStats
pub fn stats_to_json_aggregate(
    stats: &WorkerStats,
    duration: Duration,
    total_blocks: Option<u64>,
    include_coverage: bool,
    configured_block_size: u64,
    configured_queue_depth: usize,
    num_workers: usize,
) -> JsonAggregateStats {
    let read_ops = stats.read_ops();
    let write_ops = stats.write_ops();
    let read_bytes = stats.read_bytes();
    let write_bytes = stats.write_bytes();
    
    // Use milliseconds for precision
    let duration_ms = duration.as_millis() as f64;
    let read_iops = if duration_ms > 0.0 {
        ((read_ops as f64 * 1000.0) / duration_ms) as u64
    } else {
        0
    };
    let write_iops = if duration_ms > 0.0 {
        ((write_ops as f64 * 1000.0) / duration_ms) as u64
    } else {
        0
    };
    
    let read_throughput_bps = if duration_ms > 0.0 {
        ((read_bytes as f64 * 1000.0) / duration_ms) as u64
    } else {
        0
    };
    let write_throughput_bps = if duration_ms > 0.0 {
        ((write_bytes as f64 * 1000.0) / duration_ms) as u64
    } else {
        0
    };
    
    let coverage = if include_coverage && total_blocks.is_some() {
        let total_blocks = total_blocks.unwrap();
        Some(JsonCoverage {
            unique_blocks: stats.unique_blocks_count(),
            total_blocks,
            coverage_percent: stats.coverage_percent(total_blocks),
            rewrite_percent: stats.rewrite_percent(),
        })
    } else {
        None
    };
    
    let block_size_verification = if stats.max_bytes_per_op() > 0 {
        Some(JsonBlockSizeVerification {
            min_bytes: stats.min_bytes_per_op(),
            max_bytes: stats.max_bytes_per_op(),
            configured_block_size,
        })
    } else {
        None
    };
    
    let queue_depth_stats = if configured_queue_depth > 1 {
        // Only include for async engines (QD > 1)
        let avg_qd = stats.avg_queue_depth();
        let peak_qd = stats.peak_queue_depth();
        let utilization = if configured_queue_depth > 0 && avg_qd > 0.0 {
            (avg_qd / configured_queue_depth as f64) * 100.0
        } else {
            0.0
        };
        
        Some(JsonQueueDepthStats {
            avg_queue_depth: avg_qd,
            peak_queue_depth: peak_qd,
            configured_queue_depth,
            utilization_percent: utilization,
        })
    } else {
        None
    };
    
    JsonAggregateStats {
        read_ops,
        write_ops,
        read_bytes,
        write_bytes,
        total_ops: read_ops + write_ops,
        total_bytes: read_bytes + write_bytes,
        read_iops,
        write_iops,
        total_iops: read_iops + write_iops,
        read_throughput: JsonThroughput::new(read_throughput_bps),
        write_throughput: JsonThroughput::new(write_throughput_bps),
        total_throughput: JsonThroughput::new(read_throughput_bps + write_throughput_bps),
        latency: Some(extract_latency(stats)),  // Include in final summary
        read_latency: extract_latency_from_histogram(stats.read_latency()),
        write_latency: extract_latency_from_histogram(stats.write_latency()),
        errors: stats.errors(),
        errors_read: stats.errors_read(),
        errors_write: stats.errors_write(),
        errors_metadata: stats.errors_metadata(),
        resource_utilization: extract_resource_util(stats.resource_stats(), num_workers),
        metadata_operations: extract_metadata_ops(&stats.metadata),
        coverage,
        block_size_verification,
        queue_depth_stats,
    }
}

/// Write JSON output to file
pub fn write_json_output(
    output_path: &Path,
    node_output: &JsonNodeOutput,
    pretty: bool,
) -> Result<()> {
    let file = File::create(output_path)?;
    
    if pretty {
        serde_json::to_writer_pretty(file, node_output)?;
    } else {
        serde_json::to_writer(file, node_output)?;
    }
    
    Ok(())
}


/// Extract metadata latency from StatsSnapshot histograms
#[allow(dead_code)]
fn extract_metadata_latency_from_snapshot(snapshot: &crate::worker::StatsSnapshot) -> Option<JsonMetadataLatency> {
    use crate::stats::simple_histogram::SimpleHistogram;
    
    // Helper to extract latency if histogram has samples
    let extract_if_present = |hist: &SimpleHistogram| -> Option<JsonLatencySimple> {
        if hist.len() > 0 {
            Some(JsonLatencySimple {
                mean: JsonDuration::from_duration(hist.mean()),
                p99: JsonDuration::from_duration(hist.percentile(99.0)),
            })
        } else {
            None
        }
    };
    
    let has_any_ops = snapshot.metadata_open_ops > 0
        || snapshot.metadata_close_ops > 0
        || snapshot.metadata_stat_ops > 0
        || snapshot.metadata_setattr_ops > 0
        || snapshot.metadata_mkdir_ops > 0
        || snapshot.metadata_rmdir_ops > 0
        || snapshot.metadata_unlink_ops > 0
        || snapshot.metadata_rename_ops > 0
        || snapshot.metadata_readdir_ops > 0
        || snapshot.metadata_fsync_ops > 0;
    
    if !has_any_ops {
        return None;
    }
    
    Some(JsonMetadataLatency {
        open: extract_if_present(&snapshot.metadata_open_latency),
        close: extract_if_present(&snapshot.metadata_close_latency),
        stat: extract_if_present(&snapshot.metadata_stat_latency),
        setattr: extract_if_present(&snapshot.metadata_setattr_latency),
        mkdir: extract_if_present(&snapshot.metadata_mkdir_latency),
        rmdir: extract_if_present(&snapshot.metadata_rmdir_latency),
        unlink: extract_if_present(&snapshot.metadata_unlink_latency),
        rename: extract_if_present(&snapshot.metadata_rename_latency),
        readdir: extract_if_present(&snapshot.metadata_readdir_latency),
        fsync: extract_if_present(&snapshot.metadata_fsync_latency),
    })
}

/// Extract metadata operations from StatsSnapshot
#[allow(dead_code)]
fn extract_metadata_ops_from_snapshot(snapshot: &crate::worker::StatsSnapshot) -> JsonMetadataOps {
    let total_ops = snapshot.metadata_open_ops
        + snapshot.metadata_close_ops
        + snapshot.metadata_stat_ops
        + snapshot.metadata_setattr_ops
        + snapshot.metadata_mkdir_ops
        + snapshot.metadata_rmdir_ops
        + snapshot.metadata_unlink_ops
        + snapshot.metadata_rename_ops
        + snapshot.metadata_readdir_ops
        + snapshot.metadata_fsync_ops;
    
    JsonMetadataOps {
        open_ops: snapshot.metadata_open_ops,
        close_ops: snapshot.metadata_close_ops,
        stat_ops: snapshot.metadata_stat_ops,
        setattr_ops: snapshot.metadata_setattr_ops,
        mkdir_ops: snapshot.metadata_mkdir_ops,
        rmdir_ops: snapshot.metadata_rmdir_ops,
        unlink_ops: snapshot.metadata_unlink_ops,
        rename_ops: snapshot.metadata_rename_ops,
        readdir_ops: snapshot.metadata_readdir_ops,
        fsync_ops: snapshot.metadata_fsync_ops,
        total_ops,
        latency: extract_metadata_latency_from_snapshot(snapshot),
    }
}


/// Aggregated snapshot from multiple workers
///
/// This structure is created by the monitoring thread by aggregating
/// StatsSnapshot data from all workers. It's used for JSON/CSV time-series output.
#[derive(Debug, Clone)]
pub struct AggregatedSnapshot {
    pub timestamp: std::time::SystemTime,
    pub elapsed: Duration,
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub errors: u64,
    
    // IO latency histograms (merged from all workers)
    pub avg_latency_us: f64,  // Overall average (for backward compatibility)
    pub read_latency: crate::stats::simple_histogram::SimpleHistogram,
    pub write_latency: crate::stats::simple_histogram::SimpleHistogram,
    
    // Metadata counters
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
    
    // Metadata latency histograms (merged from all workers)
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
    
    // Per-worker snapshots (optional, only when --json-per-worker is enabled)
    pub per_worker: Option<Vec<crate::worker::StatsSnapshot>>,
}

impl AggregatedSnapshot {
    /// Convert to StatsSnapshot (for CSV per-worker output)
    pub fn to_stats_snapshot(&self) -> crate::worker::StatsSnapshot {
        crate::worker::StatsSnapshot {
            read_ops: self.read_ops,
            write_ops: self.write_ops,
            read_bytes: self.read_bytes,
            write_bytes: self.write_bytes,
            errors: self.errors,
            avg_latency_us: self.avg_latency_us,
            read_latency: self.read_latency.clone(),
            write_latency: self.write_latency.clone(),
            metadata_open_ops: self.metadata_open_ops,
            metadata_close_ops: self.metadata_close_ops,
            metadata_stat_ops: self.metadata_stat_ops,
            metadata_setattr_ops: self.metadata_setattr_ops,
            metadata_mkdir_ops: self.metadata_mkdir_ops,
            metadata_rmdir_ops: self.metadata_rmdir_ops,
            metadata_unlink_ops: self.metadata_unlink_ops,
            metadata_rename_ops: self.metadata_rename_ops,
            metadata_readdir_ops: self.metadata_readdir_ops,
            metadata_fsync_ops: self.metadata_fsync_ops,
            metadata_open_latency: self.metadata_open_latency.clone(),
            metadata_close_latency: self.metadata_close_latency.clone(),
            metadata_stat_latency: self.metadata_stat_latency.clone(),
            metadata_setattr_latency: self.metadata_setattr_latency.clone(),
            metadata_mkdir_latency: self.metadata_mkdir_latency.clone(),
            metadata_rmdir_latency: self.metadata_rmdir_latency.clone(),
            metadata_unlink_latency: self.metadata_unlink_latency.clone(),
            metadata_rename_latency: self.metadata_rename_latency.clone(),
            metadata_readdir_latency: self.metadata_readdir_latency.clone(),
            metadata_fsync_latency: self.metadata_fsync_latency.clone(),
        }
    }
    
    /// Create from multiple worker snapshots
    pub fn from_worker_snapshots(
        snapshots: &[crate::worker::StatsSnapshot],
        elapsed: Duration,
        include_per_worker: bool,
    ) -> Self {
        use crate::stats::simple_histogram::SimpleHistogram;
        
        let mut total_read_ops = 0u64;
        let mut total_write_ops = 0u64;
        let mut total_read_bytes = 0u64;
        let mut total_write_bytes = 0u64;
        let mut total_errors = 0u64;
        let mut sum_latency = 0.0;
        let mut count = 0;
        
        // Metadata counters
        let mut total_metadata_open = 0u64;
        let mut total_metadata_close = 0u64;
        let mut total_metadata_stat = 0u64;
        let mut total_metadata_setattr = 0u64;
        let mut total_metadata_mkdir = 0u64;
        let mut total_metadata_rmdir = 0u64;
        let mut total_metadata_unlink = 0u64;
        let mut total_metadata_rename = 0u64;
        let mut total_metadata_readdir = 0u64;
        let mut total_metadata_fsync = 0u64;
        
        // Metadata histograms (will merge)
        let mut merged_read_latency = SimpleHistogram::new();
        let mut merged_write_latency = SimpleHistogram::new();
        let mut merged_open_latency = SimpleHistogram::new();
        let mut merged_close_latency = SimpleHistogram::new();
        let mut merged_stat_latency = SimpleHistogram::new();
        let mut merged_setattr_latency = SimpleHistogram::new();
        let mut merged_mkdir_latency = SimpleHistogram::new();
        let mut merged_rmdir_latency = SimpleHistogram::new();
        let mut merged_unlink_latency = SimpleHistogram::new();
        let mut merged_rename_latency = SimpleHistogram::new();
        let mut merged_readdir_latency = SimpleHistogram::new();
        let mut merged_fsync_latency = SimpleHistogram::new();
        
        for snapshot in snapshots.iter() {
            total_read_ops += snapshot.read_ops;
            total_write_ops += snapshot.write_ops;
            total_read_bytes += snapshot.read_bytes;
            total_write_bytes += snapshot.write_bytes;
            total_errors += snapshot.errors;
            if snapshot.avg_latency_us > 0.0 {
                sum_latency += snapshot.avg_latency_us;
                count += 1;
            }
            
            // Aggregate metadata counters
            total_metadata_open += snapshot.metadata_open_ops;
            total_metadata_close += snapshot.metadata_close_ops;
            total_metadata_stat += snapshot.metadata_stat_ops;
            total_metadata_setattr += snapshot.metadata_setattr_ops;
            total_metadata_mkdir += snapshot.metadata_mkdir_ops;
            total_metadata_rmdir += snapshot.metadata_rmdir_ops;
            total_metadata_unlink += snapshot.metadata_unlink_ops;
            total_metadata_rename += snapshot.metadata_rename_ops;
            total_metadata_readdir += snapshot.metadata_readdir_ops;
            total_metadata_fsync += snapshot.metadata_fsync_ops;
            
            // Merge metadata histograms
            merged_read_latency.merge(&snapshot.read_latency);
            merged_write_latency.merge(&snapshot.write_latency);
            merged_open_latency.merge(&snapshot.metadata_open_latency);
            merged_close_latency.merge(&snapshot.metadata_close_latency);
            merged_stat_latency.merge(&snapshot.metadata_stat_latency);
            merged_setattr_latency.merge(&snapshot.metadata_setattr_latency);
            merged_mkdir_latency.merge(&snapshot.metadata_mkdir_latency);
            merged_rmdir_latency.merge(&snapshot.metadata_rmdir_latency);
            merged_unlink_latency.merge(&snapshot.metadata_unlink_latency);
            merged_rename_latency.merge(&snapshot.metadata_rename_latency);
            merged_readdir_latency.merge(&snapshot.metadata_readdir_latency);
            merged_fsync_latency.merge(&snapshot.metadata_fsync_latency);
        }
        
        let avg_latency_us = if count > 0 {
            sum_latency / count as f64
        } else {
            0.0
        };
        
        let per_worker = if include_per_worker {
            Some(snapshots.to_vec())
        } else {
            None
        };
        
        Self {
            timestamp: std::time::SystemTime::now(),
            elapsed,
            read_ops: total_read_ops,
            write_ops: total_write_ops,
            read_bytes: total_read_bytes,
            write_bytes: total_write_bytes,
            errors: total_errors,
            avg_latency_us,
            read_latency: merged_read_latency,
            write_latency: merged_write_latency,
            metadata_open_ops: total_metadata_open,
            metadata_close_ops: total_metadata_close,
            metadata_stat_ops: total_metadata_stat,
            metadata_setattr_ops: total_metadata_setattr,
            metadata_mkdir_ops: total_metadata_mkdir,
            metadata_rmdir_ops: total_metadata_rmdir,
            metadata_unlink_ops: total_metadata_unlink,
            metadata_rename_ops: total_metadata_rename,
            metadata_readdir_ops: total_metadata_readdir,
            metadata_fsync_ops: total_metadata_fsync,
            metadata_open_latency: merged_open_latency,
            metadata_close_latency: merged_close_latency,
            metadata_stat_latency: merged_stat_latency,
            metadata_setattr_latency: merged_setattr_latency,
            metadata_mkdir_latency: merged_mkdir_latency,
            metadata_rmdir_latency: merged_rmdir_latency,
            metadata_unlink_latency: merged_unlink_latency,
            metadata_rename_latency: merged_rename_latency,
            metadata_readdir_latency: merged_readdir_latency,
            metadata_fsync_latency: merged_fsync_latency,
            per_worker,
        }
    }
}


/// Extract metadata latency from aggregated histograms
fn extract_metadata_latency_from_aggregated(snapshot: &AggregatedSnapshot) -> Option<JsonMetadataLatency> {
    use crate::stats::simple_histogram::SimpleHistogram;
    
    // Helper to extract latency if histogram has samples
    let extract_if_present = |hist: &SimpleHistogram| -> Option<JsonLatencySimple> {
        if hist.len() > 0 {
            Some(JsonLatencySimple {
                mean: JsonDuration::from_duration(hist.mean()),
                p99: JsonDuration::from_duration(hist.percentile(99.0)),
            })
        } else {
            None
        }
    };
    
    let total_ops = snapshot.metadata_open_ops
        + snapshot.metadata_close_ops
        + snapshot.metadata_stat_ops
        + snapshot.metadata_setattr_ops
        + snapshot.metadata_mkdir_ops
        + snapshot.metadata_rmdir_ops
        + snapshot.metadata_unlink_ops
        + snapshot.metadata_rename_ops
        + snapshot.metadata_readdir_ops
        + snapshot.metadata_fsync_ops;
    
    if total_ops == 0 {
        return None;
    }
    
    Some(JsonMetadataLatency {
        open: extract_if_present(&snapshot.metadata_open_latency),
        close: extract_if_present(&snapshot.metadata_close_latency),
        stat: extract_if_present(&snapshot.metadata_stat_latency),
        setattr: extract_if_present(&snapshot.metadata_setattr_latency),
        mkdir: extract_if_present(&snapshot.metadata_mkdir_latency),
        rmdir: extract_if_present(&snapshot.metadata_rmdir_latency),
        unlink: extract_if_present(&snapshot.metadata_unlink_latency),
        rename: extract_if_present(&snapshot.metadata_rename_latency),
        readdir: extract_if_present(&snapshot.metadata_readdir_latency),
        fsync: extract_if_present(&snapshot.metadata_fsync_latency),
    })
}

/// Extract metadata operations from AggregatedSnapshot
fn extract_metadata_ops_from_aggregated(snapshot: &AggregatedSnapshot) -> JsonMetadataOps {
    let total_ops = snapshot.metadata_open_ops
        + snapshot.metadata_close_ops
        + snapshot.metadata_stat_ops
        + snapshot.metadata_setattr_ops
        + snapshot.metadata_mkdir_ops
        + snapshot.metadata_rmdir_ops
        + snapshot.metadata_unlink_ops
        + snapshot.metadata_rename_ops
        + snapshot.metadata_readdir_ops
        + snapshot.metadata_fsync_ops;
    
    JsonMetadataOps {
        open_ops: snapshot.metadata_open_ops,
        close_ops: snapshot.metadata_close_ops,
        stat_ops: snapshot.metadata_stat_ops,
        setattr_ops: snapshot.metadata_setattr_ops,
        mkdir_ops: snapshot.metadata_mkdir_ops,
        rmdir_ops: snapshot.metadata_rmdir_ops,
        unlink_ops: snapshot.metadata_unlink_ops,
        rename_ops: snapshot.metadata_rename_ops,
        readdir_ops: snapshot.metadata_readdir_ops,
        fsync_ops: snapshot.metadata_fsync_ops,
        total_ops,
        latency: extract_metadata_latency_from_aggregated(snapshot),
    }
}


/// Format SystemTime as ISO 8601 string
fn format_timestamp(time: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    
    let duration_since_epoch = time.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0));
    
    let total_secs = duration_since_epoch.as_secs();
    
    // Calculate date/time components
    const SECS_PER_DAY: u64 = 86400;
    const SECS_PER_HOUR: u64 = 3600;
    const SECS_PER_MINUTE: u64 = 60;
    
    // Days since Unix epoch (1970-01-01)
    let days_since_epoch = total_secs / SECS_PER_DAY;
    let remaining_secs = total_secs % SECS_PER_DAY;
    
    // Time components
    let hours = remaining_secs / SECS_PER_HOUR;
    let minutes = (remaining_secs % SECS_PER_HOUR) / SECS_PER_MINUTE;
    let seconds = remaining_secs % SECS_PER_MINUTE;
    
    // Simple date calculation (approximate, good enough for logging)
    // For production use, consider adding chrono crate
    let year = 1970 + (days_since_epoch / 365);
    let day_of_year = days_since_epoch % 365;
    let month = (day_of_year / 30) + 1;
    let day = (day_of_year % 30) + 1;
    
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}


/// Build JsonTestConfig from Config
pub fn build_test_config(config: &crate::config::Config) -> JsonTestConfig {
    let workload = &config.workload;
    
    // Extract distribution parameters
    let (distribution, zipf_theta, pareto_h, gaussian_stddev) = match &workload.distribution {
        crate::config::workload::DistributionType::Uniform => (None, None, None, None),
        crate::config::workload::DistributionType::Zipf { theta } => {
            (Some("zipf".to_string()), Some(*theta), None, None)
        }
        crate::config::workload::DistributionType::Pareto { h } => {
            (Some("pareto".to_string()), None, Some(*h), None)
        }
        crate::config::workload::DistributionType::Gaussian { stddev, center: _ } => {
            (Some("gaussian".to_string()), None, None, Some(*stddev))
        }
    };
    
    // Get file size from first target (if available)
    let file_size = config.targets.first()
        .and_then(|t| t.file_size)
        .unwrap_or(0);
    
    JsonTestConfig {
        threads: config.workers.threads,
        block_size: workload.block_size as usize,
        file_size,
        engine: format!("{:?}", workload.engine).to_lowercase(),
        queue_depth: workload.queue_depth,
        read_percent: workload.read_percent as u32,
        write_percent: workload.write_percent as u32,
        distribution,
        zipf_theta,
        pareto_h,
        gaussian_stddev,
    }
}

/// Build JsonTestInfo
pub fn build_test_info(
    node_id: String,
    hostname: Option<String>,
    start_time: std::time::SystemTime,
    end_time: Option<std::time::SystemTime>,
    duration: Option<Duration>,
    config: &crate::config::Config,
) -> JsonTestInfo {
    JsonTestInfo {
        node_id,
        hostname,
        start_time: format_timestamp(start_time),
        end_time: end_time.map(format_timestamp),
        duration: duration.map(JsonDuration::from_duration),
        config: build_test_config(config),
    }
}


/// Convert WorkerStats to JsonWorkerStatsFinal (for final summary)
pub fn worker_stats_to_json_final(node_id: String, worker_id: usize, stats: &WorkerStats) -> JsonWorkerStatsFinal {
    JsonWorkerStatsFinal {
        node_id,
        worker_id,
        read_ops: stats.read_ops(),
        write_ops: stats.write_ops(),
        read_bytes: stats.read_bytes(),
        write_bytes: stats.write_bytes(),
        latency: extract_latency(stats),
    }
}


/// Build JsonSnapshot from per-node snapshots
/// This creates the new time-series structure with per-node visibility
pub fn build_json_snapshot_with_nodes(
    node_snapshots: &[(String, &AggregatedSnapshot)],  // (node_id, snapshot) pairs
    interval_duration: Duration,
    node_resource_stats: &[(String, Option<ResourceStats>)],  // (node_id, resource_stats) pairs
    per_worker_snapshots: Option<Vec<(String, Vec<AggregatedSnapshot>)>>,  // (node_id, workers) pairs (NEW)
    total_blocks: Option<u64>,
    num_workers_per_node: usize,  // NEW: number of workers per node
) -> JsonSnapshot {
    if node_snapshots.is_empty() {
        // Return empty snapshot if no data
        // Create empty snapshot for metadata extraction
        let empty_snapshot = AggregatedSnapshot {
            timestamp: std::time::SystemTime::now(),
            elapsed: Duration::from_secs(0),
            read_ops: 0,
            write_ops: 0,
            read_bytes: 0,
            write_bytes: 0,
            errors: 0,
            avg_latency_us: 0.0,
            read_latency: SimpleHistogram::new(),
            write_latency: SimpleHistogram::new(),
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
            metadata_open_latency: SimpleHistogram::new(),
            metadata_close_latency: SimpleHistogram::new(),
            metadata_stat_latency: SimpleHistogram::new(),
            metadata_setattr_latency: SimpleHistogram::new(),
            metadata_mkdir_latency: SimpleHistogram::new(),
            metadata_rmdir_latency: SimpleHistogram::new(),
            metadata_unlink_latency: SimpleHistogram::new(),
            metadata_rename_latency: SimpleHistogram::new(),
            metadata_readdir_latency: SimpleHistogram::new(),
            metadata_fsync_latency: SimpleHistogram::new(),
            per_worker: None,
        };
        
        return JsonSnapshot {
            timestamp: format_timestamp(std::time::SystemTime::now()),
            elapsed: JsonDuration::from_duration(Duration::from_secs(0)),
            nodes: Vec::new(),
            aggregate: JsonAggregateStats {
                read_ops: 0,
                write_ops: 0,
                read_bytes: 0,
                write_bytes: 0,
                total_ops: 0,
                total_bytes: 0,
                read_iops: 0,
                write_iops: 0,
                total_iops: 0,
                read_throughput: JsonThroughput::new(0),
                write_throughput: JsonThroughput::new(0),
                total_throughput: JsonThroughput::new(0),
                latency: None,
                read_latency: latency_mean_only(0.0),
                write_latency: latency_mean_only(0.0),
                errors: 0,
                errors_read: 0,
                errors_write: 0,
                errors_metadata: 0,
                resource_utilization: extract_resource_util(None, 0),
                metadata_operations: extract_metadata_ops_from_aggregated(&empty_snapshot),
                coverage: None,
                block_size_verification: None,
                queue_depth_stats: None,
            },
        };
    }
    
    // Use timestamp and elapsed from first node (all should be synchronized)
    let timestamp = format_timestamp(node_snapshots[0].1.timestamp);
    let elapsed = JsonDuration::from_duration(node_snapshots[0].1.elapsed);
    
    // Build per-node stats
    let nodes: Vec<JsonNodeTimeSeriesStats> = node_snapshots.iter()
        .map(|(node_id, snapshot)| {
            // Find resource stats for this node
            let resource_stats = node_resource_stats.iter()
                .find(|(id, _)| id == node_id)
                .and_then(|(_, stats)| *stats);
            
            // Determine number of workers for this node
            let num_workers = if let Some(ref per_worker_data) = per_worker_snapshots {
                per_worker_data.iter()
                    .find(|(nid, _)| nid == node_id)
                    .map(|(_, workers)| workers.len())
                    .unwrap_or(num_workers_per_node)  // Use parameter as fallback
            } else {
                num_workers_per_node  // Use parameter when no per-worker data
            };
            
            // Convert snapshot to aggregate stats (without coverage in time-series)
            let stats = snapshot_to_aggregate_stats(snapshot, interval_duration, resource_stats, total_blocks, false, num_workers);
            
            // Get per-worker data for this node (NEW approach)
            let workers = if let Some(ref per_worker_data) = per_worker_snapshots {
                // Find workers for this node
                per_worker_data.iter()
                    .find(|(nid, _)| nid == node_id)
                    .map(|(_, worker_snapshots)| {
                        worker_snapshots.iter().enumerate().map(|(worker_id, ws)| {
                            // Calculate IOPS for this worker
                            let duration_ms = interval_duration.as_millis() as f64;
                            let read_iops = if duration_ms > 0.0 {
                                ((ws.read_ops as f64 * 1000.0) / duration_ms) as u64
                            } else {
                                0
                            };
                            let write_iops = if duration_ms > 0.0 {
                                ((ws.write_ops as f64 * 1000.0) / duration_ms) as u64
                            } else {
                                0
                            };
                            
                            let read_lat = if ws.read_latency.len() > 0 {
                                ws.read_latency.mean().as_micros() as u64
                            } else {
                                0
                            };
                            let write_lat = if ws.write_latency.len() > 0 {
                                ws.write_latency.mean().as_micros() as u64
                            } else {
                                0
                            };
                            
                            JsonWorkerStats {
                                worker_id,
                                read_ops: ws.read_ops,
                                write_ops: ws.write_ops,
                                read_bytes: ws.read_bytes,
                                write_bytes: ws.write_bytes,
                                read_iops,
                                write_iops,
                                read_latency_mean: JsonDuration {
                                    micros: read_lat,
                                    human: format_duration_human(Duration::from_micros(read_lat)),
                                },
                                write_latency_mean: JsonDuration {
                                    micros: write_lat,
                                    human: format_duration_human(Duration::from_micros(write_lat)),
                                },
                                metadata_open_ops: ws.metadata_open_ops,
                                metadata_close_ops: ws.metadata_close_ops,
                                metadata_fsync_ops: ws.metadata_fsync_ops,
                            }
                        }).collect()
                    })
            } else {
                None
            };
            
            JsonNodeTimeSeriesStats {
                node_id: node_id.clone(),
                stats,
                workers,
            }
        })
        .collect();
    
    // Build aggregate by merging all nodes
    let aggregate = merge_node_stats(&nodes, interval_duration);
    
    JsonSnapshot {
        timestamp,
        elapsed,
        nodes,
        aggregate,
    }
}

/// Helper: Convert AggregatedSnapshot to JsonAggregateStats
fn snapshot_to_aggregate_stats(
    snapshot: &AggregatedSnapshot,
    interval_duration: Duration,
    resource_stats: Option<ResourceStats>,
    total_blocks: Option<u64>,
    include_coverage: bool,
    num_workers: usize,
) -> JsonAggregateStats {
    // Use milliseconds for precision
    let duration_ms = interval_duration.as_millis() as f64;
    
    // Calculate instantaneous IOPS (operations in this interval)
    let read_iops = if duration_ms > 0.0 {
        ((snapshot.read_ops as f64 * 1000.0) / duration_ms) as u64
    } else {
        0
    };
    let write_iops = if duration_ms > 0.0 {
        ((snapshot.write_ops as f64 * 1000.0) / duration_ms) as u64
    } else {
        0
    };
    
    // Calculate instantaneous throughput
    let read_throughput_bps = if duration_ms > 0.0 {
        ((snapshot.read_bytes as f64 * 1000.0) / duration_ms) as u64
    } else {
        0
    };
    let write_throughput_bps = if duration_ms > 0.0 {
        ((snapshot.write_bytes as f64 * 1000.0) / duration_ms) as u64
    } else {
        0
    };
    
    let read_latency = latency_mean_only(
        if snapshot.read_latency.len() > 0 {
            snapshot.read_latency.mean().as_micros() as f64
        } else {
            0.0
        }
    );
    let write_latency = latency_mean_only(
        if snapshot.write_latency.len() > 0 {
            snapshot.write_latency.mean().as_micros() as f64
        } else {
            0.0
        }
    );
    
    let coverage = if include_coverage && total_blocks.is_some() {
        None  // Coverage only in final summary
    } else {
        None
    };
    
    JsonAggregateStats {
        read_ops: snapshot.read_ops,
        write_ops: snapshot.write_ops,
        read_bytes: snapshot.read_bytes,
        write_bytes: snapshot.write_bytes,
        total_ops: snapshot.read_ops + snapshot.write_ops,
        total_bytes: snapshot.read_bytes + snapshot.write_bytes,
        read_iops,
        write_iops,
        total_iops: read_iops + write_iops,
        read_throughput: JsonThroughput::new(read_throughput_bps),
        write_throughput: JsonThroughput::new(write_throughput_bps),
        total_throughput: JsonThroughput::new(read_throughput_bps + write_throughput_bps),
        latency: None,
        read_latency,
        write_latency,
        errors: snapshot.errors,
        errors_read: 0,
        errors_write: 0,
        errors_metadata: 0,
        resource_utilization: extract_resource_util(resource_stats, num_workers),
        metadata_operations: extract_metadata_ops_from_aggregated(snapshot),
        coverage,
        block_size_verification: None,
        queue_depth_stats: None,
    }
}

/// Helper: Merge per-node stats into aggregate
fn merge_node_stats(nodes: &[JsonNodeTimeSeriesStats], _interval_duration: Duration) -> JsonAggregateStats {
    if nodes.is_empty() {
        // Create empty snapshot for metadata extraction
        let empty_snapshot = AggregatedSnapshot {
            timestamp: std::time::SystemTime::now(),
            elapsed: Duration::from_secs(0),
            read_ops: 0,
            write_ops: 0,
            read_bytes: 0,
            write_bytes: 0,
            errors: 0,
            avg_latency_us: 0.0,
            read_latency: SimpleHistogram::new(),
            write_latency: SimpleHistogram::new(),
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
            metadata_open_latency: SimpleHistogram::new(),
            metadata_close_latency: SimpleHistogram::new(),
            metadata_stat_latency: SimpleHistogram::new(),
            metadata_setattr_latency: SimpleHistogram::new(),
            metadata_mkdir_latency: SimpleHistogram::new(),
            metadata_rmdir_latency: SimpleHistogram::new(),
            metadata_unlink_latency: SimpleHistogram::new(),
            metadata_rename_latency: SimpleHistogram::new(),
            metadata_readdir_latency: SimpleHistogram::new(),
            metadata_fsync_latency: SimpleHistogram::new(),
            per_worker: None,
        };
        
        return JsonAggregateStats {
            read_ops: 0,
            write_ops: 0,
            read_bytes: 0,
            write_bytes: 0,
            total_ops: 0,
            total_bytes: 0,
            read_iops: 0,
            write_iops: 0,
            total_iops: 0,
            read_throughput: JsonThroughput::new(0),
            write_throughput: JsonThroughput::new(0),
            total_throughput: JsonThroughput::new(0),
            latency: None,
            read_latency: latency_mean_only(0.0),
            write_latency: latency_mean_only(0.0),
            errors: 0,
            errors_read: 0,
            errors_write: 0,
            errors_metadata: 0,
            resource_utilization: extract_resource_util(None, 0),
            metadata_operations: extract_metadata_ops_from_aggregated(&empty_snapshot),
            coverage: None,
            block_size_verification: None,
            queue_depth_stats: None,
        };
    }
    
    // Sum up all node stats
    let mut aggregate = nodes[0].stats.clone();
    
    for node in &nodes[1..] {
        aggregate.read_ops += node.stats.read_ops;
        aggregate.write_ops += node.stats.write_ops;
        aggregate.read_bytes += node.stats.read_bytes;
        aggregate.write_bytes += node.stats.write_bytes;
        aggregate.total_ops += node.stats.total_ops;
        aggregate.total_bytes += node.stats.total_bytes;
        aggregate.read_iops += node.stats.read_iops;
        aggregate.write_iops += node.stats.write_iops;
        aggregate.total_iops += node.stats.total_iops;
        aggregate.errors += node.stats.errors;
        aggregate.errors_read += node.stats.errors_read;
        aggregate.errors_write += node.stats.errors_write;
        aggregate.errors_metadata += node.stats.errors_metadata;
        
        // Merge throughput
        aggregate.read_throughput = JsonThroughput::new(
            aggregate.read_throughput.bytes_per_sec + node.stats.read_throughput.bytes_per_sec
        );
        aggregate.write_throughput = JsonThroughput::new(
            aggregate.write_throughput.bytes_per_sec + node.stats.write_throughput.bytes_per_sec
        );
        aggregate.total_throughput = JsonThroughput::new(
            aggregate.total_throughput.bytes_per_sec + node.stats.total_throughput.bytes_per_sec
        );
        
        // Average latencies (weighted by operation count)
        // For now, just use simple average - could be improved with weighted average
    }
    
    aggregate
}


/// Build complete JsonNodeOutput
pub fn build_node_output(
    node_id: String,
    hostname: Option<String>,
    start_time: std::time::SystemTime,
    end_time: std::time::SystemTime,
    test_duration: Duration,
    config: &crate::config::Config,
    time_series_snapshots: Vec<AggregatedSnapshot>,
    time_series_resource_stats: Vec<ResourceStats>,  // Per-snapshot resource stats
    per_worker_time_series: Vec<Vec<AggregatedSnapshot>>,  // timestamp → workers (NEW)
    final_stats: &WorkerStats,
    per_worker_stats: &[(usize, &WorkerStats)],
    total_blocks: Option<u64>,
) -> JsonNodeOutput {
    // Build test info
    let test_info = build_test_info(
        node_id.clone(),
        hostname,
        start_time,
        Some(end_time),
        Some(test_duration),
        config,
    );
    
    // Convert time-series snapshots, skipping the first one (startup noise)
    // For single-node output, create nodes array with single entry
    let time_series: Vec<JsonSnapshot> = time_series_snapshots.iter()
        .skip(1)  // Skip first snapshot (arrives before workers have data)
        .enumerate()
        .map(|(i, snapshot)| {
            // Get resource stats for this snapshot (i+1 because we skipped first)
            let resource_stats = time_series_resource_stats.get(i + 1).copied();
            
            // Build per-node data (single node for this output)
            let node_snapshots = vec![(node_id.clone(), snapshot)];
            let node_resource_stats = vec![(node_id.clone(), resource_stats)];
            
            // Get per-worker snapshots for this timestamp (if enabled)
            let workers_at_timestamp = if i + 1 < per_worker_time_series.len() && !per_worker_time_series.is_empty() {
                Some(vec![(node_id.clone(), per_worker_time_series[i + 1].clone())])
            } else {
                None
            };
            
            build_json_snapshot_with_nodes(
                &node_snapshots,
                Duration::from_secs(1),
                &node_resource_stats,
                workers_at_timestamp,  // NEW: per-worker data
                total_blocks,
                config.workers.threads,  // NEW: num_workers_per_node
            )
        })
        .collect();
    
    // Build final summary
    let include_coverage = config.workload.heatmap;
    let configured_block_size = config.workload.block_size;
    let configured_queue_depth = config.workload.queue_depth;
    let num_workers = config.workers.threads;
    let final_aggregate = stats_to_json_aggregate(final_stats, test_duration, total_blocks, include_coverage, configured_block_size, configured_queue_depth, num_workers);
    
    let per_worker: Vec<JsonWorkerStatsFinal> = per_worker_stats.iter()
        .map(|(worker_id, stats)| worker_stats_to_json_final(node_id.clone(), *worker_id, stats))
        .collect();
    
    let final_summary = JsonFinalSummary {
        total_duration: JsonDuration::from_duration(test_duration),
        aggregate: final_aggregate,
        per_worker,
    };
    
    JsonNodeOutput {
        test_info,
        time_series,
        final_summary,
    }
}


/// Build aggregate JsonNodeOutput from multiple nodes' time-series data
pub fn build_aggregate_node_output(
    node_id: String,
    hostname: Option<String>,
    start_time: std::time::SystemTime,
    end_time: std::time::SystemTime,
    test_duration: Duration,
    config: &crate::config::Config,
    all_node_snapshots: Vec<(String, Vec<AggregatedSnapshot>)>,  // (node_id, snapshots) for each node
    all_node_resource_stats: Vec<(String, Vec<ResourceStats>)>,  // (node_id, resource_stats) for each node
    all_per_worker_time_series: Vec<(String, Vec<Vec<AggregatedSnapshot>>)>,  // (node_id, timestamp → workers) (NEW)
    final_stats: &WorkerStats,
    all_per_worker_stats: &[(String, usize, &WorkerStats)],  // (node_id, worker_id, stats) for ALL workers
    total_blocks: Option<u64>,
) -> JsonNodeOutput {
    // Build test info
    let test_info = build_test_info(
        node_id,
        hostname,
        start_time,
        Some(end_time),
        Some(test_duration),
        config,
    );
    
    // Find max number of snapshots across all nodes
    let max_snapshots = all_node_snapshots.iter()
        .map(|(_, snapshots)| snapshots.len())
        .max()
        .unwrap_or(0);
    
    // Build time-series with per-node data at each timestamp
    let time_series: Vec<JsonSnapshot> = (1..max_snapshots)  // Skip first snapshot (startup noise)
        .map(|i| {
            // Collect snapshots from all nodes at this index
            let node_snapshots: Vec<(String, &AggregatedSnapshot)> = all_node_snapshots.iter()
                .filter_map(|(node_id, snapshots)| {
                    snapshots.get(i).map(|snapshot| (node_id.clone(), snapshot))
                })
                .collect();
            
            // Collect resource stats from all nodes at this index
            let node_resource_stats: Vec<(String, Option<ResourceStats>)> = all_node_resource_stats.iter()
                .map(|(node_id, resource_stats)| {
                    let stats = resource_stats.get(i).copied();
                    (node_id.clone(), stats)
                })
                .collect();
            
            // Collect per-worker snapshots from all nodes at this index (NEW)
            let all_workers_at_timestamp: Vec<(String, Vec<AggregatedSnapshot>)> = all_per_worker_time_series.iter()
                .filter_map(|(node_id, per_worker_ts)| {
                    per_worker_ts.get(i).map(|workers| (node_id.clone(), workers.clone()))
                })
                .collect();
            
            build_json_snapshot_with_nodes(
                &node_snapshots,
                Duration::from_secs(1),
                &node_resource_stats,
                Some(all_workers_at_timestamp),  // NEW: per-worker data from all nodes
                total_blocks,
                config.workers.threads,  // NEW: num_workers_per_node
            )
        })
        .collect();
    
    // Build final summary
    let include_coverage = config.workload.heatmap;
    let configured_block_size = config.workload.block_size;
    let configured_queue_depth = config.workload.queue_depth;
    let num_nodes = all_node_snapshots.len();
    let num_workers = if all_per_worker_stats.len() > 0 {
        all_per_worker_stats.len()  // Use actual count if available
    } else {
        num_nodes * config.workers.threads  // Otherwise calculate from config
    };
    let final_aggregate = stats_to_json_aggregate(final_stats, test_duration, total_blocks, include_coverage, configured_block_size, configured_queue_depth, num_workers);
    
    let per_worker: Vec<JsonWorkerStatsFinal> = all_per_worker_stats.iter()
        .map(|(node_id, worker_id, stats)| worker_stats_to_json_final(node_id.clone(), *worker_id, stats))
        .collect();
    
    let final_summary = JsonFinalSummary {
        total_duration: JsonDuration::from_duration(test_duration),
        aggregate: final_aggregate,
        per_worker,  // True per-worker stats with node_id
    };
    
    JsonNodeOutput {
        test_info,
        time_series,
        final_summary,
    }
}


/// Histogram bucket for raw histogram export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonHistogramBucket {
    pub index: usize,
    pub range_start_micros: u64,
    pub range_end_micros: u64,
    pub count: u64,
}

/// Raw histogram output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonHistogramOutput {
    pub node_id: String,
    pub histogram: JsonHistogramData,
}

/// Histogram data with all buckets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonHistogramData {
    pub num_samples: u64,
    pub min: JsonDuration,
    pub max: JsonDuration,
    pub mean: JsonDuration,
    pub buckets: Vec<JsonHistogramBucket>,
}

/// Export histogram to JSON (only non-zero buckets)
pub fn export_histogram(
    node_id: String,
    stats: &WorkerStats,
) -> JsonHistogramOutput {
    use crate::stats::simple_histogram::bucket_idx_to_micros;
    
    let hist = stats.io_latency();
    
    // Get all non-zero buckets
    let buckets: Vec<JsonHistogramBucket> = (0..112)
        .filter_map(|idx| {
            let count = hist.bucket_count(idx);
            if count > 0 {
                let range_start = bucket_idx_to_micros(idx);
                let range_end = if idx < 111 {
                    bucket_idx_to_micros(idx + 1)
                } else {
                    u64::MAX // Last bucket
                };
                
                Some(JsonHistogramBucket {
                    index: idx,
                    range_start_micros: range_start,
                    range_end_micros: range_end,
                    count,
                })
            } else {
                None
            }
        })
        .collect();
    
    JsonHistogramOutput {
        node_id,
        histogram: JsonHistogramData {
            num_samples: hist.len(),
            min: JsonDuration::from_duration(hist.min()),
            max: JsonDuration::from_duration(hist.max()),
            mean: JsonDuration::from_duration(hist.mean()),
            buckets,
        },
    }
}

/// Write histogram JSON output
pub fn write_histogram_output(
    output_path: &Path,
    histogram_output: &JsonHistogramOutput,
    pretty: bool,
) -> Result<()> {
    let file = File::create(output_path)?;
    
    if pretty {
        serde_json::to_writer_pretty(file, histogram_output)?;
    } else {
        serde_json::to_writer(file, histogram_output)?;
    }
    
    Ok(())
}

