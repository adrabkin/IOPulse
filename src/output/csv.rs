//! CSV output formatting
//!
//! This module provides CSV output for IOPulse time-series statistics.
//! CSV format is ideal for analysis in Excel, Python pandas, R, and other tools.
//!
//! Features:
//! - Time-series data with configurable interval
//! - Header row with column labels
//! - Aggregate mode (one row per interval)
//! - Per-worker mode (multiple rows per interval, one per worker)
//! - Live updates (append rows during test)
//! - Metadata operations included
//! - Resource utilization included

use crate::output::json::AggregatedSnapshot;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use crate::Result;

/// CSV writer for time-series data
pub struct CsvWriter {
    file: File,
    per_worker: bool,
    per_node: bool,  // NEW: For distributed aggregate CSV with per-node rows
}

impl CsvWriter {
    /// Create a new CSV writer with optional node_id column
    ///
    /// When per_node is true, adds a node_id column for distributed aggregate output.
    pub fn new_with_node_id(path: &Path, per_worker: bool, per_node: bool) -> Result<Self> {
        let mut file = File::create(path)?;
        
        // Write header row
        if per_node && per_worker {
            // Distributed per-worker mode: timestamp, elapsed, node_id, worker_id, then stats
            writeln!(file, "timestamp,elapsed_sec,node_id,worker_id,read_ops,write_ops,total_ops,read_iops,write_iops,total_iops,read_mbps,write_mbps,total_mbps,read_latency_us,write_latency_us,cpu_percent_total,cpu_percent_per_worker,cpu_percent_system,memory_mb,metadata_open,metadata_close,metadata_stat,metadata_setattr,metadata_mkdir,metadata_rmdir,metadata_unlink,metadata_rename,metadata_readdir,metadata_fsync,meta_open_lat_us,meta_close_lat_us,meta_stat_lat_us,meta_setattr_lat_us,meta_mkdir_lat_us,meta_rmdir_lat_us,meta_unlink_lat_us,meta_rename_lat_us,meta_readdir_lat_us,meta_fsync_lat_us")?;
        } else if per_node {
            // Distributed aggregate mode: timestamp, elapsed, node_id, then stats
            writeln!(file, "timestamp,elapsed_sec,node_id,read_ops,write_ops,total_ops,read_iops,write_iops,total_iops,read_mbps,write_mbps,total_mbps,read_latency_us,write_latency_us,cpu_percent_total,cpu_percent_per_worker,cpu_percent_system,memory_mb,metadata_open,metadata_close,metadata_stat,metadata_setattr,metadata_mkdir,metadata_rmdir,metadata_unlink,metadata_rename,metadata_readdir,metadata_fsync,meta_open_lat_us,meta_close_lat_us,meta_stat_lat_us,meta_setattr_lat_us,meta_mkdir_lat_us,meta_rmdir_lat_us,meta_unlink_lat_us,meta_rename_lat_us,meta_readdir_lat_us,meta_fsync_lat_us")?;
        } else if per_worker {
            writeln!(file, "timestamp,elapsed_sec,worker_id,read_ops,write_ops,total_ops,read_iops,write_iops,total_iops,read_mbps,write_mbps,total_mbps,read_latency_us,write_latency_us,cpu_percent_total,cpu_percent_per_worker,cpu_percent_system,memory_mb,metadata_open,metadata_close,metadata_stat,metadata_setattr,metadata_mkdir,metadata_rmdir,metadata_unlink,metadata_rename,metadata_readdir,metadata_fsync,meta_open_lat_us,meta_close_lat_us,meta_stat_lat_us,meta_setattr_lat_us,meta_mkdir_lat_us,meta_rmdir_lat_us,meta_unlink_lat_us,meta_rename_lat_us,meta_readdir_lat_us,meta_fsync_lat_us")?;
        } else {
            writeln!(file, "timestamp,elapsed_sec,read_ops,write_ops,total_ops,read_iops,write_iops,total_iops,read_mbps,write_mbps,total_mbps,read_latency_us,write_latency_us,cpu_percent_total,cpu_percent_per_worker,cpu_percent_system,memory_mb,metadata_ops,meta_open_lat_us,meta_close_lat_us,meta_stat_lat_us,meta_setattr_lat_us,meta_mkdir_lat_us,meta_rmdir_lat_us,meta_unlink_lat_us,meta_rename_lat_us,meta_readdir_lat_us,meta_fsync_lat_us")?;
        }
        
        Ok(Self { file, per_worker, per_node })
    }
    
    /// Append a snapshot to the CSV file
    pub fn append_snapshot(
        &mut self,
        snapshot: &AggregatedSnapshot,
        interval_secs: f64,
        resource_stats: Option<&crate::util::resource::ResourceStats>,
    ) -> Result<()> {
        let timestamp = format_timestamp_csv(snapshot.timestamp);
        let elapsed_sec = snapshot.elapsed.as_secs_f64();
        
        // Calculate rates using milliseconds for precision
        let interval_ms = interval_secs * 1000.0;
        let read_iops = if interval_ms > 0.0 {
            (snapshot.read_ops as f64 * 1000.0) / interval_ms
        } else {
            0.0
        };
        let write_iops = if interval_ms > 0.0 {
            (snapshot.write_ops as f64 * 1000.0) / interval_ms
        } else {
            0.0
        };
        let total_iops = read_iops + write_iops;
        
        let read_mbps = if interval_ms > 0.0 {
            (snapshot.read_bytes as f64 * 1000.0) / interval_ms / 1_048_576.0
        } else {
            0.0
        };
        let write_mbps = if interval_ms > 0.0 {
            (snapshot.write_bytes as f64 * 1000.0) / interval_ms / 1_048_576.0
        } else {
            0.0
        };
        let total_mbps = read_mbps + write_mbps;
        
        if self.per_worker {
            // Write aggregate row FIRST (worker_id = "Aggregate")
            let (cpu_total, cpu_per_worker, cpu_system, memory_mb) = if let Some(stats) = resource_stats {
                let num_workers = if let Some(ref per_worker) = snapshot.per_worker {
                    per_worker.len()
                } else {
                    1  // Fallback
                };
                let cpu_total = stats.cpu_percent;
                let cpu_per_worker = cpu_total / num_workers as f64;
                let cpu_system = if let Some(cpus) = crate::util::resource::ResourceSnapshot::num_cpus() {
                    cpu_total / cpus as f64  // Don't multiply by 100 - already a percentage
                } else {
                    0.0
                };
                let memory_mb = stats.memory_bytes as f64 / 1_048_576.0;
                (cpu_total, cpu_per_worker, cpu_system, memory_mb)
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };
            
            let read_lat_us = if snapshot.read_latency.len() > 0 {
                snapshot.read_latency.mean().as_micros() as f64
            } else {
                0.0
            };
            let write_lat_us = if snapshot.write_latency.len() > 0 {
                snapshot.write_latency.mean().as_micros() as f64
            } else {
                0.0
            };
            
            writeln!(
                self.file,
                "{},{:.3},Aggregate,{},{},{},{:.1},{:.1},{:.1},{:.2},{:.2},{:.2},{:.1},{:.1},{:.1},{:.1},{:.1},{:.2},{},{},{},{},{},{},{},{},{},{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1}",
                timestamp,
                elapsed_sec,
                snapshot.read_ops,
                snapshot.write_ops,
                snapshot.read_ops + snapshot.write_ops,
                read_iops,
                write_iops,
                total_iops,
                read_mbps,
                write_mbps,
                total_mbps,
                read_lat_us,
                write_lat_us,
                cpu_total,
                cpu_per_worker,
                cpu_system,
                memory_mb,
                snapshot.metadata_open_ops,
                snapshot.metadata_close_ops,
                snapshot.metadata_stat_ops,
                snapshot.metadata_setattr_ops,
                snapshot.metadata_mkdir_ops,
                snapshot.metadata_rmdir_ops,
                snapshot.metadata_unlink_ops,
                snapshot.metadata_rename_ops,
                snapshot.metadata_readdir_ops,
                snapshot.metadata_fsync_ops,
                if snapshot.metadata_open_latency.len() > 0 { snapshot.metadata_open_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_close_latency.len() > 0 { snapshot.metadata_close_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_stat_latency.len() > 0 { snapshot.metadata_stat_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_setattr_latency.len() > 0 { snapshot.metadata_setattr_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_mkdir_latency.len() > 0 { snapshot.metadata_mkdir_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_rmdir_latency.len() > 0 { snapshot.metadata_rmdir_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_unlink_latency.len() > 0 { snapshot.metadata_unlink_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_rename_latency.len() > 0 { snapshot.metadata_rename_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_readdir_latency.len() > 0 { snapshot.metadata_readdir_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_fsync_latency.len() > 0 { snapshot.metadata_fsync_latency.mean().as_micros() as f64 } else { 0.0 },
            )?;
            
            // Then write one row per worker
            if let Some(ref workers) = snapshot.per_worker {
                for (worker_id, worker) in workers.iter().enumerate() {
                    let worker_read_iops = if interval_ms > 0.0 {
                        (worker.read_ops as f64 * 1000.0) / interval_ms
                    } else {
                        0.0
                    };
                    let worker_write_iops = if interval_ms > 0.0 {
                        (worker.write_ops as f64 * 1000.0) / interval_ms
                    } else {
                        0.0
                    };
                    let worker_total_iops = worker_read_iops + worker_write_iops;
                    
                    let worker_read_mbps = if interval_ms > 0.0 {
                        (worker.read_bytes as f64 * 1000.0) / interval_ms / 1_048_576.0
                    } else {
                        0.0
                    };
                    let worker_write_mbps = if interval_ms > 0.0 {
                        (worker.write_bytes as f64 * 1000.0) / interval_ms / 1_048_576.0
                    } else {
                        0.0
                    };
                    let worker_total_mbps = worker_read_mbps + worker_write_mbps;
                    
                    let worker_read_lat = if worker.read_latency.len() > 0 {
                        worker.read_latency.mean().as_micros() as f64
                    } else {
                        0.0
                    };
                    let worker_write_lat = if worker.write_latency.len() > 0 {
                        worker.write_latency.mean().as_micros() as f64
                    } else {
                        0.0
                    };
                    
                    writeln!(
                        self.file,
                        "{},{:.3},{},{},{},{},{:.1},{:.1},{:.1},{:.2},{:.2},{:.2},{:.1},{:.1},{:.1},{:.2},{},{},{},{},{},{},{},{},{},{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1}",
                        timestamp,
                        elapsed_sec,
                        worker_id,
                        worker.read_ops,
                        worker.write_ops,
                        worker.read_ops + worker.write_ops,
                        worker_read_iops,
                        worker_write_iops,
                        worker_total_iops,
                        worker_read_mbps,
                        worker_write_mbps,
                        worker_total_mbps,
                        worker_read_lat,
                        worker_write_lat,
                        0.0, // CPU per-worker not tracked
                        0.0, // Memory per-worker not tracked
                        worker.metadata_open_ops,
                        worker.metadata_close_ops,
                        worker.metadata_stat_ops,
                        worker.metadata_setattr_ops,
                        worker.metadata_mkdir_ops,
                        worker.metadata_rmdir_ops,
                        worker.metadata_unlink_ops,
                        worker.metadata_rename_ops,
                        worker.metadata_readdir_ops,
                        worker.metadata_fsync_ops,
                        if worker.metadata_open_latency.len() > 0 { worker.metadata_open_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_close_latency.len() > 0 { worker.metadata_close_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_stat_latency.len() > 0 { worker.metadata_stat_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_setattr_latency.len() > 0 { worker.metadata_setattr_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_mkdir_latency.len() > 0 { worker.metadata_mkdir_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_rmdir_latency.len() > 0 { worker.metadata_rmdir_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_unlink_latency.len() > 0 { worker.metadata_unlink_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_rename_latency.len() > 0 { worker.metadata_rename_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_readdir_latency.len() > 0 { worker.metadata_readdir_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_fsync_latency.len() > 0 { worker.metadata_fsync_latency.mean().as_micros() as f64 } else { 0.0 },
                    )?;
                }
            }
        } else {
            // Write one aggregate row
            let metadata_total = snapshot.metadata_open_ops
                + snapshot.metadata_close_ops
                + snapshot.metadata_stat_ops
                + snapshot.metadata_setattr_ops
                + snapshot.metadata_mkdir_ops
                + snapshot.metadata_rmdir_ops
                + snapshot.metadata_unlink_ops
                + snapshot.metadata_rename_ops
                + snapshot.metadata_readdir_ops
                + snapshot.metadata_fsync_ops;
            
            // Get CPU and memory from resource stats
            let (cpu_total, cpu_per_worker, cpu_system, memory_mb) = if let Some(stats) = resource_stats {
                let num_workers = if let Some(ref per_worker) = snapshot.per_worker {
                    per_worker.len()
                } else {
                    1  // Fallback
                };
                let cpu_total = stats.cpu_percent;
                let cpu_per_worker = cpu_total / num_workers as f64;
                let cpu_system = if let Some(cpus) = crate::util::resource::ResourceSnapshot::num_cpus() {
                    cpu_total / cpus as f64  // Don't multiply by 100 - already a percentage
                } else {
                    0.0
                };
                let memory_mb = stats.memory_bytes as f64 / 1_048_576.0;
                (cpu_total, cpu_per_worker, cpu_system, memory_mb)
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };
            
            let read_lat_us = if snapshot.read_latency.len() > 0 {
                snapshot.read_latency.mean().as_micros() as f64
            } else {
                0.0
            };
            let write_lat_us = if snapshot.write_latency.len() > 0 {
                snapshot.write_latency.mean().as_micros() as f64
            } else {
                0.0
            };
            
            writeln!(
                self.file,
                "{},{:.3},{},{},{},{:.1},{:.1},{:.1},{:.2},{:.2},{:.2},{:.1},{:.1},{:.1},{:.1},{:.1},{:.2},{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1}",
                timestamp,
                elapsed_sec,
                snapshot.read_ops,
                snapshot.write_ops,
                snapshot.read_ops + snapshot.write_ops,
                read_iops,
                write_iops,
                total_iops,
                read_mbps,
                write_mbps,
                total_mbps,
                read_lat_us,
                write_lat_us,
                cpu_total,
                cpu_per_worker,
                cpu_system,
                memory_mb,
                metadata_total,
                if snapshot.metadata_open_latency.len() > 0 { snapshot.metadata_open_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_close_latency.len() > 0 { snapshot.metadata_close_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_stat_latency.len() > 0 { snapshot.metadata_stat_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_setattr_latency.len() > 0 { snapshot.metadata_setattr_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_mkdir_latency.len() > 0 { snapshot.metadata_mkdir_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_rmdir_latency.len() > 0 { snapshot.metadata_rmdir_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_unlink_latency.len() > 0 { snapshot.metadata_unlink_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_rename_latency.len() > 0 { snapshot.metadata_rename_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_readdir_latency.len() > 0 { snapshot.metadata_readdir_latency.mean().as_micros() as f64 } else { 0.0 },
                if snapshot.metadata_fsync_latency.len() > 0 { snapshot.metadata_fsync_latency.mean().as_micros() as f64 } else { 0.0 },
            )?;
        }
        
        // Flush to ensure data is written
        self.file.flush()?;
        
        Ok(())
    }
    
    /// Append a snapshot with node_id to the CSV file (for distributed aggregate)
    pub fn append_snapshot_with_node(
        &mut self,
        node_id: &str,
        snapshot: &AggregatedSnapshot,
        interval_secs: f64,
        resource_stats: Option<&crate::util::resource::ResourceStats>,
        num_workers: usize,
    ) -> Result<()> {
        if !self.per_node {
            // If not in per_node mode, fall back to regular append
            return self.append_snapshot(snapshot, interval_secs, resource_stats);
        }
        
        let timestamp = format_timestamp_csv(snapshot.timestamp);
        let elapsed_sec = snapshot.elapsed.as_secs_f64();
        
        // Calculate rates using milliseconds for precision
        let interval_ms = interval_secs * 1000.0;
        let read_iops = if interval_ms > 0.0 {
            (snapshot.read_ops as f64 * 1000.0) / interval_ms
        } else {
            0.0
        };
        let write_iops = if interval_ms > 0.0 {
            (snapshot.write_ops as f64 * 1000.0) / interval_ms
        } else {
            0.0
        };
        let total_iops = read_iops + write_iops;
        
        let read_mbps = if interval_ms > 0.0 {
            (snapshot.read_bytes as f64 * 1000.0) / interval_ms / 1_048_576.0
        } else {
            0.0
        };
        let write_mbps = if interval_ms > 0.0 {
            (snapshot.write_bytes as f64 * 1000.0) / interval_ms / 1_048_576.0
        } else {
            0.0
        };
        let total_mbps = read_mbps + write_mbps;
        
        let _metadata_total = snapshot.metadata_open_ops
            + snapshot.metadata_close_ops
            + snapshot.metadata_stat_ops
            + snapshot.metadata_setattr_ops
            + snapshot.metadata_mkdir_ops
            + snapshot.metadata_rmdir_ops
            + snapshot.metadata_unlink_ops
            + snapshot.metadata_rename_ops
            + snapshot.metadata_readdir_ops
            + snapshot.metadata_fsync_ops;
        
        // Get CPU and memory from resource stats
        let (cpu_total, cpu_per_worker, cpu_system, memory_mb) = if let Some(stats) = resource_stats {
            let cpu_total = stats.cpu_percent;
            let cpu_per_worker = cpu_total / num_workers as f64;
            let cpu_system = if let Some(cpus) = crate::util::resource::ResourceSnapshot::num_cpus() {
                cpu_total / cpus as f64  // Don't multiply by 100 - already a percentage
            } else {
                0.0
            };
            let memory_mb = stats.memory_bytes as f64 / 1_048_576.0;
            (cpu_total, cpu_per_worker, cpu_system, memory_mb)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        
        let read_lat_us = if snapshot.read_latency.len() > 0 {
            snapshot.read_latency.mean().as_micros() as f64
        } else {
            0.0
        };
        let write_lat_us = if snapshot.write_latency.len() > 0 {
            snapshot.write_latency.mean().as_micros() as f64
        } else {
            0.0
        };
        
        // Write row with node_id
        writeln!(
            self.file,
            "{},{:.3},{},{},{},{},{:.1},{:.1},{:.1},{:.2},{:.2},{:.2},{:.1},{:.1},{:.1},{:.1},{:.1},{:.2},{},{},{},{},{},{},{},{},{},{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1}",
            timestamp,
            elapsed_sec,
            node_id,
            snapshot.read_ops,
            snapshot.write_ops,
            snapshot.read_ops + snapshot.write_ops,
            read_iops,
            write_iops,
            total_iops,
            read_mbps,
            write_mbps,
            total_mbps,
            read_lat_us,
            write_lat_us,
            cpu_total,
            cpu_per_worker,
            cpu_system,
            memory_mb,
            snapshot.metadata_open_ops,
            snapshot.metadata_close_ops,
            snapshot.metadata_stat_ops,
            snapshot.metadata_setattr_ops,
            snapshot.metadata_mkdir_ops,
            snapshot.metadata_rmdir_ops,
            snapshot.metadata_unlink_ops,
            snapshot.metadata_rename_ops,
            snapshot.metadata_readdir_ops,
            snapshot.metadata_fsync_ops,
            if snapshot.metadata_open_latency.len() > 0 { snapshot.metadata_open_latency.mean().as_micros() as f64 } else { 0.0 },
            if snapshot.metadata_close_latency.len() > 0 { snapshot.metadata_close_latency.mean().as_micros() as f64 } else { 0.0 },
            if snapshot.metadata_stat_latency.len() > 0 { snapshot.metadata_stat_latency.mean().as_micros() as f64 } else { 0.0 },
            if snapshot.metadata_setattr_latency.len() > 0 { snapshot.metadata_setattr_latency.mean().as_micros() as f64 } else { 0.0 },
            if snapshot.metadata_mkdir_latency.len() > 0 { snapshot.metadata_mkdir_latency.mean().as_micros() as f64 } else { 0.0 },
            if snapshot.metadata_rmdir_latency.len() > 0 { snapshot.metadata_rmdir_latency.mean().as_micros() as f64 } else { 0.0 },
            if snapshot.metadata_unlink_latency.len() > 0 { snapshot.metadata_unlink_latency.mean().as_micros() as f64 } else { 0.0 },
            if snapshot.metadata_rename_latency.len() > 0 { snapshot.metadata_rename_latency.mean().as_micros() as f64 } else { 0.0 },
            if snapshot.metadata_readdir_latency.len() > 0 { snapshot.metadata_readdir_latency.mean().as_micros() as f64 } else { 0.0 },
            if snapshot.metadata_fsync_latency.len() > 0 { snapshot.metadata_fsync_latency.mean().as_micros() as f64 } else { 0.0 },
        )?;
        
        // Write per-worker rows if enabled
        if self.per_worker {
            if let Some(ref workers) = snapshot.per_worker {
                for (worker_id, worker) in workers.iter().enumerate() {
                    let worker_read_iops = if interval_ms > 0.0 {
                        (worker.read_ops as f64 * 1000.0) / interval_ms
                    } else {
                        0.0
                    };
                    let worker_write_iops = if interval_ms > 0.0 {
                        (worker.write_ops as f64 * 1000.0) / interval_ms
                    } else {
                        0.0
                    };
                    let worker_total_iops = worker_read_iops + worker_write_iops;
                    
                    let worker_read_mbps = if interval_ms > 0.0 {
                        (worker.read_bytes as f64 * 1000.0) / interval_ms / 1_048_576.0
                    } else {
                        0.0
                    };
                    let worker_write_mbps = if interval_ms > 0.0 {
                        (worker.write_bytes as f64 * 1000.0) / interval_ms / 1_048_576.0
                    } else {
                        0.0
                    };
                    let worker_total_mbps = worker_read_mbps + worker_write_mbps;
                    
                    let worker_read_lat = if worker.read_latency.len() > 0 {
                        worker.read_latency.mean().as_micros() as f64
                    } else {
                        0.0
                    };
                    let worker_write_lat = if worker.write_latency.len() > 0 {
                        worker.write_latency.mean().as_micros() as f64
                    } else {
                        0.0
                    };
                    
                    writeln!(
                        self.file,
                        "{},{:.3},{},{},{},{},{},{:.1},{:.1},{:.1},{:.2},{:.2},{:.2},{:.1},{:.1},{:.1},{:.2},{},{},{},{},{},{},{},{},{},{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1}",
                        timestamp,
                        elapsed_sec,
                        node_id,
                        worker_id,
                        worker.read_ops,
                        worker.write_ops,
                        worker.read_ops + worker.write_ops,
                        worker_read_iops,
                        worker_write_iops,
                        worker_total_iops,
                        worker_read_mbps,
                        worker_write_mbps,
                        worker_total_mbps,
                        worker_read_lat,
                        worker_write_lat,
                        0.0, // CPU per-worker not tracked
                        0.0, // Memory per-worker not tracked
                        worker.metadata_open_ops,
                        worker.metadata_close_ops,
                        worker.metadata_stat_ops,
                        worker.metadata_setattr_ops,
                        worker.metadata_mkdir_ops,
                        worker.metadata_rmdir_ops,
                        worker.metadata_unlink_ops,
                        worker.metadata_rename_ops,
                        worker.metadata_readdir_ops,
                        worker.metadata_fsync_ops,
                        if worker.metadata_open_latency.len() > 0 { worker.metadata_open_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_close_latency.len() > 0 { worker.metadata_close_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_stat_latency.len() > 0 { worker.metadata_stat_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_setattr_latency.len() > 0 { worker.metadata_setattr_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_mkdir_latency.len() > 0 { worker.metadata_mkdir_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_rmdir_latency.len() > 0 { worker.metadata_rmdir_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_unlink_latency.len() > 0 { worker.metadata_unlink_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_rename_latency.len() > 0 { worker.metadata_rename_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_readdir_latency.len() > 0 { worker.metadata_readdir_latency.mean().as_micros() as f64 } else { 0.0 },
                        if worker.metadata_fsync_latency.len() > 0 { worker.metadata_fsync_latency.mean().as_micros() as f64 } else { 0.0 },
                    )?;
                }
            }
        }
        
        // Flush to ensure data is written
        self.file.flush()?;
        
        Ok(())
    }
}

/// Format timestamp for CSV (ISO 8601)
fn format_timestamp_csv(time: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    
    let duration_since_epoch = time.duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::from_secs(0));
    
    let total_secs = duration_since_epoch.as_secs();
    
    // Calculate date/time components
    const SECS_PER_DAY: u64 = 86400;
    const SECS_PER_HOUR: u64 = 3600;
    const SECS_PER_MINUTE: u64 = 60;
    
    let days_since_epoch = total_secs / SECS_PER_DAY;
    let remaining_secs = total_secs % SECS_PER_DAY;
    
    let hours = remaining_secs / SECS_PER_HOUR;
    let minutes = (remaining_secs % SECS_PER_HOUR) / SECS_PER_MINUTE;
    let seconds = remaining_secs % SECS_PER_MINUTE;
    
    // Simple date calculation
    let year = 1970 + (days_since_epoch / 365);
    let day_of_year = days_since_epoch % 365;
    let month = (day_of_year / 30) + 1;
    let day = (day_of_year % 30) + 1;
    
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}
