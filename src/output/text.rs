//! Human-readable text output

use crate::stats::WorkerStats;
use crate::config::Config;
use crate::util::time::{calculate_iops, calculate_throughput, format_rate, format_throughput};

/// Print test results to console
///
/// Displays comprehensive statistics including:
/// - Operations and IOPS
/// - Throughput
/// - Latency percentiles
/// - Metadata operations
/// - Resource utilization
/// - Coverage (if heatmap enabled)
/// - Heatmap visualization (if enabled)
pub fn print_results(stats: &WorkerStats, duration: std::time::Duration, config: &Config) {
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
        if let Some(system_cpus) = crate::util::resource::ResourceSnapshot::num_cpus() {
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
