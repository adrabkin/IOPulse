//! Distributed coordinator
//!
//! This module implements the coordinator for distributed mode.
//! The coordinator:
//! - Connects to all nodes
//! - Distributes configuration
//! - Coordinates synchronized start
//! - Collects heartbeats
//! - Aggregates results

use crate::distributed::protocol::*;
use crate::config::Config;
use crate::stats::WorkerStats;
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::sleep;

/// Distributed coordinator
///
/// Orchestrates distributed testing across multiple nodes.
pub struct DistributedCoordinator {
    /// Test configuration
    config: Arc<Config>,
    
    /// List of node addresses (IP:port)
    node_addresses: Vec<String>,
}

impl DistributedCoordinator {
    /// Create a new distributed coordinator
    pub fn new(config: Arc<Config>, node_addresses: Vec<String>) -> Result<Self> {
        if node_addresses.is_empty() {
            anyhow::bail!("No nodes specified for distributed mode");
        }
        
        Ok(Self {
            config,
            node_addresses,
        })
    }
    
    /// Run the distributed test
    pub async fn run(self) -> Result<()> {
        println!("Distributed Coordinator");
        println!();
        
        // Load layout_manifest if specified OR generate layout
        let file_list: Option<Vec<std::path::PathBuf>> = if !self.config.targets.is_empty() {
            let target = &self.config.targets[0];
            
            if let Some(ref manifest_path) = target.layout_manifest {
                println!("Loading layout manifest: {}", manifest_path.display());
                
                // Warn if conflicting parameters provided
                if target.layout_config.is_some() {
                    println!("⚠️  Warning: layout_manifest provided, ignoring --dir-depth, --dir-width, --total-files");
                }
                
                let manifest = crate::target::LayoutManifest::from_file(manifest_path)
                    .context("Failed to load layout manifest")?;
                
                println!("Layout manifest loaded: {} files", manifest.file_count());
                
                // Export if requested
                if let Some(ref export_path) = target.export_layout_manifest {
                    manifest.to_file(export_path)
                        .context("Failed to export layout manifest")?;
                    println!("Layout manifest exported to: {}", export_path.display());
                }
                
                // Convert to absolute paths
                let root = &target.path;
                let absolute_paths: Vec<std::path::PathBuf> = manifest.file_entries
                    .iter()
                    .map(|entry| root.join(&entry.path))
                    .collect();
                
                Some(absolute_paths)
            } else if let Some(ref layout_config) = target.layout_config {
                // Calculate total workers for per-worker distribution
                let total_workers = self.node_addresses.len() * self.config.workers.threads;
                let num_workers = if target.distribution == crate::config::workload::FileDistribution::PerWorker {
                    Some(total_workers)
                } else {
                    None
                };
                
                // Generate layout from config
                println!("Generating directory layout...");
                if let Some(nw) = num_workers {
                    println!("  Depth: {}, Width: {}, Files per dir: {} (per-worker mode: {} workers)", 
                        layout_config.depth, layout_config.width, layout_config.files_per_dir, nw);
                } else {
                    println!("  Depth: {}, Width: {}, Files per dir: {}", 
                        layout_config.depth, layout_config.width, layout_config.files_per_dir);
                }
                
                use crate::target::layout::{LayoutGenerator, LayoutConfig as GenLayoutConfig, NamingPattern as GenNamingPattern};
                
                let gen_config = GenLayoutConfig {
                    depth: layout_config.depth,
                    width: layout_config.width,
                    files_per_dir: layout_config.files_per_dir,
                    file_size: target.file_size.unwrap_or(0),
                    naming_pattern: match layout_config.naming_pattern {
                        crate::config::NamingPattern::Sequential => GenNamingPattern::Sequential,
                        crate::config::NamingPattern::Random => GenNamingPattern::Random,
                        crate::config::NamingPattern::Prefixed => GenNamingPattern::Prefixed,
                    },
                    num_workers,
                    total_files: layout_config.total_files,
                };
                
                let mut generator = LayoutGenerator::new(target.path.clone(), gen_config);
                generator.generate().context("Failed to generate directory layout")?;
                
                let file_count = generator.file_count();
                if let Some(nw) = num_workers {
                    let base_files = file_count / nw;
                    println!("Generated {} files ({} base × {} workers) in {} directories", 
                        file_count, base_files, nw, generator.stats().mkdir_count);
                } else {
                    println!("Generated {} files in {} directories", 
                        file_count, generator.stats().mkdir_count);
                }
                
                // Export layout manifest if requested
                if let Some(ref export_path) = target.export_layout_manifest {
                    // Create manifest from generated files
                    let file_size = target.file_size.unwrap_or(0);
                    let manifest = crate::target::layout_manifest::LayoutManifest::from_paths_and_size(
                        generator.file_paths().to_vec(),
                        file_size,
                        crate::target::layout_manifest::ManifestHeader {
                            generated_at: chrono::Utc::now(),
                            depth: Some(layout_config.depth),
                            width: Some(layout_config.width),
                            total_files: file_count,
                            total_directories: Some(generator.stats().mkdir_count as usize),
                            files_per_dir: Some(layout_config.files_per_dir),
                            file_size: target.file_size.unwrap_or(0),
                            num_workers,
                        },
                    );
                    
                    manifest.to_file(export_path)
                        .context("Failed to export layout manifest")?;
                    println!("Layout manifest exported to: {} ({} files)", 
                        export_path.display(), file_count);
                }
                
                Some(generator.file_paths().to_vec())
            } else {
                None
            }
        } else {
            None
        };
        
        // Validate and fill layout files if needed
        if let Some(ref file_list) = file_list {
            let target = &self.config.targets[0];
            let has_reads = self.config.workload.read_percent > 0;
            let needs_fill_for_mmap = self.config.workload.engine == crate::config::workload::EngineType::Mmap;
            
            // Check if auto-fill is disabled
            if target.no_refill && (has_reads || needs_fill_for_mmap) {
                // Check if any files are empty/sparse
                let has_empty_files = file_list.iter().any(|path| {
                    if let Ok(metadata) = std::fs::metadata(path) {
                        metadata.len() == 0
                    } else {
                        true  // File doesn't exist
                    }
                });
                
                if has_empty_files {
                    anyhow::bail!(
                        "Layout contains empty files but auto-fill is disabled (--no-refill flag).\n\
                         Remove --no-refill to enable auto-fill, or pre-fill files manually."
                    );
                }
            }
            
            if !target.no_refill && (has_reads || needs_fill_for_mmap) {
                println!("Validating {} files...", file_list.len());
                
                let start = std::time::Instant::now();
                let filled_count = validate_and_fill_files(
                    file_list,
                    target.file_size.unwrap_or(0),
                    self.config.workload.write_pattern,
                )?;
                let elapsed = start.elapsed();
                
                if filled_count > 0 {
                    println!("✅ Filled {} sparse files in {:.2}s", filled_count, elapsed.as_secs_f64());
                } else {
                    println!("✅ All files validated ({:.2}s)", elapsed.as_secs_f64());
                }
            }
        }
        
        // Create parent directories for targets (before connecting to nodes)
        println!("Preparing target directories...");
        for target in &self.config.targets {
            if let Some(parent) = target.path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
                    println!("  Created directory: {}", parent.display());
                }
            }
        }
        
        println!();
        println!("Connecting to {} nodes...", self.node_addresses.len());
        
        // Connect to all nodes
        let mut connections = Vec::new();
        for (i, addr) in self.node_addresses.iter().enumerate() {
            println!("  Connecting to node {} ({})...", i, addr);
            let stream = TcpStream::connect(addr).await
                .with_context(|| format!("Failed to connect to {}", addr))?;
            println!("  ✅ Connected to node {} ({})", i, addr);
            connections.push((i, addr.clone(), stream));
        }
        
        println!();
        println!("All {} nodes connected!", connections.len());
        
        // Prepare files if needed (create/fill before test)
        // Skip if we already have a file_list (layout was generated/loaded)
        let num_nodes = connections.len();
        
        if file_list.is_none() {
            println!();
            
            let has_reads = self.config.workload.read_percent > 0;
            let needs_preallocation = self.config.workload.direct;
            let is_shared = self.config.targets.iter()
                .all(|t| t.distribution == crate::config::workload::FileDistribution::Shared);
            
            // Smart auto-partitioning for distributed pre-allocation
            // Only for single files
            if needs_preallocation && num_nodes > 1 && is_shared {
                println!("Smart Partitioning: Distributing pre-allocation across {} nodes", num_nodes);
                println!("  Reason: O_DIRECT requires pre-allocation, parallel is faster");
                println!();
                
                // Use distributed pre-allocation
                self.distributed_preallocate(&mut connections, has_reads).await?;
            } else {
                // Coordinator handles file preparation
                println!("Preparing files...");
                
                for target in &self.config.targets {
                    if !target.path.exists() || (has_reads && is_file_sparse(&target.path)?) {
                        println!("  Creating/filling: {}", target.path.display());
                    
                    use crate::target::file::FileTarget;
                    use crate::target::Target;
                    use crate::target::OpenFlags;
                    
                    let mut file_target = FileTarget::new(
                        target.path.clone(),
                        target.file_size,
                    );
                    
                    // For O_DIRECT, we need to preallocate
                    if self.config.workload.direct {
                        file_target.set_preallocate(true);
                    }
                    
                    let flags = OpenFlags {
                        direct: false,  // Use buffered for filling (faster)
                        sync: false,
                        create: true,
                        truncate: false,
                    };
                    
                    file_target.open(flags)?;
                    
                    // Check if no_refill flag is set
                    if target.no_refill {
                        // Check if file needs filling
                        let needs_fill = has_reads || self.config.workload.engine == crate::config::workload::EngineType::Mmap;
                        
                        if needs_fill {
                            // File is empty and needs filling, but no_refill is set
                            file_target.close()?;
                            anyhow::bail!(
                                "File is empty but auto-fill is disabled (--no-refill flag).\n\
                                 Remove --no-refill to enable auto-fill, or pre-fill file manually."
                            );
                        }
                    }
                    
                    // mmap engine ALWAYS needs file filling (can't map empty files)
                    // Other engines only need filling if reads are involved
                    let needs_fill = has_reads || self.config.workload.engine == crate::config::workload::EngineType::Mmap;
                    
                    if needs_fill {
                        file_target.refill(self.config.workload.write_pattern)?;
                        println!("  ✅ File filled");
                    } else {
                        println!("  ✅ File created");
                    }
                    
                    file_target.close()?;
                } else {
                    println!("  ✅ File exists: {}", target.path.display());
                }
            }
            }  // End of if file_list.is_none()
        }
        
        // Calculate total workers
        let threads_per_node = self.config.workers.threads;
        let total_workers = connections.len() * threads_per_node;
        println!();
        println!("Total workers: {} ({} nodes × {} threads)", 
            total_workers, connections.len(), threads_per_node);
        
        // Send CONFIG messages to all nodes
        println!();
        println!("Sending configuration to all nodes...");
        
        for (node_id, addr, stream) in &mut connections {
            let worker_id_start = *node_id * threads_per_node;
            let worker_id_end = worker_id_start + threads_per_node;
            
            // For PARTITIONED mode with file_list, calculate file range for this node
            let (node_file_list, node_file_range) = if let Some(ref fl) = file_list {
                let is_partitioned = self.config.targets[0].distribution == crate::config::workload::FileDistribution::Partitioned;
                
                if is_partitioned {
                    // Partition files across nodes
                    let total_files = fl.len();
                    let files_per_node = total_files / num_nodes;
                    let start = *node_id * files_per_node;
                    let end = if *node_id == num_nodes - 1 {
                        total_files  // Last node gets remainder
                    } else {
                        start + files_per_node
                    };
                    
                    (Some(fl.clone()), Some((start, end)))
                } else {
                    // SHARED mode: all nodes get all files
                    (Some(fl.clone()), None)
                }
            } else {
                (None, None)
            };
            
            let config_msg = ConfigMessage {
                protocol_version: PROTOCOL_VERSION,
                node_id: addr.clone(),
                config: (*self.config).clone(),
                worker_id_start,
                worker_id_end,
                file_list: node_file_list,
                file_range: node_file_range,
                skip_preallocation: true, // Coordinator already pre-allocated
            };
            
            write_message(stream, &Message::Config(config_msg)).await
                .with_context(|| format!("Failed to send CONFIG to node {}", node_id))?;
            
            println!("  ✅ Sent CONFIG to node {} (workers {}-{})", node_id, worker_id_start, worker_id_end - 1);
        }
        
        // Wait for READY messages from all nodes
        println!();
        println!("Waiting for all nodes to be ready...");
        
        for (node_id, _addr, stream) in &mut connections {
            let msg = read_message(stream).await
                .with_context(|| format!("Failed to read READY from node {}", node_id))?;
            
            match msg {
                Message::Ready(ready) => {
                    if ready.protocol_version != PROTOCOL_VERSION {
                        anyhow::bail!("Protocol version mismatch on node {}: expected {}, got {}", 
                            node_id, PROTOCOL_VERSION, ready.protocol_version);
                    }
                    println!("  ✅ Node {} ready ({} workers)", node_id, ready.num_workers);
                }
                Message::Error(err) => {
                    anyhow::bail!("Node {} reported error: {}", node_id, err.error);
                }
                other => {
                    anyhow::bail!("Expected READY from node {}, got {:?}", node_id, other);
                }
            }
        }
        
        // Calculate start timestamp (now + 100ms)
        println!();
        println!("All nodes ready!");
        println!("Synchronized start in 100ms...");
        
        let start_delay = Duration::from_millis(100);
        let start_timestamp_ns = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            + start_delay)
            .as_nanos() as u64;
        
        // Send START messages to all nodes
        for (node_id, _addr, stream) in &mut connections {
            let start_msg = StartMessage {
                start_timestamp_ns,
            };
            
            write_message(stream, &Message::Start(start_msg)).await
                .with_context(|| format!("Failed to send START to node {}", node_id))?;
        }
        
        println!("Sent START to all nodes");
        
        // DON'T sleep here - start collecting heartbeats immediately to avoid race condition
        println!();
        println!("Test running...");
        
        // Collect heartbeats for time-series data (needed for CSV/JSON time-series)
        let csv_enabled = self.config.output.csv_output.is_some();
        let json_enabled = self.config.output.json_output.is_some();
        let collect_time_series = csv_enabled || json_enabled;
        
        let mut time_series_snapshots: Vec<Vec<crate::output::json::AggregatedSnapshot>> = 
            vec![Vec::new(); connections.len()];
        
        // Store resource stats per snapshot for CSV output
        let mut time_series_resource_stats: Vec<Vec<crate::util::resource::ResourceStats>> = 
            vec![Vec::new(); connections.len()];
        
        // Track previous cumulative values for delta calculation (per node)
        let mut previous_cumulative: Vec<Option<crate::output::json::AggregatedSnapshot>> = 
            vec![None; connections.len()];
        
        // Per-worker time-series collection (when --per-worker-output is enabled)
        let collect_per_worker = self.config.output.per_worker_output;
        let mut per_worker_time_series: Vec<Vec<Vec<crate::output::json::AggregatedSnapshot>>> = 
            vec![Vec::new(); connections.len()];  // node → timestamp → workers
        let mut previous_per_worker_cumulative: Vec<Option<Vec<crate::output::json::AggregatedSnapshot>>> = 
            vec![None; connections.len()];  // node → workers
        
        if let crate::config::workload::CompletionMode::Duration { seconds } = self.config.workload.completion_mode {
            let test_duration = Duration::from_secs(seconds);
            let start_time = std::time::Instant::now();
            
            // Actively collect heartbeats if time-series is needed
            if collect_time_series {
                println!("Collecting time-series data from heartbeats...");
                
                loop {
                    let elapsed = start_time.elapsed();
                    if elapsed >= test_duration {
                        break;
                    }
                    
                    // Try to read from all nodes
                    // Heartbeats arrive every 1 second, so use 1-second timeout
                    for (node_idx, (_node_id, _addr, stream)) in connections.iter_mut().enumerate() {
                        // Use 1-second timeout (heartbeats are sent every 1 second)
                        match tokio::time::timeout(Duration::from_secs(1), read_message(stream)).await {
                            Ok(Ok(Message::Heartbeat(hb))) => {
                                // Skip first heartbeat (startup artifact, not steady-state)
                                let elapsed = Duration::from_nanos(hb.elapsed_ns);
                                if elapsed.as_millis() < 500 {
                                    continue;  // Skip heartbeats in first 500ms
                                }
                                
                                // Convert WorkerStatsSnapshot to AggregatedSnapshot (cumulative values)
                                let cumulative = worker_snapshot_to_aggregated(&hb.stats, elapsed);
                                
                                // Calculate delta from previous cumulative snapshot
                                let delta_snapshot = if let Some(ref prev) = previous_cumulative[node_idx] {
                                    // Calculate deltas
                                    crate::output::json::AggregatedSnapshot {
                                        timestamp: cumulative.timestamp,
                                        elapsed: cumulative.elapsed,
                                        read_ops: cumulative.read_ops.saturating_sub(prev.read_ops),
                                        write_ops: cumulative.write_ops.saturating_sub(prev.write_ops),
                                        read_bytes: cumulative.read_bytes.saturating_sub(prev.read_bytes),
                                        write_bytes: cumulative.write_bytes.saturating_sub(prev.write_bytes),
                                        errors: cumulative.errors,
                                        avg_latency_us: cumulative.avg_latency_us,
                                        read_latency: cumulative.read_latency.clone(),
                                        write_latency: cumulative.write_latency.clone(),
                                        metadata_open_ops: cumulative.metadata_open_ops.saturating_sub(prev.metadata_open_ops),
                                        metadata_close_ops: cumulative.metadata_close_ops.saturating_sub(prev.metadata_close_ops),
                                        metadata_stat_ops: cumulative.metadata_stat_ops.saturating_sub(prev.metadata_stat_ops),
                                        metadata_setattr_ops: cumulative.metadata_setattr_ops.saturating_sub(prev.metadata_setattr_ops),
                                        metadata_mkdir_ops: cumulative.metadata_mkdir_ops.saturating_sub(prev.metadata_mkdir_ops),
                                        metadata_rmdir_ops: cumulative.metadata_rmdir_ops.saturating_sub(prev.metadata_rmdir_ops),
                                        metadata_unlink_ops: cumulative.metadata_unlink_ops.saturating_sub(prev.metadata_unlink_ops),
                                        metadata_rename_ops: cumulative.metadata_rename_ops.saturating_sub(prev.metadata_rename_ops),
                                        metadata_readdir_ops: cumulative.metadata_readdir_ops.saturating_sub(prev.metadata_readdir_ops),
                                        metadata_fsync_ops: cumulative.metadata_fsync_ops.saturating_sub(prev.metadata_fsync_ops),
                                        metadata_open_latency: cumulative.metadata_open_latency.clone(),
                                        metadata_close_latency: cumulative.metadata_close_latency.clone(),
                                        metadata_stat_latency: cumulative.metadata_stat_latency.clone(),
                                        metadata_setattr_latency: cumulative.metadata_setattr_latency.clone(),
                                        metadata_mkdir_latency: cumulative.metadata_mkdir_latency.clone(),
                                        metadata_rmdir_latency: cumulative.metadata_rmdir_latency.clone(),
                                        metadata_unlink_latency: cumulative.metadata_unlink_latency.clone(),
                                        metadata_rename_latency: cumulative.metadata_rename_latency.clone(),
                                        metadata_readdir_latency: cumulative.metadata_readdir_latency.clone(),
                                        metadata_fsync_latency: cumulative.metadata_fsync_latency.clone(),
                                        per_worker: None,
                                    }
                                } else {
                                    // First snapshot - use cumulative as-is
                                    cumulative.clone()
                                };
                                
                                // Store cumulative for next delta calculation
                                previous_cumulative[node_idx] = Some(cumulative);
                                
                                // Process per-worker snapshots if enabled
                                let mut delta_snapshot = delta_snapshot;  // Make mutable
                                if collect_per_worker {
                                    if let Some(ref per_worker_snapshots) = hb.per_worker_stats {
                                        // Convert each worker snapshot to AggregatedSnapshot (cumulative)
                                        let cumulative_workers: Vec<crate::output::json::AggregatedSnapshot> = per_worker_snapshots.iter()
                                            .map(|ws| worker_snapshot_to_aggregated(ws, elapsed))
                                            .collect();
                                        
                                        // Calculate deltas for each worker
                                        let delta_workers = if let Some(ref prev_workers) = previous_per_worker_cumulative[node_idx] {
                                            // Calculate delta for each worker
                                            cumulative_workers.iter().zip(prev_workers.iter())
                                                .map(|(curr, prev)| {
                                                    crate::output::json::AggregatedSnapshot {
                                                        timestamp: curr.timestamp,
                                                        elapsed: curr.elapsed,
                                                        read_ops: curr.read_ops.saturating_sub(prev.read_ops),
                                                        write_ops: curr.write_ops.saturating_sub(prev.write_ops),
                                                        read_bytes: curr.read_bytes.saturating_sub(prev.read_bytes),
                                                        write_bytes: curr.write_bytes.saturating_sub(prev.write_bytes),
                                                        errors: curr.errors,
                                                        avg_latency_us: curr.avg_latency_us,
                                                        read_latency: curr.read_latency.clone(),
                                                        write_latency: curr.write_latency.clone(),
                                                        metadata_open_ops: curr.metadata_open_ops.saturating_sub(prev.metadata_open_ops),
                                                        metadata_close_ops: curr.metadata_close_ops.saturating_sub(prev.metadata_close_ops),
                                                        metadata_stat_ops: curr.metadata_stat_ops.saturating_sub(prev.metadata_stat_ops),
                                                        metadata_setattr_ops: curr.metadata_setattr_ops.saturating_sub(prev.metadata_setattr_ops),
                                                        metadata_mkdir_ops: curr.metadata_mkdir_ops.saturating_sub(prev.metadata_mkdir_ops),
                                                        metadata_rmdir_ops: curr.metadata_rmdir_ops.saturating_sub(prev.metadata_rmdir_ops),
                                                        metadata_unlink_ops: curr.metadata_unlink_ops.saturating_sub(prev.metadata_unlink_ops),
                                                        metadata_rename_ops: curr.metadata_rename_ops.saturating_sub(prev.metadata_rename_ops),
                                                        metadata_readdir_ops: curr.metadata_readdir_ops.saturating_sub(prev.metadata_readdir_ops),
                                                        metadata_fsync_ops: curr.metadata_fsync_ops.saturating_sub(prev.metadata_fsync_ops),
                                                        metadata_open_latency: curr.metadata_open_latency.clone(),
                                                        metadata_close_latency: curr.metadata_close_latency.clone(),
                                                        metadata_stat_latency: curr.metadata_stat_latency.clone(),
                                                        metadata_setattr_latency: curr.metadata_setattr_latency.clone(),
                                                        metadata_mkdir_latency: curr.metadata_mkdir_latency.clone(),
                                                        metadata_rmdir_latency: curr.metadata_rmdir_latency.clone(),
                                                        metadata_unlink_latency: curr.metadata_unlink_latency.clone(),
                                                        metadata_rename_latency: curr.metadata_rename_latency.clone(),
                                                        metadata_readdir_latency: curr.metadata_readdir_latency.clone(),
                                                        metadata_fsync_latency: curr.metadata_fsync_latency.clone(),
                                                        per_worker: None,
                                                    }
                                                })
                                                .collect()
                                        } else {
                                            // First heartbeat - use cumulative as-is
                                            cumulative_workers.clone()
                                        };
                                        
                                        // Store deltas for this timestamp (for JSON per-worker time-series)
                                        per_worker_time_series[node_idx].push(delta_workers.clone());
                                        
                                        // Convert to StatsSnapshot format for CSV per-worker output
                                        let delta_stats_snapshots: Vec<crate::worker::StatsSnapshot> = delta_workers.iter()
                                            .map(|agg| agg.to_stats_snapshot())
                                            .collect();
                                        delta_snapshot.per_worker = Some(delta_stats_snapshots);
                                        
                                        // Update previous cumulative
                                        previous_per_worker_cumulative[node_idx] = Some(cumulative_workers);
                                    }
                                }
                                
                                // Store delta snapshot for time-series
                                time_series_snapshots[node_idx].push(delta_snapshot);
                                
                                // Store current resource stats for this snapshot (from service heartbeat)
                                let heartbeat_resource_stats = crate::util::resource::ResourceStats {
                                    cpu_percent: hb.stats.cpu_percent,
                                    memory_bytes: hb.stats.memory_bytes,
                                    peak_memory_bytes: hb.stats.peak_memory_bytes,
                                };
                                
                                if self.config.runtime.debug {
                                    eprintln!("DEBUG: Heartbeat resource stats: CPU={:.1}%, Memory={} MB", 
                                        heartbeat_resource_stats.cpu_percent, 
                                        heartbeat_resource_stats.memory_bytes / 1_048_576);
                                }
                                
                                time_series_resource_stats[node_idx].push(heartbeat_resource_stats);
                            }
                            Ok(Ok(_)) => {
                                // Other message - ignore (shouldn't happen during test)
                            }
                            Ok(Err(e)) => {
                                // Error reading from node
                                eprintln!("Warning: Error reading from node {}: {}", node_idx, e);
                            }
                            Err(_) => {
                                // Timeout - no heartbeat received in 1 second
                                // This is normal if test is ending or node is slow
                            }
                        }
                    }
                }
                
                let total_snapshots: usize = time_series_snapshots.iter().map(|s| s.len()).sum();
                let max_per_node = time_series_snapshots.iter().map(|s| s.len()).max().unwrap_or(0);
                println!("Collected {} total snapshots ({} max per node)", total_snapshots, max_per_node);
            } else {
                // No time-series needed - but still need to drain heartbeats to avoid protocol errors
                println!("Waiting for test to complete (draining heartbeats)...");
                
                loop {
                    let elapsed = start_time.elapsed();
                    if elapsed >= test_duration {
                        break;
                    }
                    
                    // Drain heartbeats from all nodes (don't store them)
                    for (_node_idx, (_node_id, _addr, stream)) in connections.iter_mut().enumerate() {
                        match tokio::time::timeout(Duration::from_millis(100), read_message(stream)).await {
                            Ok(Ok(Message::Heartbeat(_))) => {
                                // Discard heartbeat
                            }
                            Ok(Ok(_)) => {
                                // Other message - ignore
                            }
                            Ok(Err(_)) | Err(_) => {
                                // Error or timeout - ignore
                            }
                        }
                    }
                    
                    // Sleep briefly to avoid busy loop
                    sleep(Duration::from_millis(100)).await;
                }
            }
        } else {
            // For other modes, wait a reasonable time
            sleep(Duration::from_secs(10)).await;
        }
        
        // Send STOP messages to all nodes
        println!();
        println!("Stopping test...");
        
        for (node_id, _addr, stream) in &mut connections {
            write_message(stream, &Message::Stop).await
                .with_context(|| format!("Failed to send STOP to node {}", node_id))?;
        }
        
        println!("Sent STOP to all nodes");
        
        // Give nodes time to complete in-flight operations
        sleep(Duration::from_millis(500)).await;
        
        // Collect RESULTS from all nodes
        println!();
        println!("Collecting results from all nodes...");
        
        let mut all_results = Vec::new();
        for (node_id, addr, stream) in &mut connections {
            // Read messages until we get RESULTS (skip any late HEARTBEATs)
            loop {
                let msg = read_message(stream).await
                    .with_context(|| format!("Failed to read from node {}", node_id))?;
                
                match msg {
                    Message::Results(results) => {
                        println!("  ✅ Received results from node {} ({} workers)", 
                            node_id, results.per_worker_stats.len());
                        all_results.push((*node_id, addr.clone(), results));
                        break;
                    }
                    Message::Heartbeat(_) => {
                        // Skip late heartbeats
                        continue;
                    }
                    Message::Error(err) => {
                        anyhow::bail!("Node {} reported error: {}", node_id, err.error);
                    }
                    other => {
                        anyhow::bail!("Expected RESULTS from node {}, got {:?}", node_id, other);
                    }
                }
            }
        }
        
        // Aggregate results
        println!();
        
        // Merge all node statistics into a single WorkerStats for display
        let enable_heatmap = self.config.workload.heatmap;
        let track_locks = self.config.targets.iter()
            .any(|t| t.lock_mode != crate::config::workload::FileLockMode::None);
        
        let mut merged_stats = crate::stats::WorkerStats::with_heatmap(track_locks, enable_heatmap);
        let mut max_duration_ns = 0u64;
        
        for (node_id, _addr, results) in &all_results {
            // Convert snapshot back to WorkerStats
            let node_stats = results.aggregate_stats.to_worker_stats(enable_heatmap, track_locks)
                .with_context(|| format!("Failed to deserialize stats from node {}", node_id))?;
            
            // Merge into aggregate
            merged_stats.merge(&node_stats)?;
            
            // Track max duration
            max_duration_ns = max_duration_ns.max(results.duration_ns);
        }
        
        let test_duration = Duration::from_nanos(max_duration_ns);
        
        // Use standalone's print_results() for consistent output
        crate::output::text::print_results(&merged_stats, test_duration, &self.config);
        
        // Write JSON output if requested
        if let Some(ref json_output_path) = self.config.output.json_output {
            println!();
            println!("Writing JSON output...");
            
            // Create output directory if it doesn't exist
            if let Some(parent) = json_output_path.parent() {
                std::fs::create_dir_all(parent)
                    .context("Failed to create JSON output directory")?;
            }
            
            // Determine if json_output_path is a directory or file
            let is_dir = json_output_path.is_dir() || 
                         json_output_path.to_string_lossy().ends_with('/') ||
                         !json_output_path.to_string_lossy().contains('.');
            
            if is_dir {
                // Create directory if needed
                std::fs::create_dir_all(json_output_path)
                    .context("Failed to create JSON output directory")?;
                
                // Write per-node JSON files
                for (node_idx, (node_id, addr, results)) in all_results.iter().enumerate() {
                    // Use IP address (without port) as filename - keep dots for proper IP notation
                    let fallback = format!("node{}", node_id);
                    let ip_addr = addr.split(':').next().unwrap_or(&fallback);
                    let node_filename = format!("{}.json", ip_addr);
                    let node_output_path = json_output_path.join(&node_filename);
                    
                    // Convert node stats to WorkerStats for JSON generation
                    let node_stats = results.aggregate_stats.to_worker_stats(enable_heatmap, track_locks)?;
                    
                    // Build per-worker stats for this node (only if --per-worker-output is enabled)
                    let per_worker_stats: Vec<(usize, WorkerStats)> = if self.config.output.per_worker_output {
                        results.per_worker_stats.iter()
                            .enumerate()
                            .map(|(i, snapshot)| {
                                let ws = snapshot.to_worker_stats(enable_heatmap, track_locks).unwrap_or_else(|_| crate::stats::WorkerStats::new());
                                (i, ws)
                            })
                            .collect()
                    } else {
                        Vec::new()  // Empty if flag not set
                    };
                    
                    let per_worker_refs: Vec<(usize, &WorkerStats)> = per_worker_stats.iter()
                        .map(|(id, stats)| (*id, stats))
                        .collect();
                    
                    // Get time-series snapshots for this node
                    let node_time_series = if node_idx < time_series_snapshots.len() {
                        time_series_snapshots[node_idx].clone()
                    } else {
                        Vec::new()
                    };
                    
                    // Calculate total blocks
                    let total_blocks = if !self.config.targets.is_empty() {
                        let file_size = self.config.targets[0].file_size.unwrap_or(0);
                        let block_size = self.config.workload.block_size;
                        if file_size > 0 && block_size > 0 {
                            Some(file_size / block_size)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    
                    // Build JSON output for this node
                    let node_resource_stats = if node_idx < time_series_resource_stats.len() {
                        time_series_resource_stats[node_idx].clone()
                    } else {
                        Vec::new()
                    };
                    
                    // Get per-worker time-series for this node (if enabled)
                    let node_per_worker_time_series = if node_idx < per_worker_time_series.len() {
                        per_worker_time_series[node_idx].clone()
                    } else {
                        Vec::new()
                    };
                    
                    // Extract IP address (without port) for node_id
                    let ip_addr = addr.split(':').next().unwrap_or(addr);
                    let ip_addr = if ip_addr == "localhost" { "127.0.0.1" } else { ip_addr }.to_string();
                    
                    let node_output = crate::output::json::build_node_output(
                        ip_addr.clone(),  // Use IP only as node_id
                        Some(addr.clone()),  // Keep full address as hostname
                        std::time::SystemTime::now() - test_duration,
                        std::time::SystemTime::now(),
                        test_duration,
                        &self.config,
                        node_time_series,  // Include time-series data
                        node_resource_stats,  // Per-snapshot resource stats
                        node_per_worker_time_series,  // Per-worker time-series (NEW)
                        &node_stats,
                        &per_worker_refs,
                        total_blocks,
                    );
                    
                    // Write node JSON file
                    if let Err(e) = crate::output::json::write_json_output(&node_output_path, &node_output, true) {
                        eprintln!("Warning: Failed to write JSON for node {}: {}", addr, e);
                    } else {
                        println!("  ✅ Node {} JSON: {}", addr, node_output_path.display());
                    }
                }
                
                // Write aggregate JSON file
                let aggregate_path = json_output_path.join("aggregate.json");
                
                // Collect ALL per-worker stats from ALL nodes (for true per-worker breakdown)
                let all_per_worker_stats: Vec<(String, usize, WorkerStats)> = all_results.iter()
                    .flat_map(|(_node_id, addr, results)| {
                        let ip_addr = addr.split(':').next().unwrap_or(addr).to_string();
                        results.per_worker_stats.iter().enumerate().map(move |(worker_id, snapshot)| {
                            let worker_stats = snapshot.to_worker_stats(enable_heatmap, track_locks)
                                .unwrap_or_else(|_| crate::stats::WorkerStats::new());
                            (ip_addr.clone(), worker_id, worker_stats)
                        }).collect::<Vec<_>>()
                    })
                    .collect();
                
                let all_per_worker_refs: Vec<(String, usize, &WorkerStats)> = all_per_worker_stats.iter()
                    .map(|(node_id, worker_id, stats)| (node_id.clone(), *worker_id, stats))
                    .collect();
                
                let total_blocks = if !self.config.targets.is_empty() {
                    let file_size = self.config.targets[0].file_size.unwrap_or(0);
                    let block_size = self.config.workload.block_size;
                    if file_size > 0 && block_size > 0 {
                        Some(file_size / block_size)
                    } else {
                        None
                    }
                } else {
                    None
                };
                
                // Prepare per-node time-series data for aggregate
                let all_node_snapshots: Vec<(String, Vec<crate::output::json::AggregatedSnapshot>)> = 
                    all_results.iter()
                        .enumerate()
                        .filter_map(|(node_idx, (_node_id, addr, _results))| {
                            let ip_addr = addr.split(':').next().unwrap_or(addr).to_string();
                            if node_idx < time_series_snapshots.len() {
                                Some((ip_addr, time_series_snapshots[node_idx].clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                
                let all_node_resource_stats: Vec<(String, Vec<crate::util::resource::ResourceStats>)> = 
                    all_results.iter()
                        .enumerate()
                        .filter_map(|(node_idx, (_node_id, addr, _results))| {
                            let ip_addr = addr.split(':').next().unwrap_or(addr).to_string();
                            if node_idx < time_series_resource_stats.len() {
                                Some((ip_addr, time_series_resource_stats[node_idx].clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                
                // Prepare per-worker time-series data for aggregate (NEW)
                let all_per_worker_time_series: Vec<(String, Vec<Vec<crate::output::json::AggregatedSnapshot>>)> = 
                    all_results.iter()
                        .enumerate()
                        .filter_map(|(node_idx, (_node_id, addr, _results))| {
                            let ip_addr = addr.split(':').next().unwrap_or(addr).to_string();
                            if node_idx < per_worker_time_series.len() {
                                Some((ip_addr, per_worker_time_series[node_idx].clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                
                let aggregate_output = crate::output::json::build_aggregate_node_output(
                    "aggregate".to_string(),
                    None,
                    std::time::SystemTime::now() - test_duration,
                    std::time::SystemTime::now(),
                    test_duration,
                    &self.config,
                    all_node_snapshots,  // Per-node time-series data
                    all_node_resource_stats,  // Per-node resource stats
                    all_per_worker_time_series,  // Per-worker time-series (NEW)
                    &merged_stats,
                    &all_per_worker_refs,  // ALL per-worker stats from ALL nodes
                    total_blocks,
                );
                
                if let Err(e) = crate::output::json::write_json_output(&aggregate_path, &aggregate_output, true) {
                    eprintln!("Warning: Failed to write aggregate JSON: {}", e);
                } else {
                    println!("  ✅ Aggregate JSON: {}", aggregate_path.display());
                }
                
                println!();
                println!("JSON output written to: {}", json_output_path.display());
            } else {
                // Single file output - just write aggregate
                let _total_blocks = if !self.config.targets.is_empty() {
                    let file_size = self.config.targets[0].file_size.unwrap_or(0);
                    let block_size = self.config.workload.block_size;
                    if file_size > 0 && block_size > 0 {
                        Some(file_size / block_size)
                    } else {
                        None
                    }
                } else {
                    None
                };
                
                // Collect ALL per-worker stats from ALL nodes (for true per-worker breakdown)
                let all_per_worker_stats: Vec<(String, usize, WorkerStats)> = all_results.iter()
                    .flat_map(|(_node_id, addr, results)| {
                        let ip_addr = addr.split(':').next().unwrap_or(addr);
                        let ip_addr = if ip_addr == "localhost" { "127.0.0.1" } else { ip_addr }.to_string();
                        results.per_worker_stats.iter().enumerate().map(move |(worker_id, snapshot)| {
                            let worker_stats = snapshot.to_worker_stats(enable_heatmap, track_locks)
                                .unwrap_or_else(|_| crate::stats::WorkerStats::new());
                            (ip_addr.clone(), worker_id, worker_stats)
                        }).collect::<Vec<_>>()
                    })
                    .collect();
                
                let all_per_worker_refs: Vec<(String, usize, &WorkerStats)> = all_per_worker_stats.iter()
                    .map(|(node_id, worker_id, stats)| (node_id.clone(), *worker_id, stats))
                    .collect();
                
                let total_blocks = if !self.config.targets.is_empty() {
                    let file_size = self.config.targets[0].file_size.unwrap_or(0);
                    let block_size = self.config.workload.block_size;
                    if file_size > 0 && block_size > 0 {
                        Some(file_size / block_size)
                    } else {
                        None
                    }
                } else {
                    None
                };
                
                // Prepare per-node time-series data for aggregate
                let all_node_snapshots: Vec<(String, Vec<crate::output::json::AggregatedSnapshot>)> = 
                    all_results.iter()
                        .enumerate()
                        .filter_map(|(node_idx, (_node_id, addr, _results))| {
                            let ip_addr = addr.split(':').next().unwrap_or(addr);
                            let ip_addr = if ip_addr == "localhost" { "127.0.0.1" } else { ip_addr }.to_string();
                            if node_idx < time_series_snapshots.len() {
                                Some((ip_addr, time_series_snapshots[node_idx].clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                
                let all_node_resource_stats: Vec<(String, Vec<crate::util::resource::ResourceStats>)> = 
                    all_results.iter()
                        .enumerate()
                        .filter_map(|(node_idx, (_node_id, addr, _results))| {
                            let ip_addr = addr.split(':').next().unwrap_or(addr);
                            let ip_addr = if ip_addr == "localhost" { "127.0.0.1" } else { ip_addr }.to_string();
                            if node_idx < time_series_resource_stats.len() {
                                Some((ip_addr, time_series_resource_stats[node_idx].clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                
                // Prepare per-worker time-series data for aggregate (NEW)
                let all_per_worker_time_series: Vec<(String, Vec<Vec<crate::output::json::AggregatedSnapshot>>)> = 
                    all_results.iter()
                        .enumerate()
                        .filter_map(|(node_idx, (_node_id, addr, _results))| {
                            let ip_addr = addr.split(':').next().unwrap_or(addr);
                            let ip_addr = if ip_addr == "localhost" { "127.0.0.1" } else { ip_addr }.to_string();
                            if node_idx < per_worker_time_series.len() {
                                Some((ip_addr, per_worker_time_series[node_idx].clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                
                let aggregate_output = crate::output::json::build_aggregate_node_output(
                    if all_results.len() == 1 {
                        // Single node - use actual node address
                        all_results[0].1.clone()
                    } else {
                        // Multiple nodes - use "aggregate"
                        "aggregate".to_string()
                    },
                    None,
                    std::time::SystemTime::now() - test_duration,
                    std::time::SystemTime::now(),
                    test_duration,
                    &self.config,
                    all_node_snapshots,  // Per-node time-series data
                    all_node_resource_stats,  // Per-node resource stats
                    all_per_worker_time_series,  // Per-worker time-series (NEW)
                    &merged_stats,
                    &all_per_worker_refs,  // ALL per-worker stats from ALL nodes
                    total_blocks,
                );
                
                if let Err(e) = crate::output::json::write_json_output(json_output_path, &aggregate_output, true) {
                    eprintln!("Warning: Failed to write JSON output: {}", e);
                } else {
                    println!();
                    println!("JSON output written to: {}", json_output_path.display());
                }
            }
        }
        
        // Write histogram output if requested
        if self.config.output.json_histogram {
            if let Some(ref json_output_path) = self.config.output.json_output {
                println!();
                println!("Writing histogram output...");
                
                // Determine histogram path based on JSON output path
                let histogram_path = if json_output_path.is_dir() || 
                                       json_output_path.to_string_lossy().ends_with('/') ||
                                       !json_output_path.to_string_lossy().contains('.') {
                    // Directory output - put histogram in the directory
                    json_output_path.join("histogram.json")
                } else {
                    // File output - create histogram file next to it
                    let stem = json_output_path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("output");
                    json_output_path.with_file_name(format!("{}_histogram.json", stem))
                };
                
                // Export histogram from merged stats
                let histogram_output = crate::output::json::export_histogram(
                    "aggregate".to_string(),
                    &merged_stats,
                );
                
                // Write histogram file
                if let Err(e) = crate::output::json::write_histogram_output(&histogram_path, &histogram_output, true) {
                    eprintln!("Warning: Failed to write histogram output: {}", e);
                } else {
                    println!("  ✅ Histogram exported: {}", histogram_path.display());
                }
            }
        }
        
        // Write CSV output if requested
        if let Some(ref csv_output_path) = self.config.output.csv_output {
            if !time_series_snapshots.is_empty() && time_series_snapshots.iter().any(|s| !s.is_empty()) {
                println!();
                println!("Writing CSV output...");
                
                // Determine if csv_output_path is a directory or file
                let is_dir = csv_output_path.is_dir() || 
                             csv_output_path.to_string_lossy().ends_with('/') ||
                             !csv_output_path.to_string_lossy().contains('.');
                
                if is_dir {
                    // Create directory if needed
                    std::fs::create_dir_all(csv_output_path)
                        .context("Failed to create CSV output directory")?;
                    
                    // Write per-node CSV files
                    for (node_idx, (node_id, addr, _results)) in all_results.iter().enumerate() {
                        if time_series_snapshots[node_idx].is_empty() {
                            continue;  // Skip nodes with no snapshots
                        }
                        
                        let fallback = format!("node{}", node_id);
                        let ip_addr = addr.split(':').next().unwrap_or(&fallback);
                        let csv_filename = format!("{}.csv", ip_addr);
                        let csv_path = csv_output_path.join(&csv_filename);
                        
                        // Create CSV writer (per-node file)
                        let mut csv_writer = crate::output::csv::CsvWriter::new_with_node_id(&csv_path, self.config.output.per_worker_output, false)
                            .context("Failed to create CSV writer")?;
                        
                        // Write all snapshots for this node
                        // Calculate actual interval between snapshots (not fixed 1.0s)
                        let mut prev_elapsed = Duration::from_secs(0);
                        for (i, snapshot) in time_series_snapshots[node_idx].iter().enumerate() {
                            // Calculate interval since previous snapshot
                            let interval_duration = snapshot.elapsed - prev_elapsed;
                            let interval_secs = interval_duration.as_secs_f64();
                            prev_elapsed = snapshot.elapsed;
                            
                            // Get resource stats for this snapshot (if available)
                            let resource_stats = time_series_resource_stats[node_idx].get(i);
                            
                            csv_writer.append_snapshot(snapshot, interval_secs, resource_stats)
                                .context("Failed to write CSV row")?;
                        }
                        
                        println!("  ✅ Node {} CSV: {}", addr, csv_path.display());
                    }
                    
                    // Write aggregate CSV (with per-node rows, and per-worker if enabled)
                    let aggregate_csv_path = csv_output_path.join("aggregate.csv");
                    let mut csv_writer = crate::output::csv::CsvWriter::new_with_node_id(&aggregate_csv_path, self.config.output.per_worker_output, true)
                        .context("Failed to create aggregate CSV writer")?;
                    
                    // Find max number of snapshots across all nodes
                    let max_snapshots = time_series_snapshots.iter()
                        .map(|s| s.len())
                        .max()
                        .unwrap_or(0);
                    
                    // Write per-node rows at each timestamp
                    for i in 0..max_snapshots {
                        // Write one row per node at this timestamp
                        for (node_idx, (_node_id, addr, _results)) in all_results.iter().enumerate() {
                            if let Some(snapshot) = time_series_snapshots.get(node_idx).and_then(|s| s.get(i)) {
                                // Calculate interval since previous snapshot for this node
                                let prev_elapsed = if i > 0 {
                                    time_series_snapshots[node_idx].get(i - 1)
                                        .map(|s| s.elapsed)
                                        .unwrap_or(Duration::from_secs(0))
                                } else {
                                    Duration::from_secs(0)
                                };
                                let interval_duration = snapshot.elapsed - prev_elapsed;
                                let interval_secs = interval_duration.as_secs_f64();
                                
                                // Get resource stats for this snapshot (if available)
                                let resource_stats = time_series_resource_stats.get(node_idx)
                                    .and_then(|stats| stats.get(i));
                                
                                // Extract IP address (without port), convert localhost to 127.0.0.1
                                let ip_addr = addr.split(':').next().unwrap_or(addr);
                                let ip_addr = if ip_addr == "localhost" { "127.0.0.1" } else { ip_addr };
                                
                                csv_writer.append_snapshot_with_node(ip_addr, snapshot, interval_secs, resource_stats, self.config.workers.threads)
                                    .context("Failed to write CSV row")?;
                            }
                        }
                    }
                    
                    println!("  ✅ Aggregate CSV: {}", aggregate_csv_path.display());
                    println!();
                    println!("CSV output written to: {}", csv_output_path.display());
                } else {
                    // Single file output - write per-node rows with node_id column (ALWAYS, even for 1 node)
                    let mut csv_writer = crate::output::csv::CsvWriter::new_with_node_id(csv_output_path, self.config.output.per_worker_output, true)
                        .context("Failed to create CSV writer")?;
                    
                    // Find max number of snapshots across all nodes
                    let max_snapshots = time_series_snapshots.iter()
                        .map(|s| s.len())
                        .max()
                        .unwrap_or(0);
                    
                    // Write per-node rows at each timestamp
                    for i in 0..max_snapshots {
                        for (node_idx, (_node_id, addr, _results)) in all_results.iter().enumerate() {
                            if let Some(snapshot) = time_series_snapshots.get(node_idx).and_then(|s| s.get(i)) {
                                // Calculate interval since previous snapshot for this node
                                let prev_elapsed = if i > 0 {
                                    time_series_snapshots[node_idx].get(i - 1)
                                        .map(|s| s.elapsed)
                                        .unwrap_or(Duration::from_secs(0))
                                } else {
                                    Duration::from_secs(0)
                                };
                                let interval_duration = snapshot.elapsed - prev_elapsed;
                                let interval_secs = interval_duration.as_secs_f64();
                                
                                // Get resource stats for this snapshot (if available)
                                let resource_stats = time_series_resource_stats.get(node_idx)
                                    .and_then(|stats| stats.get(i));
                                
                                // Extract IP address (without port), convert localhost to 127.0.0.1
                                let ip_addr = addr.split(':').next().unwrap_or(addr);
                                let ip_addr = if ip_addr == "localhost" { "127.0.0.1" } else { ip_addr };
                                
                                csv_writer.append_snapshot_with_node(ip_addr, snapshot, interval_secs, resource_stats, self.config.workers.threads)
                                    .context("Failed to write CSV row")?;
                            }
                        }
                    }
                    
                    println!("CSV output written to: {}", csv_output_path.display());
                }
            } else {
                eprintln!("Warning: No time-series data collected (heartbeats may not have been received)");
                eprintln!("         CSV output requires time-series data");
            }
        }
        
        Ok(())
    }
    
    /// Distributed pre-allocation
    ///
    /// Partitions file across nodes and has each node pre-allocate its region in parallel.
    /// Much faster than coordinator pre-allocating alone.
    async fn distributed_preallocate(
        &self,
        connections: &mut [(usize, String, TcpStream)],
        fill_files: bool,
    ) -> Result<()> {
        let num_nodes = connections.len();
        
        // For each target, partition and distribute
        for target in &self.config.targets {
            let file_size = target.file_size.ok_or_else(|| anyhow::anyhow!("File size required for pre-allocation"))?;
            
            println!("Distributing pre-allocation for: {}", target.path.display());
            println!("  File size: {} bytes", file_size);
            println!("  Nodes: {}", num_nodes);
            
            // Calculate region size per node
            let region_size = file_size / num_nodes as u64;
            
            // Send PrepareFiles to each node
            for (node_id, addr, stream) in connections.iter_mut() {
                let start_offset = *node_id as u64 * region_size;
                let end_offset = if *node_id == num_nodes - 1 {
                    file_size  // Last node gets remainder
                } else {
                    start_offset + region_size
                };
                
                println!("  Node {}: bytes {}-{} ({} MB)", 
                    node_id, start_offset, end_offset,
                    (end_offset - start_offset) / 1_000_000);
                
                let prepare_msg = PrepareFilesMessage {
                    protocol_version: PROTOCOL_VERSION,
                    node_id: addr.clone(),
                    file_list: vec![target.path.clone()],
                    file_size: end_offset - start_offset,
                    start_offset,
                    fill_pattern: self.config.workload.write_pattern,
                    fill_files,
                };
                
                write_message(stream, &Message::PrepareFiles(prepare_msg)).await
                    .with_context(|| format!("Failed to send PrepareFiles to node {}", node_id))?;
            }
            
            // Wait for all nodes to complete (barrier)
            println!();
            println!("Waiting for all nodes to complete pre-allocation...");
            
            // Measure total barrier time
            let barrier_start = std::time::Instant::now();
            
            // Read responses from all nodes (in any order)
            let mut received = vec![false; connections.len()];
            let mut responses = Vec::new();
            
            while responses.len() < connections.len() {
                // Try each connection that hasn't responded yet
                for (i, (node_id, _addr, stream)) in connections.iter_mut().enumerate() {
                    if received[i] {
                        continue;  // Already got response from this node
                    }
                    
                    // Try to read with timeout
                    match tokio::time::timeout(Duration::from_millis(100), read_message(stream)).await {
                        Ok(Ok(msg)) => {
                            received[i] = true;
                            responses.push((*node_id, msg));
                            
                            // Process immediately
                            match &responses.last().unwrap().1 {
                                Message::FilesReady(ready) => {
                                    println!("  ✅ Node {} ready ({} files, {:.2}s actual)", 
                                        node_id, ready.files_created,
                                        ready.duration_ns as f64 / 1_000_000_000.0);
                                }
                                Message::Error(err) => {
                                    anyhow::bail!("Node {} reported error: {}", node_id, err.error);
                                }
                                other => {
                                    anyhow::bail!("Expected FilesReady from node {}, got {:?}", node_id, other);
                                }
                            }
                            
                            break;  // Got a response, start over to check all nodes
                        }
                        Ok(Err(e)) => {
                            anyhow::bail!("Failed to read from node {}: {}", node_id, e);
                        }
                        Err(_) => {
                            // Timeout - no data ready from this node yet
                            continue;
                        }
                    }
                }
            }
            
            let barrier_elapsed = barrier_start.elapsed();
            println!("  ✅ All nodes completed pre-allocation (barrier time: {:.2}s)", barrier_elapsed.as_secs_f64());
        }
        
        Ok(())
    }
}


/// Check if a file is sparse
fn is_file_sparse(path: &std::path::Path) -> Result<bool> {
    let metadata = std::fs::metadata(path)?;
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let logical_size = metadata.len();
        let allocated_size = metadata.blocks() * 512;
        
        // If allocated < 10% of logical, it's sparse
        Ok(logical_size == 0 || allocated_size < (logical_size / 10))
    }
    
    #[cfg(not(unix))]
    {
        Ok(metadata.len() == 0)
    }
}

/// Convert WorkerStatsSnapshot to AggregatedSnapshot for time-series
///
/// This is a simplified conversion used for heartbeat data.
/// We don't have per-worker snapshots in heartbeats, so per_worker is None.
fn worker_snapshot_to_aggregated(
    snapshot: &crate::distributed::protocol::WorkerStatsSnapshot,
    elapsed: Duration,
) -> crate::output::json::AggregatedSnapshot {
    use crate::stats::simple_histogram::SimpleHistogram;
    
    // Deserialize histograms
    let read_latency: SimpleHistogram = bincode::deserialize(&snapshot.read_latency_histogram)
        .unwrap_or_else(|_| SimpleHistogram::new());
    let write_latency: SimpleHistogram = bincode::deserialize(&snapshot.write_latency_histogram)
        .unwrap_or_else(|_| SimpleHistogram::new());
    
    // Deserialize metadata latency histograms
    let metadata_open_latency: SimpleHistogram = bincode::deserialize(&snapshot.metadata_open_latency)
        .unwrap_or_else(|_| SimpleHistogram::new());
    let metadata_close_latency: SimpleHistogram = bincode::deserialize(&snapshot.metadata_close_latency)
        .unwrap_or_else(|_| SimpleHistogram::new());
    let metadata_stat_latency: SimpleHistogram = bincode::deserialize(&snapshot.metadata_stat_latency)
        .unwrap_or_else(|_| SimpleHistogram::new());
    let metadata_setattr_latency: SimpleHistogram = bincode::deserialize(&snapshot.metadata_setattr_latency)
        .unwrap_or_else(|_| SimpleHistogram::new());
    let metadata_mkdir_latency: SimpleHistogram = bincode::deserialize(&snapshot.metadata_mkdir_latency)
        .unwrap_or_else(|_| SimpleHistogram::new());
    let metadata_rmdir_latency: SimpleHistogram = bincode::deserialize(&snapshot.metadata_rmdir_latency)
        .unwrap_or_else(|_| SimpleHistogram::new());
    let metadata_unlink_latency: SimpleHistogram = bincode::deserialize(&snapshot.metadata_unlink_latency)
        .unwrap_or_else(|_| SimpleHistogram::new());
    let metadata_rename_latency: SimpleHistogram = bincode::deserialize(&snapshot.metadata_rename_latency)
        .unwrap_or_else(|_| SimpleHistogram::new());
    let metadata_readdir_latency: SimpleHistogram = bincode::deserialize(&snapshot.metadata_readdir_latency)
        .unwrap_or_else(|_| SimpleHistogram::new());
    let metadata_fsync_latency: SimpleHistogram = bincode::deserialize(&snapshot.metadata_fsync_latency)
        .unwrap_or_else(|_| SimpleHistogram::new());
    
    crate::output::json::AggregatedSnapshot {
        timestamp: std::time::SystemTime::now(),
        elapsed,
        read_ops: snapshot.read_ops,
        write_ops: snapshot.write_ops,
        read_bytes: snapshot.read_bytes,
        write_bytes: snapshot.write_bytes,
        errors: snapshot.errors,
        avg_latency_us: if snapshot.read_ops + snapshot.write_ops > 0 {
            read_latency.mean().as_micros() as f64
        } else {
            0.0
        },
        read_latency,
        write_latency,
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
        per_worker: None,  // Heartbeats don't include per-worker data
    }
}

/// Validate and fill sparse files in parallel
///
/// Checks each file in the list and fills it with the specified pattern if it's sparse (0 bytes on disk).
/// Uses rayon for parallel processing with progress updates every 1000 files.
///
/// Returns the number of files that were filled.
fn validate_and_fill_files(
    file_list: &[std::path::PathBuf],
    file_size: u64,
    pattern: crate::config::workload::VerifyPattern,
) -> Result<usize> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    
    let filled_count = AtomicUsize::new(0);
    let processed_count = AtomicUsize::new(0);
    let total_files = file_list.len();
    
    // Process files in parallel
    file_list.par_iter().try_for_each(|path| -> Result<()> {
        // Check if file exists and is sparse
        let needs_fill = if let Ok(metadata) = std::fs::metadata(path) {
            // File exists - check if it's sparse
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let logical_size = metadata.len();
                let allocated_size = metadata.blocks() * 512;
                
                // If file is empty or allocated size is less than 10% of logical size, it's sparse
                logical_size == 0 || allocated_size < (logical_size / 10)
            }
            #[cfg(not(unix))]
            {
                metadata.len() == 0
            }
        } else {
            // File doesn't exist - needs creation and filling
            true
        };
        
        if needs_fill {
            // Create and fill the file
            use crate::target::file::FileTarget;
            use crate::target::Target;
            use crate::target::OpenFlags;
            
            let mut target = FileTarget::new(path.clone(), Some(file_size));
            
            let flags = OpenFlags {
                direct: false,
                sync: false,
                create: true,
                truncate: false,
            };
            
            target.open(flags)?;
            target.refill(pattern)?;
            target.close()?;
            
            filled_count.fetch_add(1, Ordering::Relaxed);
        }
        
        // Update progress
        let processed = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
        if processed % 1000 == 0 || processed == total_files {
            println!("  Progress: {}/{} files validated...", processed, total_files);
        }
        
        Ok(())
    })?;
    
    Ok(filled_count.load(Ordering::Relaxed))
}
