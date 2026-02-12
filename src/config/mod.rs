//! Configuration module
//!
//! Handles CLI argument parsing, TOML configuration files, and validation.

pub mod cli;
pub mod cli_convert;
pub mod toml;
pub mod validator;
pub mod workload;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use workload::*;

/// Complete test configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub workload: WorkloadConfig,
    pub targets: Vec<TargetConfig>,
    #[serde(default)]
    pub workers: WorkerConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
}

/// Workload configuration with composite IO patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadConfig {
    /// Read percentage (0-100)
    pub read_percent: u8,
    /// Write percentage (0-100)
    pub write_percent: u8,
    /// Read operation distribution
    #[serde(default)]
    pub read_distribution: Vec<IOPattern>,
    /// Write operation distribution
    #[serde(default)]
    pub write_distribution: Vec<IOPattern>,
    /// Default block size (used when distributions are empty)
    #[serde(default = "default_block_size")]
    pub block_size: u64,
    /// IO queue depth (1-1024)
    #[serde(default = "default_queue_depth")]
    pub queue_depth: usize,
    /// Completion mode
    pub completion_mode: CompletionMode,
    /// Use random offsets (true) or sequential (false)
    #[serde(default)]
    pub random: bool,
    /// Random distribution type (only used if random=true)
    #[serde(default)]
    pub distribution: DistributionType,
    /// Think time configuration
    pub think_time: Option<ThinkTimeConfig>,
    /// IO engine type
    #[serde(default)]
    pub engine: EngineType,
    /// Use direct IO (O_DIRECT)
    #[serde(default)]
    pub direct: bool,
    /// Use synchronous IO (O_SYNC)
    #[serde(default)]
    pub sync: bool,
    /// Enable block access heatmap
    #[serde(default)]
    pub heatmap: bool,
    /// Number of buckets for heatmap
    #[serde(default = "default_heatmap_buckets")]
    pub heatmap_buckets: usize,
    /// Pattern to use for write buffer data
    #[serde(default)]
    pub write_pattern: VerifyPattern,
}

fn default_block_size() -> u64 {
    4096
}

fn default_queue_depth() -> usize {
    1
}

fn default_heatmap_buckets() -> usize {
    100
}

/// Target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    /// Path to target (file, directory, or block device)
    pub path: PathBuf,
    /// Target type
    #[serde(default)]
    pub target_type: TargetType,
    /// File size (for file creation)
    pub file_size: Option<u64>,
    /// Number of files
    pub num_files: Option<usize>,
    /// Number of directories
    pub num_dirs: Option<usize>,
    /// Directory layout configuration
    pub layout_config: Option<LayoutConfig>,
    /// Layout manifest path (input)
    pub layout_manifest: Option<PathBuf>,
    /// Export layout manifest path (output)
    pub export_layout_manifest: Option<PathBuf>,
    /// File distribution strategy
    #[serde(default)]
    pub distribution: FileDistribution,
    /// fadvise flags
    #[serde(default)]
    pub fadvise_flags: FadviseFlags,
    /// madvise flags
    #[serde(default)]
    pub madvise_flags: MadviseFlags,
    /// File locking mode
    #[serde(default)]
    pub lock_mode: FileLockMode,
    /// Pre-allocate file space
    #[serde(default)]
    pub preallocate: bool,
    /// Truncate to size on creation
    #[serde(default)]
    pub truncate_to_size: bool,
    /// Fill pre-allocated files with pattern data
    #[serde(default)]
    pub refill: bool,
    /// Pattern to use for refill operation
    #[serde(default)]
    pub refill_pattern: VerifyPattern,
    /// Disable automatic file filling for read tests
    #[serde(default)]
    pub no_refill: bool,
}

/// Target type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TargetType {
    File,
    BlockDevice,
    Directory,
}

impl Default for TargetType {
    fn default() -> Self {
        Self::File
    }
}

/// Directory layout configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    /// Directory depth (number of nested levels)
    pub depth: usize,
    /// Directory width (subdirectories per level)
    pub width: usize,
    /// Files per directory
    pub files_per_dir: usize,
    /// File naming pattern
    #[serde(default)]
    pub naming_pattern: NamingPattern,
    /// Number of workers (for per-worker distribution)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_workers: Option<usize>,
    /// Exact total number of files to generate (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_files: Option<usize>,
}

/// File naming pattern
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NamingPattern {
    Sequential,
    Random,
    Prefixed,
}

impl Default for NamingPattern {
    fn default() -> Self {
        Self::Sequential
    }
}

/// Worker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    /// Number of worker threads
    #[serde(default = "default_threads")]
    pub threads: usize,
    /// CPU cores to bind to (comma-separated)
    pub cpu_cores: Option<String>,
    /// NUMA zones to bind to (comma-separated)
    pub numa_zones: Option<String>,
    /// Rate limit (IOPS per worker)
    pub rate_limit_iops: Option<u64>,
    /// Rate limit (throughput per worker in bytes/sec)
    pub rate_limit_throughput: Option<u64>,
    /// Offset range for partitioned distribution (start_offset, end_offset)
    /// Only used when file_distribution is Partitioned
    #[serde(skip)]
    pub offset_range: Option<(u64, u64)>,
}

fn default_threads() -> usize {
    1
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            threads: default_threads(),
            cpu_cores: None,
            numa_zones: None,
            rate_limit_iops: None,
            rate_limit_throughput: None,
            offset_range: None,
        }
    }
}

/// Output configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// JSON output file path or directory
    pub json_output: Option<PathBuf>,
    /// Name for aggregate JSON file
    #[serde(default = "default_json_name")]
    pub json_name: String,
    /// Generate separate histogram file
    #[serde(default)]
    pub json_histogram: bool,
    /// Include per-worker stats in time-series output (JSON and CSV)
    #[serde(default)]
    pub per_worker_output: bool,
    /// Skip aggregate file generation
    #[serde(default)]
    pub no_aggregate: bool,
    /// Polling interval for JSON time-series (seconds)
    pub json_interval: Option<u64>,
    /// CSV output file path
    pub csv_output: Option<PathBuf>,
    /// Enable Prometheus metrics
    #[serde(default)]
    pub prometheus: bool,
    /// Prometheus port
    #[serde(default = "default_prometheus_port")]
    pub prometheus_port: u16,
    /// Show latency statistics
    #[serde(default)]
    pub show_latency: bool,
    /// Show latency histogram
    #[serde(default)]
    pub show_histogram: bool,
    /// Show latency percentiles
    #[serde(default)]
    pub show_percentiles: bool,
    /// Live statistics interval (seconds)
    pub live_interval: Option<u64>,
    /// Disable live statistics
    #[serde(default)]
    pub no_live: bool,
    /// Output verbosity level
    #[serde(default)]
    pub verbosity: u8,
}

fn default_json_name() -> String {
    "aggregate".to_string()
}

fn default_prometheus_port() -> u16 {
    9090
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            json_output: None,
            json_name: default_json_name(),
            json_histogram: false,
            per_worker_output: false,
            no_aggregate: false,
            json_interval: None,
            csv_output: None,
            prometheus: false,
            prometheus_port: default_prometheus_port(),
            show_latency: false,
            show_histogram: false,
            show_percentiles: false,
            live_interval: None,
            no_live: false,
            verbosity: 0,
        }
    }
}

/// Runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Continue on IO errors
    #[serde(default)]
    pub continue_on_error: bool,
    /// Maximum errors before aborting
    pub max_errors: Option<usize>,
    /// Continue on worker failure (distributed mode)
    #[serde(default)]
    pub continue_on_worker_failure: bool,
    /// Enable data verification
    #[serde(default)]
    pub verify: bool,
    /// Verification pattern
    pub verify_pattern: Option<VerifyPattern>,
    /// Dry run mode
    #[serde(default)]
    pub dry_run: bool,
    /// Enable debug output
    #[serde(default)]
    pub debug: bool,
    /// Allow write conflicts in shared mode (benchmark mode)
    #[serde(default)]
    pub allow_write_conflicts: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            continue_on_error: false,
            max_errors: None,
            continue_on_worker_failure: false,
            verify: false,
            verify_pattern: None,
            dry_run: false,
            debug: false,
            allow_write_conflicts: false,
        }
    }
}

/// Phase definition for multi-phase tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseConfig {
    /// Phase name
    pub name: String,
    /// Workload for this phase
    pub workload: WorkloadConfig,
    /// Targets for this phase (optional, uses global if not specified)
    pub targets: Option<Vec<TargetConfig>>,
    /// Stonewall synchronization
    #[serde(default)]
    pub stonewall: bool,
}

/// Multi-phase configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiPhaseConfig {
    /// Global targets (used if phase doesn't specify targets)
    pub targets: Vec<TargetConfig>,
    /// Worker configuration
    #[serde(default)]
    pub workers: WorkerConfig,
    /// Output configuration
    #[serde(default)]
    pub output: OutputConfig,
    /// Runtime configuration
    #[serde(default)]
    pub runtime: RuntimeConfig,
    /// Phases to execute in sequence
    pub phases: Vec<PhaseConfig>,
}

// Display trait implementations

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Configuration:")?;
        writeln!(f, "  Workload: {}", self.workload)?;
        writeln!(f, "  Targets: {} target(s)", self.targets.len())?;
        writeln!(f, "  Workers: {}", self.workers)?;
        writeln!(f, "  Output: {}", self.output)?;
        writeln!(f, "  Runtime: {}", self.runtime)?;
        Ok(())
    }
}

impl fmt::Display for WorkloadConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}% read / {}% write, queue_depth={}, engine={}, completion={}",
            self.read_percent, self.write_percent, self.queue_depth, self.engine, self.completion_mode
        )?;
        if !self.read_distribution.is_empty() {
            write!(f, ", read_dist=[{}]", 
                self.read_distribution.iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
        if !self.write_distribution.is_empty() {
            write!(f, ", write_dist=[{}]", 
                self.write_distribution.iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
        if let Some(ref think_time) = self.think_time {
            write!(f, ", think_time={}", think_time)?;
        }
        Ok(())
    }
}

impl fmt::Display for TargetConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.path.display(), self.target_type)?;
        if let Some(size) = self.file_size {
            write!(f, ", size={}", format_bytes(size))?;
        }
        if let Some(num) = self.num_files {
            write!(f, ", files={}", num)?;
        }
        if self.lock_mode != FileLockMode::None {
            write!(f, ", lock={}", self.lock_mode)?;
        }
        Ok(())
    }
}

impl fmt::Display for TargetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetType::File => write!(f, "file"),
            TargetType::BlockDevice => write!(f, "block_device"),
            TargetType::Directory => write!(f, "directory"),
        }
    }
}

impl fmt::Display for LayoutConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "depth={}, width={}, files_per_dir={}, naming={}",
            self.depth, self.width, self.files_per_dir, self.naming_pattern
        )?;
        if let Some(num_workers) = self.num_workers {
            write!(f, ", workers={}", num_workers)?;
        }
        Ok(())
    }
}

impl fmt::Display for NamingPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NamingPattern::Sequential => write!(f, "sequential"),
            NamingPattern::Random => write!(f, "random"),
            NamingPattern::Prefixed => write!(f, "prefixed"),
        }
    }
}

impl fmt::Display for WorkerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} thread(s)", self.threads)?;
        if let Some(ref cores) = self.cpu_cores {
            write!(f, ", cpu_cores={}", cores)?;
        }
        if let Some(ref zones) = self.numa_zones {
            write!(f, ", numa_zones={}", zones)?;
        }
        Ok(())
    }
}

impl fmt::Display for OutputConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if let Some(ref path) = self.json_output {
            parts.push(format!("json={}", path.display()));
        }
        if let Some(ref path) = self.csv_output {
            parts.push(format!("csv={}", path.display()));
        }
        if self.prometheus {
            parts.push(format!("prometheus=:{}", self.prometheus_port));
        }
        if parts.is_empty() {
            write!(f, "text output")
        } else {
            write!(f, "{}", parts.join(", "))
        }
    }
}

impl fmt::Display for RuntimeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.continue_on_error {
            parts.push("continue_on_error".to_string());
        }
        if self.continue_on_worker_failure {
            parts.push("continue_on_worker_failure".to_string());
        }
        if self.verify {
            parts.push(format!("verify={}", 
                self.verify_pattern.map(|p| p.to_string()).unwrap_or_else(|| "default".to_string())
            ));
        }
        if self.dry_run {
            parts.push("dry_run".to_string());
        }
        if parts.is_empty() {
            write!(f, "default")
        } else {
            write!(f, "{}", parts.join(", "))
        }
    }
}

impl fmt::Display for PhaseConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Phase '{}': {}", self.name, self.workload)?;
        if self.stonewall {
            write!(f, " (stonewall)")?;
        }
        Ok(())
    }
}

// Validation methods

impl Config {
    /// Validate the complete configuration
    pub fn validate(&self) -> Result<(), String> {
        self.workload.validate()?;
        
        if self.targets.is_empty() {
            return Err("At least one target must be specified".to_string());
        }
        
        for (i, target) in self.targets.iter().enumerate() {
            target.validate().map_err(|e| format!("Target {}: {}", i, e))?;
        }
        
        self.workers.validate()?;
        self.output.validate()?;
        self.runtime.validate()?;
        
        Ok(())
    }
}

impl WorkloadConfig {
    /// Convert WorkloadConfig to engine::EngineConfig
    ///
    /// This creates an EngineConfig suitable for initializing IO engines.
    /// The io_uring-specific optimizations are enabled based on the engine type
    /// and queue depth.
    pub fn to_engine_config(&self) -> crate::engine::EngineConfig {
        crate::engine::EngineConfig {
            queue_depth: self.queue_depth,
            // Enable optimizations for io_uring with high queue depths
            use_registered_buffers: matches!(self.engine, workload::EngineType::IoUring) 
                && self.queue_depth >= 32,
            use_fixed_files: matches!(self.engine, workload::EngineType::IoUring) 
                && self.queue_depth >= 32,
            polling_mode: false, // Can be exposed in config later if needed
        }
    }

    /// Validate the workload configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate read/write percentages
        if self.read_percent > 100 {
            return Err(format!("read_percent must be 0-100, got {}", self.read_percent));
        }
        if self.write_percent > 100 {
            return Err(format!("write_percent must be 0-100, got {}", self.write_percent));
        }
        if self.read_percent + self.write_percent != 100 {
            return Err(format!(
                "read_percent + write_percent must equal 100, got {} + {} = {}",
                self.read_percent,
                self.write_percent,
                self.read_percent + self.write_percent
            ));
        }
        
        // Validate queue depth
        if self.queue_depth == 0 {
            return Err("queue_depth must be greater than 0".to_string());
        }
        if self.queue_depth > 1024 {
            return Err(format!("queue_depth must be at most 1024, got {}", self.queue_depth));
        }
        
        // Validate read distribution
        if !self.read_distribution.is_empty() {
            let total: u32 = self.read_distribution.iter().map(|p| p.weight as u32).sum();
            if total != 100 {
                return Err(format!(
                    "read_distribution weights must sum to 100, got {}",
                    total
                ));
            }
            for (i, pattern) in self.read_distribution.iter().enumerate() {
                pattern.validate().map_err(|e| format!("read_distribution[{}]: {}", i, e))?;
            }
        }
        
        // Validate write distribution
        if !self.write_distribution.is_empty() {
            let total: u32 = self.write_distribution.iter().map(|p| p.weight as u32).sum();
            if total != 100 {
                return Err(format!(
                    "write_distribution weights must sum to 100, got {}",
                    total
                ));
            }
            for (i, pattern) in self.write_distribution.iter().enumerate() {
                pattern.validate().map_err(|e| format!("write_distribution[{}]: {}", i, e))?;
            }
        }
        
        // Validate distribution type
        self.distribution.validate()?;
        
        // Validate completion mode
        self.completion_mode.validate()?;
        
        // Validate think time
        if let Some(ref think_time) = self.think_time {
            think_time.validate()?;
        }
        
        Ok(())
    }
}

impl TargetConfig {
    /// Validate the target configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate file size
        if let Some(size) = self.file_size {
            if size == 0 {
                return Err("file_size must be greater than 0".to_string());
            }
        }
        
        // Validate num_files
        if let Some(num) = self.num_files {
            if num == 0 {
                return Err("num_files must be greater than 0".to_string());
            }
        }
        
        // Validate num_dirs
        if let Some(num) = self.num_dirs {
            if num == 0 {
                return Err("num_dirs must be greater than 0".to_string());
            }
        }
        
        // Validate layout config
        if let Some(ref layout) = self.layout_config {
            layout.validate()?;
        }
        
        // Validate fadvise flags
        self.fadvise_flags.validate()?;
        
        // Validate madvise flags
        self.madvise_flags.validate()?;
        
        Ok(())
    }
}

impl LayoutConfig {
    /// Validate the layout configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.depth == 0 {
            return Err("layout depth must be greater than 0".to_string());
        }
        if self.width == 0 {
            return Err("layout width must be greater than 0".to_string());
        }
        if self.files_per_dir == 0 {
            return Err("files_per_dir must be greater than 0".to_string());
        }
        
        // Check for potential overflow
        let total_dirs = self.width.saturating_pow(self.depth as u32);
        if total_dirs == usize::MAX {
            return Err(format!(
                "layout configuration would create too many directories (depth={}, width={})",
                self.depth, self.width
            ));
        }
        
        Ok(())
    }
}

impl WorkerConfig {
    /// Validate the worker configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.threads == 0 {
            return Err("threads must be greater than 0".to_string());
        }
        
        // Validate CPU cores format if specified
        if let Some(ref cores) = self.cpu_cores {
            validate_cpu_list(cores)?;
        }
        
        // Validate NUMA zones format if specified
        if let Some(ref zones) = self.numa_zones {
            validate_numa_list(zones)?;
        }
        
        Ok(())
    }
}

impl OutputConfig {
    /// Validate the output configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.prometheus_port == 0 {
            return Err("prometheus_port must be greater than 0".to_string());
        }
        
        if let Some(interval) = self.live_interval {
            if interval == 0 {
                return Err("live_interval must be greater than 0".to_string());
            }
        }
        
        Ok(())
    }
}

impl RuntimeConfig {
    /// Validate the runtime configuration
    pub fn validate(&self) -> Result<(), String> {
        if let Some(max) = self.max_errors {
            if max == 0 {
                return Err("max_errors must be greater than 0 if specified".to_string());
            }
        }
        
        Ok(())
    }
}

impl PhaseConfig {
    /// Validate the phase configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("phase name cannot be empty".to_string());
        }
        
        self.workload.validate()?;
        
        if let Some(ref targets) = self.targets {
            if targets.is_empty() {
                return Err("phase targets cannot be empty if specified".to_string());
            }
            for (i, target) in targets.iter().enumerate() {
                target.validate().map_err(|e| format!("Phase '{}' target {}: {}", self.name, i, e))?;
            }
        }
        
        Ok(())
    }
}

impl MultiPhaseConfig {
    /// Validate the multi-phase configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.targets.is_empty() {
            return Err("At least one global target must be specified".to_string());
        }
        
        for (i, target) in self.targets.iter().enumerate() {
            target.validate().map_err(|e| format!("Global target {}: {}", i, e))?;
        }
        
        if self.phases.is_empty() {
            return Err("At least one phase must be specified".to_string());
        }
        
        for phase in &self.phases {
            phase.validate()?;
        }
        
        self.workers.validate()?;
        self.output.validate()?;
        self.runtime.validate()?;
        
        Ok(())
    }
}

// Helper functions

fn validate_cpu_list(list: &str) -> Result<(), String> {
    for part in list.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let range: Vec<&str> = part.split('-').collect();
            if range.len() != 2 {
                return Err(format!("Invalid CPU range: {}", part));
            }
            let start: usize = range[0].parse()
                .map_err(|_| format!("Invalid CPU number: {}", range[0]))?;
            let end: usize = range[1].parse()
                .map_err(|_| format!("Invalid CPU number: {}", range[1]))?;
            if start >= end {
                return Err(format!("Invalid CPU range: start must be less than end in {}", part));
            }
        } else {
            part.parse::<usize>()
                .map_err(|_| format!("Invalid CPU number: {}", part))?;
        }
    }
    Ok(())
}

fn validate_numa_list(list: &str) -> Result<(), String> {
    for part in list.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let range: Vec<&str> = part.split('-').collect();
            if range.len() != 2 {
                return Err(format!("Invalid NUMA range: {}", part));
            }
            let start: usize = range[0].parse()
                .map_err(|_| format!("Invalid NUMA node: {}", range[0]))?;
            let end: usize = range[1].parse()
                .map_err(|_| format!("Invalid NUMA node: {}", range[1]))?;
            if start >= end {
                return Err(format!("Invalid NUMA range: start must be less than end in {}", part));
            }
        } else {
            part.parse::<usize>()
                .map_err(|_| format!("Invalid NUMA node: {}", part))?;
        }
    }
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2}TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workload_to_engine_config_sync() {
        let workload = WorkloadConfig {
            read_percent: 100,
            write_percent: 0,
            read_distribution: vec![],
            write_distribution: vec![],
            queue_depth: 32,
            completion_mode: CompletionMode::RunUntilComplete,
            distribution: DistributionType::Uniform,
            think_time: None,
            engine: workload::EngineType::Sync,
            direct: false,
            sync: false,
        };

        let engine_config = workload.to_engine_config();
        assert_eq!(engine_config.queue_depth, 32);
        assert!(!engine_config.use_registered_buffers); // Sync doesn't use these
        assert!(!engine_config.use_fixed_files);
        assert!(!engine_config.polling_mode);
    }

    #[test]
    fn test_workload_to_engine_config_io_uring_high_qd() {
        let workload = WorkloadConfig {
            read_percent: 100,
            write_percent: 0,
            read_distribution: vec![],
            write_distribution: vec![],
            queue_depth: 64,
            completion_mode: CompletionMode::RunUntilComplete,
            distribution: DistributionType::Uniform,
            think_time: None,
            engine: workload::EngineType::IoUring,
            direct: false,
            sync: false,
        };

        let engine_config = workload.to_engine_config();
        assert_eq!(engine_config.queue_depth, 64);
        assert!(engine_config.use_registered_buffers); // Enabled for io_uring with QD >= 32
        assert!(engine_config.use_fixed_files);
    }

    #[test]
    fn test_workload_to_engine_config_io_uring_low_qd() {
        let workload = WorkloadConfig {
            read_percent: 100,
            write_percent: 0,
            read_distribution: vec![],
            write_distribution: vec![],
            queue_depth: 8,
            completion_mode: CompletionMode::RunUntilComplete,
            distribution: DistributionType::Uniform,
            think_time: None,
            engine: workload::EngineType::IoUring,
            direct: false,
            sync: false,
        };

        let engine_config = workload.to_engine_config();
        assert_eq!(engine_config.queue_depth, 8);
        assert!(!engine_config.use_registered_buffers); // Not enabled for low QD
        assert!(!engine_config.use_fixed_files);
    }

    #[test]
    fn test_workload_to_engine_config_libaio() {
        let workload = WorkloadConfig {
            read_percent: 100,
            write_percent: 0,
            read_distribution: vec![],
            write_distribution: vec![],
            queue_depth: 128,
            completion_mode: CompletionMode::RunUntilComplete,
            distribution: DistributionType::Uniform,
            think_time: None,
            engine: workload::EngineType::Libaio,
            direct: false,
            sync: false,
        };

        let engine_config = workload.to_engine_config();
        assert_eq!(engine_config.queue_depth, 128);
        assert!(!engine_config.use_registered_buffers); // libaio doesn't use io_uring features
        assert!(!engine_config.use_fixed_files);
    }
}
