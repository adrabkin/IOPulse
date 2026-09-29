//! Node service for distributed mode
//!
//! This module implements the node service that runs on each node in distributed mode.
//! The node service:
//! - Listens for connections from the coordinator
//! - Receives test configuration
//! - Spawns worker threads
//! - Sends periodic heartbeats
//! - Implements dead man's switch (self-stop if coordinator disappears)
//! - Sends final results

use crate::distributed::protocol::*;
use crate::stats::WorkerStats;
use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;

/// Node service
///
/// Runs on each node in distributed mode, accepting commands from coordinator.
pub struct NodeService {
    /// Port to listen on
    listen_port: u16,
    
    /// Node identifier (IP address or hostname)
    node_id: String,
}

impl NodeService {
    /// Create a new node service
    pub fn new(listen_port: u16) -> Result<Self> {
        // Get node ID (IP address or hostname)
        let node_id = get_node_id()?;
        
        Ok(Self {
            listen_port,
            node_id,
        })
    }
    
    /// Run the node service
    ///
    /// Listens for connections from coordinator and handles test execution.
    pub async fn run(self) -> Result<()> {
        let addr = format!("0.0.0.0:{}", self.listen_port);
        let listener = TcpListener::bind(&addr).await
            .context("Failed to bind node service")?;
        
        println!("Node service listening on port {}", self.listen_port);
        println!("Node ID: {}", self.node_id);
        println!("Waiting for coordinator connection...");
        
        loop {
            // Accept connection from coordinator
            let (stream, addr) = listener.accept().await
                .context("Failed to accept connection")?;
            
            println!("Coordinator connected from: {}", addr);
            
            // Handle this test (blocks until test completes)
            if let Err(e) = self.handle_test(stream).await {
                eprintln!("Test failed: {}", e);
            }
            
            println!("Test complete. Waiting for next connection...");
        }
    }
    
    /// Handle a single test execution
    async fn handle_test(&self, mut stream: TcpStream) -> Result<()> {
        // Check if first message is PrepareFiles or Config
        println!("Waiting for first message (PrepareFiles or CONFIG)...");
        let first_msg = read_message(&mut stream).await?;
        
        match first_msg {
            Message::PrepareFiles(prepare_msg) => {
                // Handle file preparation
                println!("Received PrepareFiles message");
                self.handle_prepare_files(&mut stream, prepare_msg).await?;
                
                // Now wait for CONFIG message
                println!("Waiting for CONFIG message...");
                let config_msg = match read_message(&mut stream).await {
                    Ok(Message::Config(msg)) => {
                        println!("Received CONFIG message successfully");
                        msg
                    }
                    Ok(other) => {
                        let err = format!("Expected CONFIG message, got {:?}", other);
                        eprintln!("{}", err);
                        anyhow::bail!(err)
                    }
                    Err(e) => {
                        eprintln!("Failed to read/deserialize CONFIG: {:#}", e);
                        anyhow::bail!("Failed to deserialize message: {:#}", e)
                    }
                };
                
                self.handle_test_execution(stream, config_msg).await
            }
            Message::Config(config_msg) => {
                // No file preparation needed, proceed directly to test
                println!("Received CONFIG message successfully");
                self.handle_test_execution(stream, config_msg).await
            }
            other => {
                anyhow::bail!("Expected PrepareFiles or CONFIG, got {:?}", other)
            }
        }
    }
    
    /// Handle file preparation (distributed filling)
    async fn handle_prepare_files(&self, stream: &mut TcpStream, prepare_msg: PrepareFilesMessage) -> Result<()> {
        use std::time::Instant;
        
        // Validate protocol version
        if prepare_msg.protocol_version != PROTOCOL_VERSION {
            let error = ErrorMessage {
                node_id: self.node_id.clone(),
                error: format!("Protocol version mismatch: coordinator={}, node={}", 
                    prepare_msg.protocol_version, PROTOCOL_VERSION),
                elapsed_ns: 0,
            };
            write_message(stream, &Message::Error(error)).await?;
            anyhow::bail!("Protocol version mismatch");
        }
        
        println!("  Files to prepare: {}", prepare_msg.file_list.len());
        println!("  File size/region: {} bytes", prepare_msg.file_size);
        println!("  Start offset: {}", prepare_msg.start_offset);
        println!("  Fill files: {}", prepare_msg.fill_files);
        println!("  Pattern: {:?}", prepare_msg.fill_pattern);
        
        let start = Instant::now();
        
        // Create/fill files or regions
        let (files_created, files_filled) = if prepare_msg.start_offset > 0 {
            // Region pre-allocation (distributed mode)
            preallocate_region(
                &prepare_msg.file_list[0],
                prepare_msg.start_offset,
                prepare_msg.file_size,
                prepare_msg.fill_files,
                prepare_msg.fill_pattern,
            )?
        } else if prepare_msg.fill_files {
            // Full file filling
            let filled = validate_and_fill_files_distributed(
                &prepare_msg.file_list,
                prepare_msg.file_size,
                prepare_msg.fill_pattern,
            )?;
            (prepare_msg.file_list.len(), filled)
        } else {
            // Just create empty files
            let created = create_files_distributed(
                &prepare_msg.file_list,
                prepare_msg.file_size,
            )?;
            (created, 0)
        };
        
        let duration = start.elapsed();
        println!("  ✅ Prepared {} files ({} filled) in {:.2}s", 
            files_created, files_filled, duration.as_secs_f64());
        
        // Send FilesReady message
        let ready = FilesReadyMessage {
            protocol_version: PROTOCOL_VERSION,
            node_id: self.node_id.clone(),
            files_created,
            files_filled,
            duration_ns: duration.as_nanos() as u64,
        };
        write_message(stream, &Message::FilesReady(ready)).await?;
        println!("Sent FilesReady message");
        
        Ok(())
    }
    
    /// Handle test execution (after files are prepared)
    async fn handle_test_execution(&self, mut stream: TcpStream, config_msg: ConfigMessage) -> Result<()> {
        
        // Validate protocol version
        if config_msg.protocol_version != PROTOCOL_VERSION {
            let error = ErrorMessage {
                node_id: self.node_id.clone(),
                error: format!("Protocol version mismatch: coordinator={}, node={}", 
                    config_msg.protocol_version, PROTOCOL_VERSION),
                elapsed_ns: 0,
            };
            write_message(&mut stream, &Message::Error(error)).await?;
            anyhow::bail!("Protocol version mismatch");
        }
        
        println!("Received configuration:");
        println!("  Protocol version: {}", config_msg.protocol_version);
        let num_workers = config_msg.config.workers.threads;
        println!("  Worker threads: {}", num_workers);
        println!("  Worker ID range: {}-{}", config_msg.worker_id_start, config_msg.worker_id_end);
        println!("  Skip preallocation: {}", config_msg.skip_preallocation);
        
        if let Some(ref file_list) = config_msg.file_list {
            println!("  File list: {} files", file_list.len());
            if let Some((start, end)) = config_msg.file_range {
                println!("  File range: {}-{} ({} files)", start, end, end - start);
            }
        }
        
        // Prepare workers (spawn threads in separate task)
        println!("Preparing {} worker threads...", num_workers);
        
        // Modify config to skip preallocation if coordinator already did it
        let mut config = config_msg.config;
        if config_msg.skip_preallocation {
            for target in &mut config.targets {
                target.preallocate = false;
                target.no_refill = true;  // Also skip auto-refill
            }
        }
        
        // Create shared state for workers
        use std::sync::{Arc, Mutex};
        use std::sync::atomic::{AtomicBool, Ordering};
        
        let stop_flag = Arc::new(AtomicBool::new(false));
        
        // Create shared snapshots for live stats (like standalone mode)
        let shared_snapshots: Arc<Mutex<Vec<crate::worker::StatsSnapshot>>> = Arc::new(Mutex::new(
            vec![crate::worker::StatsSnapshot {
                read_ops: 0,
                write_ops: 0,
                read_bytes: 0,
                write_bytes: 0,
                errors: 0,
                avg_latency_us: 0.0,
                read_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
                write_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
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
                metadata_open_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
                metadata_close_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
                metadata_stat_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
                metadata_setattr_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
                metadata_mkdir_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
                metadata_rmdir_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
                metadata_unlink_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
                metadata_rename_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
                metadata_readdir_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
                metadata_fsync_latency: crate::stats::simple_histogram::SimpleHistogram::new(),
            }; num_workers]
        ));
        
        // Also keep final stats for RESULTS message
        let worker_stats: Arc<Mutex<Vec<crate::stats::WorkerStats>>> = Arc::new(Mutex::new(Vec::new()));
        
        // Spawn workers in a separate thread (not async)
        let config = Arc::new(config);
        let config_for_heartbeat = config.clone();  // Clone for heartbeat task
        let config_for_results = config.clone();  // Clone for results collection
        let stop_flag_clone = stop_flag.clone();
        let worker_stats_clone = worker_stats.clone();
        let shared_snapshots_clone = shared_snapshots.clone();  // For workers to update
        let file_list = config_msg.file_list.clone().map(Arc::new);
        let file_range = config_msg.file_range;
        let worker_id_start = config_msg.worker_id_start;
        let worker_id_end = config_msg.worker_id_end;
        
        let worker_handle = std::thread::spawn(move || {
            spawn_workers(
                config,
                file_list,
                file_range,
                worker_id_start,
                worker_id_end,
                stop_flag_clone,
                worker_stats_clone,
                shared_snapshots_clone,  // Pass to workers
            )
        });
        
        // Send READY message
        let ready = ReadyMessage {
            protocol_version: PROTOCOL_VERSION,
            node_id: self.node_id.clone(),
            num_workers,
            ready: true,
        };
        write_message(&mut stream, &Message::Ready(ready)).await?;
        println!("Sent READY message");
        
        // Wait for START message
        let start_msg = match read_message(&mut stream).await? {
            Message::Start(msg) => msg,
            other => anyhow::bail!("Expected START message, got {:?}", other),
        };
        
        println!("Received START message: timestamp={}", start_msg.start_timestamp_ns);
        
        // Wait until start timestamp
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos() as u64;
        
        if start_msg.start_timestamp_ns > now_ns {
            let wait_ns = start_msg.start_timestamp_ns - now_ns;
            let wait_duration = Duration::from_nanos(wait_ns);
            println!("Waiting {}ms until start time...", wait_duration.as_millis());
            sleep(wait_duration).await;
        }
        
        println!("Starting IO operations...");
        let test_start = std::time::Instant::now();
        
        // Initialize resource tracker for CPU/memory monitoring
        let resource_tracker = Arc::new(Mutex::new({
            let mut tracker = crate::util::resource::ResourceTracker::new();
            tracker.start();
            tracker.sample();  // Initial baseline
            tracker
        }));
        
        // Split stream for concurrent read/write
        let (read_half, write_half) = stream.into_split();
        let read_half = Arc::new(tokio::sync::Mutex::new(read_half));
        let write_half = Arc::new(tokio::sync::Mutex::new(write_half));
        
        // Start heartbeat task
        let heartbeat_handle = {
            let node_id = self.node_id.clone();
            let stop_flag = stop_flag.clone();
            let shared_snapshots = shared_snapshots.clone();  // Use shared snapshots
            let write_half = write_half.clone();
            let resource_tracker = resource_tracker.clone();
            // config_for_heartbeat already cloned above
            
            tokio::spawn(async move {
                heartbeat_loop(
                    write_half,
                    node_id,
                    test_start,
                    stop_flag,
                    shared_snapshots,  // Pass shared snapshots
                    resource_tracker,  // Pass resource tracker
                    config_for_heartbeat,
                ).await
            })
        };
        
        // Wait for STOP message or test completion
        loop {
            tokio::select! {
                // Check for STOP message
                msg_result = async {
                    let mut read = read_half.lock().await;
                    read_message_from_read_half(&mut *read).await
                } => {
                    match msg_result {
                        Ok(Message::Stop) => {
                            println!("Received STOP message");
                            stop_flag.store(true, Ordering::Relaxed);
                            break;
                        }
                        Ok(Message::HeartbeatAck) => {
                            // Ignore ACKs in main loop (handled by heartbeat task)
                        }
                        Ok(other) => {
                            println!("Unexpected message: {:?}", other);
                        }
                        Err(e) => {
                            eprintln!("Error reading message: {}", e);
                            stop_flag.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
                
                // Check if workers completed
                _ = sleep(Duration::from_millis(100)) => {
                    // Check if worker thread finished
                    if worker_handle.is_finished() {
                        println!("Workers completed");
                        stop_flag.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }
        }
        
        // Wait for workers to finish
        println!("Waiting for workers to complete in-flight operations...");
        worker_handle.join()
            .map_err(|_| anyhow::anyhow!("Worker thread panicked"))??;
        
        // Stop heartbeat task
        heartbeat_handle.abort();
        
        let test_duration = test_start.elapsed();
        println!("Test duration: {:.2}s", test_duration.as_secs_f64());
        
        // Collect final statistics
        let stats_vec = worker_stats.lock().unwrap();
        
        // Get file_size and block_size from config for coverage calculation
        let file_size = config_for_results.targets.first().and_then(|t| t.file_size);
        let block_size = config_for_results.workload.block_size;
        
        // Create comprehensive per-worker snapshots
        let per_worker_snapshots: Vec<WorkerStatsSnapshot> = stats_vec.iter()
            .map(|s| WorkerStatsSnapshot::from_worker_stats(s, file_size, block_size))
            .collect::<Result<Vec<_>>>()
            .context("Failed to create worker snapshots")?;
        
        // Aggregate statistics by merging all worker stats
        let aggregate = if !stats_vec.is_empty() {
            // Start with first worker's stats
            let mut merged_stats = WorkerStats::with_heatmap(
                config_for_results.targets.iter().any(|t| t.lock_mode != crate::config::workload::FileLockMode::None),
                config_for_results.workload.heatmap,
            );
            
            // Merge all workers
            for worker_stats in stats_vec.iter() {
                merged_stats.merge(worker_stats)?;
            }
            
            // Create snapshot from merged stats
            WorkerStatsSnapshot::from_worker_stats(&merged_stats, file_size, block_size)
                .context("Failed to create aggregate snapshot")?
        } else {
            // No workers - create empty snapshot
            WorkerStatsSnapshot::from_worker_stats(&WorkerStats::new(), file_size, block_size)
                .context("Failed to create empty snapshot")?
        };
        
        // Send RESULTS message
        let results = ResultsMessage {
            node_id: self.node_id.clone(),
            duration_ns: test_duration.as_nanos() as u64,
            per_worker_stats: per_worker_snapshots,
            aggregate_stats: aggregate,
        };
        
        let mut write = write_half.lock().await;
        write_message_to_write_half(&mut *write, &Message::Results(results)).await?;
        println!("Sent RESULTS message");
        
        // Give coordinator time to read the message before closing connection
        // This is especially important for large messages (many workers with histograms)
        // 500ms should be enough even for 128 workers with full statistics
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        Ok(())
    }
}

/// Spawn worker threads and run the test
fn spawn_workers(
    config: Arc<crate::config::Config>,
    file_list: Option<Arc<Vec<std::path::PathBuf>>>,
    file_range: Option<(usize, usize)>,
    worker_id_start: usize,
    worker_id_end: usize,
    stop_flag: Arc<AtomicBool>,
    worker_stats: Arc<Mutex<Vec<crate::stats::WorkerStats>>>,
    shared_snapshots: Arc<Mutex<Vec<crate::worker::StatsSnapshot>>>,  // Add this parameter
) -> Result<()> {
    use crate::worker::Worker;
    
    let num_workers = config.workers.threads;
    let mut handles = Vec::new();
    
    // Check if per-worker distribution is enabled
    let is_per_worker = config.targets.iter()
        .any(|t| t.distribution == crate::config::workload::FileDistribution::PerWorker);
    
    // Check if partitioned distribution is enabled
    let is_partitioned = config.targets.iter()
        .any(|t| t.distribution == crate::config::workload::FileDistribution::Partitioned);
    
    // Determine if we need offset partitioning (single file + partitioned mode)
    let needs_offset_partitioning = is_partitioned && file_list.is_none() && !config.targets.is_empty();
    
    // Calculate offset ranges for partitioned single-file mode
    // IMPORTANT: In distributed mode, we need to calculate based on GLOBAL worker IDs
    // to ensure workers across nodes get non-overlapping regions
    let offset_ranges: Option<Vec<(u64, u64)>> = if needs_offset_partitioning {
        if let Some(file_size) = config.targets[0].file_size {
            // In distributed mode, we need to know the total number of workers across ALL nodes
            // The coordinator doesn't send this, so we need to infer it from worker_id_end
            // For now, we'll calculate based on the global worker IDs we received
            
            // Calculate region size based on the HIGHEST worker ID we know about
            // This is a limitation: we don't know the true total, so we use worker_id_end as a proxy
            // Better solution: coordinator should send total_workers_global
            let estimated_total_workers = worker_id_end;  // This is the highest worker ID + 1
            let region_size = file_size / estimated_total_workers as u64;
            
            let ranges: Vec<(u64, u64)> = (0..num_workers)
                .map(|local_worker_id| {
                    let global_worker_id = worker_id_start + local_worker_id;
                    let start = global_worker_id as u64 * region_size;
                    let end = if global_worker_id == estimated_total_workers - 1 {
                        file_size  // Last worker globally gets remainder
                    } else {
                        start + region_size
                    };
                    (start, end)
                })
                .collect();
            Some(ranges)
        } else {
            None
        }
    } else {
        None
    };
    
    // Spawn worker threads
    for local_worker_id in 0..num_workers {
        let global_worker_id = worker_id_start + local_worker_id;
        let mut worker_config = (*config).clone();
        let stop_flag = stop_flag.clone();
        let shared_snapshots = shared_snapshots.clone();  // Clone for this worker
        
        // Set offset range for this worker if partitioned single-file mode
        if let Some(ref ranges) = offset_ranges {
            worker_config.workers.offset_range = Some(ranges[local_worker_id]);
        }
        
        let worker_config = Arc::new(worker_config);
        
        // Filter file list for per-worker mode
        let worker_file_list = if is_per_worker {
            file_list.as_ref().map(|fl| {
                let worker_suffix = format!(".worker{}", global_worker_id);
                let filtered: Vec<std::path::PathBuf> = fl.iter()
                    .filter(|path| path.to_string_lossy().ends_with(&worker_suffix))
                    .cloned()
                    .collect();
                Arc::new(filtered)
            })
        } else {
            file_list.clone()
        };
        
        let handle = std::thread::spawn(move || {
            // Create worker with GLOBAL worker ID for proper identification
            let mut worker = Worker::new(global_worker_id, worker_config)
                .expect("Failed to create worker");
            
            // Set shared stats so worker updates during execution
            worker.set_shared_stats(shared_snapshots);
            
            // Set file list if provided
            if let Some(fl) = worker_file_list {
                worker.set_file_list(fl);
                
                // Set file range if provided (for PARTITIONED mode with file lists)
                // Note: file_range is not used in per-worker mode
                if let Some((start, end)) = file_range {
                    worker.set_file_range(start, end);
                }
            }
            
            // Run worker until stop flag is set
            worker.run_until_stopped(&stop_flag)
                .expect("Worker failed");
            
            // Return worker stats
            worker.into_stats()
        });
        
        handles.push(handle);
    }
    
    // Wait for all workers to complete
    let mut stats_vec = Vec::new();
    for handle in handles {
        let stats = handle.join()
            .map_err(|_| anyhow::anyhow!("Worker thread panicked"))?;
        stats_vec.push(stats);
    }
    
    // Store statistics
    *worker_stats.lock().unwrap() = stats_vec;
    
    Ok(())
}

/// Heartbeat loop
///
/// Sends periodic heartbeats to coordinator and implements dead man's switch.
async fn heartbeat_loop(
    write_half: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    node_id: String,
    test_start: std::time::Instant,
    stop_flag: Arc<AtomicBool>,
    shared_snapshots: Arc<Mutex<Vec<crate::worker::StatsSnapshot>>>,  // Vec of snapshots
    resource_tracker: Arc<Mutex<crate::util::resource::ResourceTracker>>,  // Resource tracker
    config: Arc<crate::config::Config>,  // Config for per-worker flag check
) -> Result<()> {
    use tokio::time::interval;
    
    let mut heartbeat_interval = interval(Duration::from_secs(1));
    
    loop {
        // Check if test stopped
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        
        // Wait for next heartbeat interval
        heartbeat_interval.tick().await;
        
        // Sample resource utilization
        {
            let mut tracker = resource_tracker.lock().unwrap();
            tracker.sample();
        }
        
        // Collect current statistics from shared snapshots
        let elapsed_ns = test_start.elapsed().as_nanos() as u64;
        
        // Aggregate current snapshots (cumulative values)
        let aggregate = {
            let snapshots = shared_snapshots.lock().unwrap();
            
            // Aggregate snapshots directly (like standalone monitoring thread does)
            let mut total_read_ops = 0u64;
            let mut total_write_ops = 0u64;
            let mut total_read_bytes = 0u64;
            let mut total_write_bytes = 0u64;
            let mut total_errors = 0u64;
            
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
            
            // Merge histograms
            use crate::stats::simple_histogram::SimpleHistogram;
            let mut merged_io_latency = SimpleHistogram::new();
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
                
                merged_io_latency.merge(&snapshot.read_latency);
                merged_io_latency.merge(&snapshot.write_latency);
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
            
            // Serialize histograms
            let io_latency_bytes = bincode::serialize(&merged_io_latency).unwrap_or_default();
            let read_latency_bytes = bincode::serialize(&merged_read_latency).unwrap_or_default();
            let write_latency_bytes = bincode::serialize(&merged_write_latency).unwrap_or_default();
            let open_latency_bytes = bincode::serialize(&merged_open_latency).unwrap_or_default();
            let close_latency_bytes = bincode::serialize(&merged_close_latency).unwrap_or_default();
            let stat_latency_bytes = bincode::serialize(&merged_stat_latency).unwrap_or_default();
            let setattr_latency_bytes = bincode::serialize(&merged_setattr_latency).unwrap_or_default();
            let mkdir_latency_bytes = bincode::serialize(&merged_mkdir_latency).unwrap_or_default();
            let rmdir_latency_bytes = bincode::serialize(&merged_rmdir_latency).unwrap_or_default();
            let unlink_latency_bytes = bincode::serialize(&merged_unlink_latency).unwrap_or_default();
            let rename_latency_bytes = bincode::serialize(&merged_rename_latency).unwrap_or_default();
            let readdir_latency_bytes = bincode::serialize(&merged_readdir_latency).unwrap_or_default();
            let fsync_latency_bytes = bincode::serialize(&merged_fsync_latency).unwrap_or_default();
            
            // Debug: print cumulative values
            if elapsed_ns < 6_000_000_000 {
                eprintln!("DEBUG CUMULATIVE: total_read={}, total_write={}", 
                    total_read_ops, total_write_ops);
            }
            
            // Create WorkerStatsSnapshot with CUMULATIVE values (not deltas)
            // The coordinator will calculate deltas when building time-series
            let snapshot = WorkerStatsSnapshot {
                read_ops: total_read_ops,  // CUMULATIVE, not delta
                write_ops: total_write_ops,  // CUMULATIVE, not delta
                read_bytes: total_read_bytes,  // CUMULATIVE, not delta
                write_bytes: total_write_bytes,  // CUMULATIVE, not delta
                errors: total_errors,
                test_duration_ns: elapsed_ns,
                errors_read: 0,
                errors_write: 0,
                errors_metadata: 0,
                verify_ops: 0,
                verify_failures: 0,
                min_bytes_per_op: 0,
                max_bytes_per_op: 0,
                avg_queue_depth: 0.0,
                peak_queue_depth: 0,
                io_latency_histogram: io_latency_bytes,
                read_latency_histogram: read_latency_bytes,
                write_latency_histogram: write_latency_bytes,
                metadata_open_ops: total_metadata_open,  // CUMULATIVE
                metadata_close_ops: total_metadata_close,  // CUMULATIVE
                metadata_stat_ops: total_metadata_stat,  // CUMULATIVE
                metadata_setattr_ops: total_metadata_setattr,  // CUMULATIVE
                metadata_mkdir_ops: total_metadata_mkdir,  // CUMULATIVE
                metadata_rmdir_ops: total_metadata_rmdir,  // CUMULATIVE
                metadata_unlink_ops: total_metadata_unlink,  // CUMULATIVE
                metadata_rename_ops: total_metadata_rename,  // CUMULATIVE
                metadata_readdir_ops: total_metadata_readdir,  // CUMULATIVE
                metadata_fsync_ops: total_metadata_fsync,  // CUMULATIVE
                metadata_open_latency: open_latency_bytes,
                metadata_close_latency: close_latency_bytes,
                metadata_stat_latency: stat_latency_bytes,
                metadata_setattr_latency: setattr_latency_bytes,
                metadata_mkdir_latency: mkdir_latency_bytes,
                metadata_rmdir_latency: rmdir_latency_bytes,
                metadata_unlink_latency: unlink_latency_bytes,
                metadata_rename_latency: rename_latency_bytes,
                metadata_readdir_latency: readdir_latency_bytes,
                metadata_fsync_latency: fsync_latency_bytes,
                cpu_percent: {
                    let tracker = resource_tracker.lock().unwrap();
                    tracker.stats().map(|s| s.cpu_percent).unwrap_or(0.0)
                },
                memory_bytes: {
                    let tracker = resource_tracker.lock().unwrap();
                    tracker.stats().map(|s| s.memory_bytes).unwrap_or(0)
                },
                peak_memory_bytes: {
                    let tracker = resource_tracker.lock().unwrap();
                    tracker.stats().map(|s| s.peak_memory_bytes).unwrap_or(0)
                },
                unique_blocks: 0,
                total_blocks: 0,
                lock_latency_histogram: None,
            };
            
            snapshot
        };
        
        // Debug: print cumulative values before sending
        if elapsed_ns < 6_000_000_000 {  // First 6 seconds
            eprintln!("DEBUG HEARTBEAT: elapsed={}s, read_ops={} (cumulative), write_ops={} (cumulative)", 
                elapsed_ns as f64 / 1_000_000_000.0,
                aggregate.read_ops,
                aggregate.write_ops);
        }
        
        // Send HEARTBEAT with cumulative values
        // Include per-worker snapshots if --per-worker-output is enabled
        let per_worker_snapshots = if config.output.per_worker_output {
            let snapshots = shared_snapshots.lock().unwrap();
            
            Some(snapshots.iter()
                .map(|s| WorkerStatsSnapshot::from_stats_snapshot(s))
                .collect::<Result<Vec<_>>>()
                .unwrap_or_else(|_| Vec::new()))
        } else {
            None
        };
        
        let heartbeat = HeartbeatMessage {
            node_id: node_id.clone(),
            elapsed_ns,
            stats: aggregate,
            per_worker_stats: per_worker_snapshots,
        };
        
        let mut write = write_half.lock().await;
        if let Err(e) = write_message_to_write_half(&mut *write, &Message::Heartbeat(heartbeat)).await {
            eprintln!("Failed to send heartbeat: {}", e);
            break;
        }
        
        // Note: We don't wait for HEARTBEAT_ACK in this simplified version
        // The coordinator will handle ACKs, and we rely on the main loop to detect disconnection
    }
    
    Ok(())
}

/// Read message from split read half
async fn read_message_from_read_half(read_half: &mut tokio::net::tcp::OwnedReadHalf) -> Result<Message> {
    use tokio::io::AsyncReadExt;
    
    // Read length field (4 bytes)
    let mut len_buf = [0u8; 4];
    read_half.read_exact(&mut len_buf).await
        .context("Failed to read message length")?;
    
    let msg_len = u32::from_le_bytes(len_buf) as usize;
    
    // Sanity check
    if msg_len > 100 * 1024 * 1024 {
        anyhow::bail!("Message too large: {} bytes", msg_len);
    }
    
    // Read message body
    let mut msg_buf = vec![0u8; msg_len];
    read_half.read_exact(&mut msg_buf).await
        .context("Failed to read message body")?;
    
    // Deserialize
    let msg = rmp_serde::from_slice(&msg_buf)
        .context("Failed to deserialize message")?;
    
    Ok(msg)
}

/// Write message to split write half
async fn write_message_to_write_half(write_half: &mut tokio::net::tcp::OwnedWriteHalf, msg: &Message) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    
    // Serialize with length prefix
    let framed = serialize_message(msg)?;
    
    // Write to stream
    write_half.write_all(&framed).await
        .context("Failed to write message")?;
    
    // Flush to ensure message is sent immediately
    write_half.flush().await
        .context("Failed to flush stream")?;
    
    Ok(())
}

/// Get node identifier (IP address or hostname)
fn get_node_id() -> Result<String> {
    // Try to get hostname first
    if let Ok(hostname) = hostname::get() {
        if let Ok(hostname_str) = hostname.into_string() {
            return Ok(hostname_str);
        }
    }
    
    // Fall back to "unknown"
    Ok("unknown".to_string())
}

/// Pre-allocate a region of a file (distributed mode)
///
/// Each node pre-allocates its assigned region of the file in parallel.
fn preallocate_region(
    path: &std::path::Path,
    start_offset: u64,
    region_size: u64,
    fill: bool,
    _pattern: crate::config::workload::VerifyPattern,
) -> Result<(usize, usize)> {
    use crate::target::file::FileTarget;
    use crate::target::Target;
    use crate::target::OpenFlags;
    
    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    // Check if file already exists and is fully allocated
    let file_exists = path.exists();
    let full_file_size = start_offset + region_size;
    
    let needs_allocation = if file_exists {
        // File exists - check if it's already allocated
        if let Ok(metadata) = std::fs::metadata(path) {
            let logical_size = metadata.len();
            
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let physical_bytes = metadata.blocks() * 512;
                let is_sparse = physical_bytes < logical_size / 2;
                
                // Need allocation if file is too small or sparse
                logical_size < full_file_size || is_sparse
            }
            #[cfg(not(unix))]
            {
                logical_size < full_file_size
            }
        } else {
            true // Can't stat, assume needs allocation
        }
    } else {
        true // File doesn't exist, needs creation and allocation
    };
    
    if needs_allocation {
        // Create FileTarget with full file size for proper allocation
        let mut target = FileTarget::new(path.to_path_buf(), Some(full_file_size));
        target.set_preallocate(true);
        target.set_offset_range(start_offset, full_file_size);
        
        let flags = OpenFlags {
            direct: false,
            sync: false,
            create: true,
            truncate: false,
        };
        
        target.open(flags)?;
        
        println!("  File opened successfully");
        
        // Don't fill here - FileTarget handles it based on offset_range
        // The fill is happening automatically in FileTarget::open() or refill logic
        
        println!("  Closing file...");
        target.close()?;
        
        println!("  File closed successfully");
        
        Ok((1, if fill { 1 } else { 0 }))
    } else {
        // File already exists and is fully allocated, nothing to do
        println!("  File already allocated, skipping");
        Ok((1, 0))
    }
}

/// Create files in parallel (distributed mode)
///
/// Creates empty files without filling them.
/// Used for write-only workloads where files will be filled during the test.
fn create_files_distributed(
    file_list: &[std::path::PathBuf],
    file_size: u64,
) -> Result<usize> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    
    let created_count = AtomicUsize::new(0);
    let processed_count = AtomicUsize::new(0);
    let total_files = file_list.len();
    
    // Create files in parallel
    file_list.par_iter().try_for_each(|path| -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        // Create file with specified size
        let file = std::fs::File::create(path)?;
        file.set_len(file_size)?;
        
        created_count.fetch_add(1, Ordering::Relaxed);
        
        // Update progress
        let processed = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
        if processed % 1000 == 0 || processed == total_files {
            println!("  Progress: {}/{} files created...", processed, total_files);
        }
        
        Ok(())
    })?;
    
    Ok(created_count.load(Ordering::Relaxed))
}

/// Validate and fill files in parallel (distributed mode)
///
/// Same logic as standalone P0 fix, but for distributed nodes.
fn validate_and_fill_files_distributed(
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
                
                // If allocated < 10% of logical, it's sparse
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
            // Create parent directory if needed
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            
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
