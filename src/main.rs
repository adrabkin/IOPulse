//! IOPulse CLI entry point

use anyhow::{Context, Result};
use iopulse::config::{cli::Cli, cli_convert, Config, WorkloadConfig, TargetConfig, TargetType, WorkerConfig, OutputConfig, RuntimeConfig, LayoutConfig, NamingPattern};
use iopulse::config::workload::*;
// Note: LocalCoordinator removed - all modes use distributed architecture
use iopulse::stats::WorkerStats;
use std::sync::Arc;

fn main() -> Result<()> {
    use std::time::Instant;
    
    let main_start = Instant::now();
    
    println!("IOPulse v{}", env!("CARGO_PKG_VERSION"));
    println!("High-performance IO profiling tool");
    println!();
    
    // Parse CLI arguments
    let parse_start = Instant::now();
    let cli = Cli::parse_args();
    cli.validate()?;
    let parse_elapsed = parse_start.elapsed();
    if cli.debug {
        eprintln!("DEBUG TIMING: CLI parse: {:.3}s", parse_elapsed.as_secs_f64());
    }
    
    // Handle different execution modes
    match cli.mode {
        iopulse::config::cli::ExecutionMode::Standalone => {
            run_standalone(cli, main_start)
        }
        iopulse::config::cli::ExecutionMode::Service => {
            run_service(cli)
        }
        iopulse::config::cli::ExecutionMode::Coordinator => {
            run_coordinator(cli)
        }
    }
}

/// Run in standalone mode (single machine)
fn run_standalone(cli: Cli, _main_start: std::time::Instant) -> Result<()> {
    use std::time::Instant;
    
    // Build configuration from CLI
    let config_start = Instant::now();
    let config = build_config_from_cli(&cli)?;
    let config_elapsed = config_start.elapsed();
    if cli.debug {
        eprintln!("DEBUG TIMING: Config build: {:.3}s", config_elapsed.as_secs_f64());
    }
    
    // Validate configuration (includes write conflict detection)
    iopulse::config::validator::validate_config(&config)
        .context("Configuration validation failed")?;
    
    // Display configuration
    let print_start = Instant::now();
    print_configuration(&config);
    let print_elapsed = print_start.elapsed();
    if cli.debug {
        eprintln!("DEBUG TIMING: Print config: {:.3}s", print_elapsed.as_secs_f64());
    }
    
    if cli.dry_run {
        println!();
        println!("Dry run mode - configuration validated successfully");
        return Ok(());
    }

    println!();
    println!("Starting test...");
    println!();
    
    // Use distributed architecture with localhost service (unified path for all modes)
    if cli.debug {
        eprintln!("DEBUG: Using unified architecture (localhost service)");
    }
    
    // Find available port
    let service_port = find_available_port(cli.debug)?;
    if cli.debug {
        eprintln!("DEBUG: Found available port: {}", service_port);
    }
    
    // Auto-launch service on localhost
    let service_handle = launch_localhost_service(service_port, &cli)?;
    if cli.debug {
        eprintln!("DEBUG: Service launched (PID: {})", service_handle.id());
    }
    
    // Wait for service to be ready
    std::thread::sleep(std::time::Duration::from_millis(500));
    
    // Use DistributedCoordinator with localhost
    let node_addresses = vec![format!("localhost:{}", service_port)];
    
    let runtime = tokio::runtime::Runtime::new()
        .context("Failed to create tokio runtime")?;
    
    let result = runtime.block_on(async {
        let coordinator = iopulse::distributed::DistributedCoordinator::new(
            Arc::new(config),
            node_addresses,
        ).context("Failed to create coordinator")?;
        
        coordinator.run().await
    });
    
    // Cleanup service
    if let Err(e) = cleanup_service(service_handle, cli.debug) {
        eprintln!("Warning: Failed to cleanup service: {}", e);
    }
    
    result
}

/// Build configuration from CLI arguments
fn build_config_from_cli(cli: &Cli) -> Result<Config> {
    // Parse block size (for future use with IO patterns)
    let block_size = cli_convert::parse_size(&cli.block_size)
        .context("Invalid block size")?;
    
    // Determine read/write percentages
    let (read_percent, write_percent) = match (cli.read_percent, cli.write_percent) {
        (Some(r), Some(w)) => (r, w),
        (Some(r), None) => (r, 100 - r),
        (None, Some(w)) => (100 - w, w),
        (None, None) => (100, 0), // Default to 100% read
    };
    
    // Parse completion mode
    let completion_mode = if let Some(ref duration_str) = cli.duration {
        let seconds = cli_convert::parse_duration(duration_str)
            .context("Invalid duration")?;
        if seconds == 0 {
            // Duration 0 means "run until file is complete"
            CompletionMode::RunUntilComplete
        } else {
            CompletionMode::Duration { seconds }
        }
    } else if let Some(ref bytes_str) = cli.total_bytes {
        let bytes = cli_convert::parse_size(bytes_str)
            .context("Invalid total bytes")?;
        CompletionMode::TotalBytes { bytes }
    } else if cli.run_until_complete {
        CompletionMode::RunUntilComplete
    } else {
        CompletionMode::Duration { seconds: 10 } // Default
    };
    
    // Convert distribution
    let distribution = cli_convert::convert_distribution_type(
        cli.distribution,
        cli.zipf_theta,
        cli.pareto_h,
        cli.gaussian_stddev,
        cli.gaussian_center,
    )?;
    
    // Parse think time if specified
    let think_time = if let Some(ref think_str) = cli.think_time {
        let duration_us = cli_convert::parse_time_us(think_str)
            .context("Invalid think time")?;
        Some(ThinkTimeConfig {
            duration_us,
            mode: cli_convert::convert_think_mode(cli.think_mode),
            apply_every_n_blocks: cli.think_every,
            adaptive_percent: cli.think_adaptive_percent,
        })
    } else if cli.think_adaptive_percent.is_some() {
        // Adaptive-only mode (no base duration, purely adaptive)
        Some(ThinkTimeConfig {
            duration_us: 0,  // No base duration
            mode: cli_convert::convert_think_mode(cli.think_mode),
            apply_every_n_blocks: cli.think_every,
            adaptive_percent: cli.think_adaptive_percent,
        })
    } else {
        None
    };
    
    // Build workload configuration
    let workload = WorkloadConfig {
        read_percent,
        write_percent,
        read_distribution: vec![],
        write_distribution: vec![],
        block_size,  // Pass parsed block size
        queue_depth: cli.queue_depth,
        completion_mode,
        random: cli.random,  // Pass random flag
        distribution,
        think_time,
        engine: cli_convert::convert_engine_type(cli.engine),
        direct: cli.direct,
        sync: cli.sync,
        heatmap: cli.heatmap,
        heatmap_buckets: cli.heatmap_buckets,
        write_pattern: cli_convert::convert_verify_pattern(cli.write_pattern),
    };
    
    // Parse file size if specified
    let file_size = if let Some(ref size_str) = cli.file_size {
        Some(cli_convert::parse_size(size_str).context("Invalid file size")?)
    } else {
        None
    };
    
    // Parse fadvise flags
    let fadvise_flags = if let Some(ref fadvise_str) = cli.fadvise {
        parse_fadvise_flags(fadvise_str)?
    } else {
        FadviseFlags::default()
    };
    
    // Build target configuration
    let target_path = cli.target.clone()
        .ok_or_else(|| anyhow::anyhow!("Target path required in standalone mode"))?;
    
    let mut target = TargetConfig {
        path: target_path,
        target_type: TargetType::File, // TODO: Detect block devices
        file_size,
        num_files: cli.num_files,
        num_dirs: cli.num_dirs,
        layout_config: None,  // Will be built below if layout parameters provided
        layout_manifest: cli.layout_manifest.clone(),
        export_layout_manifest: cli.export_layout_manifest.clone(),
        distribution: cli_convert::convert_file_distribution(cli.file_distribution),
        fadvise_flags,
        madvise_flags: MadviseFlags::default(),
        lock_mode: cli_convert::convert_lock_mode(cli.lock_mode),
        preallocate: cli.preallocate,  // Default: false
        truncate_to_size: cli.truncate_to_size,
        refill: cli.refill,
        refill_pattern: cli_convert::convert_verify_pattern(cli.refill_pattern),
        no_refill: cli.no_refill,
    };
    
    // Build layout_config if layout parameters are provided
    // Note: layout_manifest takes precedence and will override this
    if let (Some(depth), Some(width)) = (cli.dir_depth, cli.dir_width) {
        // Validate that file_size is provided for layout generation
        // UNLESS layout_manifest is provided (manifest has file sizes)
        if target.file_size.is_none() && cli.layout_manifest.is_none() {
            anyhow::bail!("--file-size is required when generating layouts (--dir-depth, --dir-width, --total-files)");
        }
        
        // Calculate files_per_dir from total_files if provided
        let files_per_dir = if let Some(total_files) = cli.total_files {
            // Calculate total directories in layout
            // Note: The layout generator creates files at:
            // - All leaf directories (at max depth)
            // - All intermediate directories (depth > 0 and depth < max_depth)
            // - NOT at root (depth == 0)
            
            let mut total_dirs_with_files = 0;
            
            // Leaf directories (at max depth)
            total_dirs_with_files += width.pow(depth as u32);
            
            // Intermediate directories (if depth > 1)
            if depth > 1 {
                for level in 1..depth {
                    total_dirs_with_files += width.pow(level as u32);
                }
            }
            
            // Calculate base files_per_dir (floor division)
            // We'll handle the remainder in the layout generator
            total_files / total_dirs_with_files
        } else {
            // Default to 1 file per directory if not specified
            1
        };
        
        target.layout_config = Some(LayoutConfig {
            depth,
            width,
            files_per_dir,
            naming_pattern: NamingPattern::Sequential,
            num_workers: None,  // Will be set by coordinator if per-worker mode
            total_files: cli.total_files,  // Pass through for exact file count
        });
    } else if cli.num_files.is_some() || cli.num_dirs.is_some() {
        // Simple case: --num-files and/or --num-dirs without full tree parameters
        // Create a flat structure with the specified number of directories and files
        
        // Validate that file_size is provided
        if target.file_size.is_none() {
            anyhow::bail!("--file-size is required when using --num-files or --num-dirs");
        }
        
        let num_dirs = cli.num_dirs.unwrap_or(1);
        let num_files = cli.num_files.unwrap_or(1);
        
        // Calculate files per directory
        let files_per_dir = (num_files + num_dirs - 1) / num_dirs;
        
        // Create a simple layout: depth=1 (flat), width=num_dirs
        // With the updated LayoutGenerator, depth=1 means files only in subdirectories
        target.layout_config = Some(LayoutConfig {
            depth: 1,
            width: num_dirs,
            files_per_dir,
            naming_pattern: NamingPattern::Sequential,
            num_workers: None,  // Will be set by coordinator if per-worker mode
            total_files: Some(num_files),  // Exact file count for simple layout
        });
    }
    
    // If layout_manifest is provided, load it and use file_size from manifest
    if let Some(ref manifest_path) = target.layout_manifest {
        use iopulse::target::LayoutManifest;
        let manifest = LayoutManifest::from_file(manifest_path)
            .context("Failed to load layout manifest for file_size")?;
        
        if manifest.header.file_size > 0 {
            // Use file_size from manifest if not specified in CLI
            if target.file_size.is_none() {
                target.file_size = Some(manifest.header.file_size);
            } else if target.file_size != Some(manifest.header.file_size) {
                // Warn if CLI file_size differs from manifest
                eprintln!("⚠️  Warning: CLI --file-size ({}) differs from manifest file_size ({})", 
                    target.file_size.unwrap(), manifest.header.file_size);
                eprintln!("           Using CLI file_size. Remove --file-size to use manifest value.");
            }
        }
    }
    
    // Build worker configuration
    let workers = WorkerConfig {
        threads: cli.threads,
        cpu_cores: cli.cpu_cores.clone(),
        numa_zones: cli.numa_zones.clone(),
        rate_limit_iops: None,
        rate_limit_throughput: None,
        offset_range: None,  // Set by coordinator for partitioned distribution
    };
    
    // Parse live interval if specified
    let live_interval = if let Some(ref interval_str) = cli.live_interval {
        Some(cli_convert::parse_duration(interval_str).context("Invalid live interval")?)
    } else {
        None
    };
    
    // Helper function to parse duration string to seconds
    let parse_duration_to_secs = |s: Option<&str>| -> Option<u64> {
        s.and_then(|interval_str| cli_convert::parse_duration(interval_str).ok())
    };
    
    // Build output configuration
    let output = OutputConfig {
        json_output: cli.json_output.clone(),
        json_name: cli.json_name.clone(),
        json_histogram: cli.json_histogram,
        per_worker_output: cli.per_worker_output,
        no_aggregate: cli.no_aggregate,
        json_interval: parse_duration_to_secs(cli.json_interval.as_deref()),
        csv_output: cli.csv_output.clone(),
        prometheus: cli.prometheus,
        prometheus_port: cli.prometheus_port,
        show_latency: cli.show_latency,
        show_histogram: cli.show_histogram,
        show_percentiles: cli.show_percentiles,
        live_interval,
        no_live: cli.no_live,
        verbosity: 0,
    };
    
    // Build runtime configuration
    let runtime = RuntimeConfig {
        continue_on_error: cli.continue_on_error,
        max_errors: cli.max_errors,
        continue_on_worker_failure: false,
        verify: cli.verify,
        verify_pattern: cli.verify_pattern.map(cli_convert::convert_verify_pattern),
        dry_run: cli.dry_run,
        debug: cli.debug,
        allow_write_conflicts: cli.allow_write_conflicts,
    };
    
    Ok(Config {
        workload,
        targets: vec![target],
        workers,
        output,
        runtime,
    })
}

/// Parse fadvise flags from comma-separated string
fn parse_fadvise_flags(s: &str) -> Result<FadviseFlags> {
    let mut flags = FadviseFlags::default();
    
    for flag in s.split(',') {
        match flag.trim().to_lowercase().as_str() {
            "seq" | "sequential" => flags.sequential = true,
            "rand" | "random" => flags.random = true,
            "willneed" => flags.willneed = true,
            "dontneed" => flags.dontneed = true,
            "noreuse" => flags.noreuse = true,
            "" => {}
            other => anyhow::bail!("Unknown fadvise flag: {}", other),
        }
    }
    
    Ok(flags)
}

/// Print configuration summary
fn print_configuration(config: &Config) {
    println!("Configuration:");
    println!("  Workload:");
    println!("    Read: {}%, Write: {}%", config.workload.read_percent, config.workload.write_percent);
    println!("    Queue depth: {}", config.workload.queue_depth);
    println!("    Engine: {}", config.workload.engine);
    println!("    Distribution: {}", config.workload.distribution);
    println!("    Completion: {}", config.workload.completion_mode);
    
    if let Some(ref think_time) = config.workload.think_time {
        println!("    Think time: {}", think_time);
    }
    
    // Show lock mode if not None
    if config.targets.get(0).map(|t| t.lock_mode) != Some(FileLockMode::None) {
        if let Some(lock_mode) = config.targets.get(0).map(|t| t.lock_mode) {
            println!("    Lock mode: {:?}", lock_mode);
        }
    }
    
    println!("  Targets:");
    for target in &config.targets {
        println!("    Path: {}", target.path.display());
        println!("    Type: {:?}", target.target_type);
        if let Some(size) = target.file_size {
            println!("    Size: {} bytes", size);
        }
    }
    
    println!("  Workers:");
    println!("    Threads: {}", config.workers.threads);
    if let Some(ref cores) = config.workers.cpu_cores {
        println!("    CPU cores: {}", cores);
    }
    if let Some(ref zones) = config.workers.numa_zones {
        println!("    NUMA zones: {}", zones);
    }
}

/// Run in service mode (distributed node)
fn run_service(cli: Cli) -> Result<()> {
    // Service mode uses tokio runtime
    let runtime = tokio::runtime::Runtime::new()
        .context("Failed to create tokio runtime")?;
    
    runtime.block_on(async {
        let service = iopulse::distributed::NodeService::new(cli.listen_port)
            .context("Failed to create node service")?;
        
        service.run().await
    })
}

/// Run in coordinator mode (distributed orchestration)
fn run_coordinator(cli: Cli) -> Result<()> {
    // Parse node addresses
    let node_addresses = if let Some(ref host_list) = cli.host_list {
        // Parse comma-separated list
        host_list.split(',')
            .map(|s| {
                let addr = s.trim();
                // Add port if not specified
                if addr.contains(':') {
                    addr.to_string()
                } else {
                    format!("{}:{}", addr, cli.worker_port)
                }
            })
            .collect()
    } else if let Some(ref clients_file) = cli.clients_file {
        // Read from file
        let content = std::fs::read_to_string(clients_file)
            .context("Failed to read clients file")?;
        
        content.lines()
            .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
            .map(|line| {
                let addr = line.trim();
                if addr.contains(':') {
                    addr.to_string()
                } else {
                    format!("{}:{}", addr, cli.worker_port)
                }
            })
            .collect()
    } else {
        anyhow::bail!("Coordinator mode requires --host-list or --clients-file");
    };
    
    // Build configuration
    let config = build_config_from_cli(&cli)?;
    
    // Validate configuration (includes write conflict detection)
    iopulse::config::validator::validate_config(&config)
        .context("Configuration validation failed")?;
    
    // Coordinator mode uses tokio runtime
    let runtime = tokio::runtime::Runtime::new()
        .context("Failed to create tokio runtime")?;
    
    runtime.block_on(async {
        let coordinator = iopulse::distributed::DistributedCoordinator::new(
            Arc::new(config),
            node_addresses,
        ).context("Failed to create coordinator")?;
        
        coordinator.run().await
    })
}

/// Print test results
pub fn print_results(stats: &WorkerStats, duration: std::time::Duration, config: &Config) {
    use iopulse::util::time::{calculate_iops, calculate_throughput, format_rate, format_throughput};
    
    println!("═══════════════════════════════════════════════════════════");
    println!("                    TEST RESULTS");
    println!("═══════════════════════════════════════════════════════════");
    println!();
    
    // Print elapsed time
    println!("Elapsed Time: {:.3}s", duration.as_secs_f64());
    println!();
    
    // Calculate IOPS and throughput
    let read_iops = calculate_iops(stats.read_ops(), duration);
    let write_iops = calculate_iops(stats.write_ops(), duration);
    let total_iops = calculate_iops(stats.total_ops(), duration);
    
    let read_throughput = calculate_throughput(stats.read_bytes(), duration);
    let write_throughput = calculate_throughput(stats.write_bytes(), duration);
    let total_throughput = calculate_throughput(stats.total_bytes(), duration);
    
    // Operations with IOPS
    println!("Operations:");
    println!("  Read:  {} ops ({}) - {} IOPS", 
             format_number(stats.read_ops()), 
             format_bytes(stats.read_bytes()),
             format_rate(read_iops));
    println!("  Write: {} ops ({}) - {} IOPS", 
             format_number(stats.write_ops()), 
             format_bytes(stats.write_bytes()),
             format_rate(write_iops));
    println!("  Total: {} ops ({}) - {} IOPS", 
             format_number(stats.total_ops()), 
             format_bytes(stats.total_bytes()),
             format_rate(total_iops));
    
    if stats.errors() > 0 {
        println!("  Errors: {}", stats.errors());
    }
    
    // Verification statistics (only if verification enabled)
    if stats.verify_ops() > 0 {
        let success_rate = if stats.verify_ops() > 0 {
            ((stats.verify_ops() - stats.verify_failures()) as f64 / stats.verify_ops() as f64) * 100.0
        } else {
            0.0
        };
        println!();
        println!("Verification:");
        println!("  Operations: {}", format_number(stats.verify_ops()));
        println!("  Failures:   {}", format_number(stats.verify_failures()));
        println!("  Success:    {:.2}%", success_rate);
    }
    
    println!();
    
    // Coverage and rewrite statistics (only if heatmap enabled)
    if config.workload.heatmap {
        if let Some(file_size) = config.targets.get(0).and_then(|t| t.file_size) {
            let total_blocks = file_size / config.workload.block_size;
            let unique_blocks = stats.unique_blocks_count();
            let coverage = stats.coverage_percent(total_blocks);
            let rewrites = stats.rewrite_percent();
            
            println!("Coverage:");
            println!("  Unique blocks: {} / {} ({:.2}%)", 
                     format_number(unique_blocks),
                     format_number(total_blocks),
                     coverage);
            println!("  Rewrites:      {} ops ({:.2}% of operations)",
                     format_number(stats.total_ops() - unique_blocks),
                     rewrites);
            println!();
        }
    }
    
    println!();
    
    // Throughput
    println!("Throughput:");
    println!("  Read:  {}", format_throughput(read_throughput));
    println!("  Write: {}", format_throughput(write_throughput));
    println!("  Total: {}", format_throughput(total_throughput));
    
    println!();
    
    // Latency statistics
    println!("Latency:");
    let hist = stats.io_latency();
    
    if hist.len() > 0 {
        let min = hist.min();
        println!("  Min:    {:?}", min);
        
        let mean = hist.mean();
        println!("  Mean:   {:?}", mean);
        
        let max = hist.max();
        println!("  Max:    {:?}", max);
        
        println!();
        println!("  Percentiles:");
        for &p in &[50.0, 90.0, 95.0, 99.0, 99.9, 99.99] {
            let val = hist.percentile(p);
            println!("    p{:5.2}: {:?}", p, val);
        }
    } else {
        println!("  No latency data collected");
    }
    
    println!();
    
    // Metadata operations
    let metadata_ops = stats.metadata.total_ops();
    if metadata_ops > 0 {
        println!("Metadata Operations:");
        println!("  Open:   {}", stats.metadata.open_ops.get());
        println!("  Close:  {}", stats.metadata.close_ops.get());
        println!("  Fsync:  {}", stats.metadata.fsync_ops.get());
        println!("  Total:  {}", metadata_ops);
        println!();
    }
    
    // Lock latency statistics (if locking was enabled)
    if let Some(ref lock_hist) = stats.lock_latency() {
        if lock_hist.len() > 0 {
            println!("File Locking:");
            println!("  Locks acquired: {}", lock_hist.len());
            println!("  Min latency:    {:?}", lock_hist.min());
            println!("  Mean latency:   {:?}", lock_hist.mean());
            println!("  Max latency:    {:?}", lock_hist.max());
            println!();
        }
    }
    
    // Heatmap output (if enabled)
    if config.workload.heatmap {
        if let Some(file_size) = config.targets[0].file_size {
            let total_blocks = file_size / config.workload.block_size;
            if let Some(heatmap_output) = stats.heatmap_summary(config.workload.heatmap_buckets, total_blocks) {
                println!("{}", heatmap_output);
            }
        }
    }
    
    // Resource utilization (CPU and memory)
    if let Some(resource_stats) = stats.resource_stats() {
        println!("Resource Utilization:");
        
        // CPU utilization - show both process and system perspective
        let num_threads = config.workers.threads as f64;
        let process_cpu = resource_stats.cpu_percent;  // Total across all threads
        let avg_cpu_per_thread = process_cpu / num_threads;
        
        // Get system CPU count
        if let Some(system_cpus) = iopulse::util::resource::ResourceSnapshot::num_cpus() {
            let system_cpu_percent = process_cpu / system_cpus as f64;
            println!("  CPU:    {:.0}% per worker avg ({} workers)", 
                     avg_cpu_per_thread, config.workers.threads);
            println!("          {:.1}% of system capacity ({} cores total)", 
                     system_cpu_percent, system_cpus);
        } else {
            println!("  CPU:    {:.1}% avg per thread ({} threads)", 
                     avg_cpu_per_thread, config.workers.threads);
        }
        
        // Memory utilization
        println!("  Memory: {} (peak: {})", 
                 format_bytes(resource_stats.memory_bytes),
                 format_bytes(resource_stats.peak_memory_bytes));
        println!();
    }
    
    println!("═══════════════════════════════════════════════════════════");
}

/// Format a number with thousands separators
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let mut count = 0;
    
    for c in s.chars().rev() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
        count += 1;
    }
    
    result.chars().rev().collect()
}

/// Format bytes with appropriate units
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    
    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Find an available port for the localhost service
fn find_available_port(debug: bool) -> Result<u16> {
    use std::net::TcpListener;
    
    // Try ports 9999-10099
    for port in 9999..10100 {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            drop(listener);
            if debug {
                eprintln!("DEBUG: Port {} is available", port);
            }
            return Ok(port);
        }
    }
    
    anyhow::bail!("No available ports found in range 9999-10099. Please close other IOPulse instances or specify --no-service.")
}

/// Launch localhost service in background
fn launch_localhost_service(port: u16, cli: &Cli) -> Result<std::process::Child> {
    use std::process::{Command, Stdio};
    
    // Get current executable path
    let exe_path = std::env::current_exe()
        .context("Failed to get current executable path")?;
    
    // Launch service mode
    let mut cmd = Command::new(&exe_path);
    cmd.arg("--mode").arg("service");
    cmd.arg("--listen-port").arg(port.to_string());
    
    // Pass debug flag if set
    if cli.debug {
        cmd.arg("--debug");
    }
    
    // Redirect output to /dev/null (or log file if debug)
    if cli.debug {
        let log_path = format!("/tmp/iopulse_service_{}.log", port);
        let log_file = std::fs::File::create(&log_path)
            .context("Failed to create service log file")?;
        cmd.stdout(Stdio::from(log_file.try_clone()?));
        cmd.stderr(Stdio::from(log_file));
        eprintln!("DEBUG: Service log: {}", log_path);
    } else {
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
    }
    
    let child = cmd.spawn()
        .context("Failed to spawn service process")?;
    
    if cli.debug {
        eprintln!("DEBUG: Service launched on port {} (PID: {})", port, child.id());
    }
    
    Ok(child)
}

/// Cleanup service process
fn cleanup_service(mut child: std::process::Child, debug: bool) -> Result<()> {
    use std::time::Duration;
    
    if debug {
        eprintln!("DEBUG: Cleaning up service (PID: {})...", child.id());
    }
    
    // Try graceful shutdown first (service should exit when coordinator disconnects)
    match child.try_wait()? {
        Some(status) => {
            if debug {
                eprintln!("DEBUG: Service already exited with status: {}", status);
            }
            return Ok(());
        }
        None => {
            // Still running, wait for service to finish sending results and exit
            // Service has a 500ms delay after sending RESULTS, so wait at least 1 second
            std::thread::sleep(Duration::from_millis(1000));
            
            match child.try_wait()? {
                Some(status) => {
                    if debug {
                        eprintln!("DEBUG: Service exited gracefully with status: {}", status);
                    }
                    return Ok(());
                }
                None => {
                    // Force kill
                    if debug {
                        eprintln!("DEBUG: Service still running, force killing...");
                    }
                    child.kill()?;
                    let status = child.wait()?;
                    if debug {
                        eprintln!("DEBUG: Service killed with status: {}", status);
                    }
                }
            }
        }
    }
    
    Ok(())
}
