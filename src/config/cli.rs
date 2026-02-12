//! CLI argument parsing using clap

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// Execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExecutionMode {
    /// Standalone mode (default) - single machine testing
    Standalone,
    /// Coordinator mode - orchestrate distributed testing
    Coordinator,
    /// Service mode - run service on node (accepts coordinator commands)
    Service,
}

/// IOPulse - High-performance IO profiling tool
#[derive(Parser, Debug)]
#[command(name = "iopulse")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Execution mode: standalone, coordinator, or service
    #[arg(long, value_enum, default_value = "standalone")]
    pub mode: ExecutionMode,
    
    /// Port for service to listen on (service mode only)
    #[arg(long, default_value = "9999")]
    pub listen_port: u16,
    
    /// Comma-separated list of node addresses for coordinator mode (e.g., "10.0.1.10:9999,10.0.1.11:9999")
    #[arg(long)]
    pub host_list: Option<String>,
    
    /// File containing list of node addresses (one per line, for coordinator mode)
    #[arg(long)]
    pub clients_file: Option<PathBuf>,
    
    /// Port to connect to on worker nodes (coordinator mode only)
    #[arg(long, default_value = "9999")]
    pub worker_port: u16,
    
    /// Target path (file, directory, or block device)
    /// 
    /// Not required in service mode (coordinator sends configuration)
    #[arg(value_name = "PATH")]
    pub target: Option<PathBuf>,

    // === Basic Options ===
    /// Number of worker threads
    #[arg(short = 't', long, default_value = "1")]
    pub threads: usize,

    /// Block size for IO operations (e.g., 4k, 1M, 64k)
    #[arg(short = 'b', long, default_value = "4k")]
    pub block_size: String,

    /// File size for created files (e.g., 1G, 100M)
    #[arg(short = 's', long)]
    pub file_size: Option<String>,

    /// Test duration (e.g., 60s, 5m, 1h)
    #[arg(short = 'd', long)]
    pub duration: Option<String>,

    /// Total bytes to transfer (e.g., 10G, 1T)
    #[arg(long)]
    pub total_bytes: Option<String>,

    /// Run until all operations complete (no time/byte limit)
    #[arg(long)]
    pub run_until_complete: bool,

    // === Workload Options ===
    /// Use random offsets instead of sequential
    #[arg(long)]
    pub random: bool,

    /// Read percentage for mixed workloads (0-100)
    #[arg(long)]
    pub read_percent: Option<u8>,

    /// Write percentage for mixed workloads (0-100)
    #[arg(long)]
    pub write_percent: Option<u8>,

    /// IO queue depth (1-1024)
    #[arg(short = 'q', long, default_value = "1")]
    pub queue_depth: usize,
    
    /// Pattern to use for write buffer data (default: random for realistic benchmarking)
    #[arg(long, value_enum, default_value = "random")]
    pub write_pattern: VerifyPattern,

    // === Distribution Options ===
    /// Random distribution type
    #[arg(long, value_enum, default_value = "uniform")]
    pub distribution: DistributionType,

    /// Zipf theta parameter (0.0-3.0)
    #[arg(long, default_value = "1.2")]
    pub zipf_theta: f64,

    /// Pareto h parameter (0.0-10.0)
    #[arg(long, default_value = "0.9")]
    pub pareto_h: f64,

    /// Gaussian standard deviation
    #[arg(long)]
    pub gaussian_stddev: Option<f64>,

    /// Gaussian center point (0.0-1.0, fraction of file size)
    #[arg(long, default_value = "0.5")]
    pub gaussian_center: f64,

    // === Think Time Options ===
    /// Think time between IOs (e.g., 100us, 1ms, 10ms)
    #[arg(long)]
    pub think_time: Option<String>,

    /// Think time mode: sleep or spin
    #[arg(long, value_enum, default_value = "sleep")]
    pub think_mode: ThinkMode,

    /// Apply think time every N blocks
    #[arg(long, default_value = "1")]
    pub think_every: usize,

    /// Adaptive think time as percentage of IO latency
    #[arg(long)]
    pub think_adaptive_percent: Option<u8>,

    // === IO Engine Options ===
    /// IO engine to use
    #[arg(long, value_enum, default_value = "sync")]
    pub engine: EngineType,

    /// Use direct IO (O_DIRECT) - bypasses page cache for real storage testing
    /// Note: Requires aligned buffers and may require pre-existing files
    #[arg(long)]
    pub direct: bool,

    /// Use synchronous IO (O_SYNC)
    #[arg(long)]
    pub sync: bool,

    // === fadvise/madvise Options ===
    /// fadvise hints (comma-separated: seq,rand,willneed,dontneed,noreuse)
    #[arg(long)]
    pub fadvise: Option<String>,

    /// madvise hints (comma-separated: seq,rand,willneed,dontneed,hugepage,nohugepage)
    #[arg(long)]
    pub madvise: Option<String>,

    // === File Locking Options ===
    /// File locking mode
    #[arg(long, value_enum, default_value = "none")]
    pub lock_mode: LockMode,

    // === File Distribution Options ===
    /// File distribution strategy
    #[arg(long, value_enum, default_value = "shared")]
    pub file_distribution: FileDistributionType,

    /// Number of files per directory
    #[arg(short = 'n', long)]
    pub num_files: Option<usize>,

    /// Number of directories
    #[arg(short = 'N', long)]
    pub num_dirs: Option<usize>,
    
    // === Directory Tree Options ===
    /// Directory tree depth (number of nested levels)
    #[arg(long)]
    pub dir_depth: Option<usize>,
    
    /// Directory tree width (subdirectories per level)
    #[arg(long)]
    pub dir_width: Option<usize>,
    
    /// Total number of files to generate (distributed across tree)
    #[arg(long)]
    pub total_files: Option<usize>,
    
    /// Layout manifest file (input) - defines directory/file structure
    /// Overrides --dir-depth, --dir-width, --total-files if provided
    #[arg(long)]
    pub layout_manifest: Option<PathBuf>,
    
    /// Export layout manifest file (output) - save generated structure for reuse
    #[arg(long)]
    pub export_layout_manifest: Option<PathBuf>,

    // === Target Options ===
    /// Enable file space pre-allocation via posix_fallocate() (disabled by default)
    #[arg(long = "preallocate")]
    pub preallocate: bool,

    /// Truncate files to size on creation
    #[arg(long)]
    pub truncate_to_size: bool,
    
    /// Fill pre-allocated files with pattern data (enables read testing on pre-allocated files)
    #[arg(long)]
    pub refill: bool,
    
    /// Pattern to use for refill operation
    #[arg(long, value_enum, default_value = "random")]
    pub refill_pattern: VerifyPattern,
    
    /// Disable automatic file filling for read tests (advanced users only)
    /// By default, IOPulse automatically fills empty files when read operations are requested.
    /// Use this flag to disable auto-fill and get an error instead.
    #[arg(long)]
    pub no_refill: bool,

    // === Output Options ===
    /// JSON output file path or directory
    #[arg(long)]
    pub json_output: Option<PathBuf>,
    
    /// Name for aggregate JSON file (default: "aggregate")
    #[arg(long, default_value = "aggregate")]
    pub json_name: String,
    
    /// Generate separate histogram file with all 112 buckets
    #[arg(long)]
    pub json_histogram: bool,
    
    /// Include per-worker stats in time-series output (JSON and CSV)
    #[arg(long)]
    pub per_worker_output: bool,
    
    /// Skip aggregate file generation (distributed mode)
    #[arg(long)]
    pub no_aggregate: bool,
    
    /// Polling interval for JSON time-series (default: 1s)
    #[arg(long)]
    pub json_interval: Option<String>,

    /// CSV output file path
    #[arg(long)]
    pub csv_output: Option<PathBuf>,

    /// Enable Prometheus metrics endpoint
    #[arg(long)]
    pub prometheus: bool,

    /// Prometheus port
    #[arg(long, default_value = "9090")]
    pub prometheus_port: u16,
    
    /// Enable block access heatmap output
    /// Note: Enables coverage and rewrite tracking. May impact performance (5-10% overhead).
    /// Use for workload analysis and debugging, not for peak performance testing.
    #[arg(long)]
    pub heatmap: bool,
    
    /// Number of buckets for heatmap (default: 100)
    #[arg(long, default_value = "100")]
    pub heatmap_buckets: usize,

    /// Show latency statistics
    #[arg(long)]
    pub show_latency: bool,

    /// Show latency histogram
    #[arg(long)]
    pub show_histogram: bool,

    /// Show latency percentiles
    #[arg(long)]
    pub show_percentiles: bool,

    /// Live statistics update interval (e.g., 1s, 500ms)
    #[arg(long)]
    pub live_interval: Option<String>,

    /// Disable live statistics
    #[arg(long)]
    pub no_live: bool,

    // === CPU/NUMA Options ===
    /// CPU cores to bind workers to (comma-separated)
    #[arg(long)]
    pub cpu_cores: Option<String>,

    /// NUMA zones to bind workers to (comma-separated)
    #[arg(long)]
    pub numa_zones: Option<String>,

    // === Error Handling Options ===
    /// Continue on IO errors instead of aborting
    #[arg(long)]
    pub continue_on_error: bool,

    /// Maximum errors before aborting (even in continue mode)
    #[arg(long)]
    pub max_errors: Option<usize>,

    // === Data Integrity Options ===
    /// Enable data verification
    #[arg(long)]
    pub verify: bool,

    /// Verification pattern
    #[arg(long, value_enum)]
    pub verify_pattern: Option<VerifyPattern>,

    // === Configuration File ===
    /// TOML configuration file
    #[arg(short = 'c', long)]
    pub config: Option<PathBuf>,

    /// Dry run - validate configuration without executing
    #[arg(long)]
    pub dry_run: bool,
    
    /// Enable debug output (timing, file operations, etc.)
    #[arg(long)]
    pub debug: bool,
    
    /// Allow write conflicts in shared mode (benchmark mode - may cause data corruption)
    /// Use this flag to bypass write conflict detection when benchmarking raw performance.
    /// WARNING: This may result in data corruption when multiple workers write to shared files.
    #[arg(long)]
    pub allow_write_conflicts: bool,
}

/// Random distribution type
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DistributionType {
    /// Uniform random distribution
    Uniform,
    /// Zipf distribution (power law)
    Zipf,
    /// Pareto distribution (80/20 rule)
    Pareto,
    /// Gaussian/normal distribution
    Gaussian,
}

/// Think time mode
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ThinkMode {
    /// Sleep (yield CPU)
    Sleep,
    /// Spin (busy-wait)
    Spin,
}

/// IO engine type
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum EngineType {
    /// Synchronous IO (pread/pwrite)
    Sync,
    /// io_uring (Linux 5.1+)
    #[value(name = "io_uring")]
    IoUring,
    /// libaio
    Libaio,
    /// Memory-mapped IO
    Mmap,
}

/// File locking mode
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LockMode {
    /// No locking
    None,
    /// Lock byte range per IO
    Range,
    /// Lock entire file per IO
    Full,
}

/// File distribution strategy
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum FileDistributionType {
    /// All workers access all files
    Shared,
    /// Files partitioned among workers
    Partitioned,
    /// Each worker gets its own file
    PerWorker,
}

/// Data verification pattern
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum VerifyPattern {
    /// All zeros
    Zeros,
    /// All ones
    Ones,
    /// Random with seed
    Random,
    /// Sequential pattern
    Sequential,
}

impl Cli {
    /// Parse CLI arguments
    pub fn parse_args() -> Self {
        Self::parse()
    }

    /// Validate CLI arguments
    pub fn validate(&self) -> anyhow::Result<()> {
        // Service mode doesn't need validation (coordinator sends config)
        if self.mode == ExecutionMode::Service {
            return Ok(());
        }
        
        // Validate threads
        if self.threads == 0 {
            anyhow::bail!("threads must be at least 1");
        }

        // Validate queue depth
        if self.queue_depth == 0 || self.queue_depth > 1024 {
            anyhow::bail!("queue_depth must be between 1 and 1024");
        }

        // Validate read/write percentages
        if let (Some(r), Some(w)) = (self.read_percent, self.write_percent) {
            if r + w != 100 {
                anyhow::bail!("read_percent + write_percent must equal 100");
            }
        }

        // Validate distribution parameters
        match self.distribution {
            DistributionType::Zipf => {
                if self.zipf_theta < 0.0 || self.zipf_theta > 3.0 {
                    anyhow::bail!("zipf_theta must be between 0.0 and 3.0");
                }
            }
            DistributionType::Pareto => {
                if self.pareto_h < 0.0 || self.pareto_h > 10.0 {
                    anyhow::bail!("pareto_h must be between 0.0 and 10.0");
                }
            }
            DistributionType::Gaussian => {
                if self.gaussian_center < 0.0 || self.gaussian_center > 1.0 {
                    anyhow::bail!("gaussian_center must be between 0.0 and 1.0");
                }
                if self.gaussian_stddev.is_none() {
                    anyhow::bail!("gaussian_stddev is required for gaussian distribution");
                }
            }
            _ => {}
        }

        // Validate think time adaptive percent
        if let Some(pct) = self.think_adaptive_percent {
            if pct > 100 {
                anyhow::bail!("think_adaptive_percent must be between 0 and 100");
            }
        }

        // Validate completion mode
        let completion_modes = [
            self.duration.is_some(),
            self.total_bytes.is_some(),
            self.run_until_complete,
        ];
        let count = completion_modes.iter().filter(|&&x| x).count();
        if count == 0 {
            anyhow::bail!("must specify one of: --duration, --total-bytes, or --run-until-complete");
        }
        if count > 1 {
            anyhow::bail!("can only specify one completion mode");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_validate_threads() {
        // This would require mocking CLI parsing, skip for now
        // Real validation will be tested via integration tests
    }
}
