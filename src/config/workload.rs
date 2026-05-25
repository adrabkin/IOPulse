//! Workload definition structures

use serde::{Deserialize, Serialize};
use std::fmt;

/// Individual IO pattern within a workload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IOPattern {
    /// Percentage weight within operation type (0-100)
    pub weight: u8,
    /// Access pattern (sequential or random)
    pub access: AccessPattern,
    /// Block size in bytes
    pub block_size: u64,
}

/// Access pattern type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccessPattern {
    Sequential,
    Random,
}

/// Random distribution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DistributionType {
    Uniform,
    Zipf { theta: f64 },
    Pareto { h: f64 },
    Gaussian { stddev: f64, center: f64 },
}

impl Default for DistributionType {
    fn default() -> Self {
        Self::Uniform
    }
}

/// Completion criteria
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompletionMode {
    Duration { seconds: u64 },
    TotalBytes { bytes: u64 },
    RunUntilComplete,
}

/// Think time mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ThinkTimeMode {
    Sleep,
    Spin,
}

/// Think time configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkTimeConfig {
    /// Duration in microseconds
    pub duration_us: u64,
    /// Mode (sleep or spin)
    pub mode: ThinkTimeMode,
    /// Apply every N blocks
    #[serde(default = "default_think_every")]
    pub apply_every_n_blocks: usize,
    /// Adaptive percentage of IO latency
    pub adaptive_percent: Option<u8>,
}

fn default_think_every() -> usize {
    1
}

/// File distribution strategy
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileDistribution {
    /// All workers access all files
    Shared,
    /// Files divided among workers
    Partitioned,
    /// Each worker gets its own file
    PerWorker,
}

impl Default for FileDistribution {
    fn default() -> Self {
        Self::Shared
    }
}

/// File locking mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileLockMode {
    None,
    Range,
    Full,
}

impl Default for FileLockMode {
    fn default() -> Self {
        Self::None
    }
}

/// fadvise flags
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FadviseFlags {
    pub sequential: bool,
    pub random: bool,
    pub willneed: bool,
    pub dontneed: bool,
    pub noreuse: bool,
}

/// madvise flags
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MadviseFlags {
    pub sequential: bool,
    pub random: bool,
    pub willneed: bool,
    pub dontneed: bool,
    pub hugepage: bool,
    pub nohugepage: bool,
}

/// IO engine type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EngineType {
    Sync,
    #[serde(rename = "io_uring")]
    IoUring,
    Libaio,
    Mmap,
}

impl Default for EngineType {
    fn default() -> Self {
        Self::Sync
    }
}

/// Verification pattern
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerifyPattern {
    Zeros,
    Ones,
    Random,
    Sequential,
}

// Display trait implementations

impl fmt::Display for IOPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}% {} {}B",
            self.weight,
            self.access,
            self.block_size
        )
    }
}

impl fmt::Display for AccessPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccessPattern::Sequential => write!(f, "sequential"),
            AccessPattern::Random => write!(f, "random"),
        }
    }
}

impl fmt::Display for DistributionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DistributionType::Uniform => write!(f, "uniform"),
            DistributionType::Zipf { theta } => write!(f, "zipf(theta={})", theta),
            DistributionType::Pareto { h } => write!(f, "pareto(h={})", h),
            DistributionType::Gaussian { stddev, center } => {
                write!(f, "gaussian(stddev={}, center={})", stddev, center)
            }
        }
    }
}

impl fmt::Display for CompletionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompletionMode::Duration { seconds } => write!(f, "duration({}s)", seconds),
            CompletionMode::TotalBytes { bytes } => {
                write!(f, "total_bytes({})", format_bytes(*bytes))
            }
            CompletionMode::RunUntilComplete => write!(f, "run_until_complete"),
        }
    }
}

impl fmt::Display for ThinkTimeMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThinkTimeMode::Sleep => write!(f, "sleep"),
            ThinkTimeMode::Spin => write!(f, "spin"),
        }
    }
}

impl fmt::Display for ThinkTimeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(pct) = self.adaptive_percent {
            if self.duration_us == 0 {
                // Adaptive-only mode
                write!(f, "adaptive {}% of IO latency every {} blocks", pct, self.apply_every_n_blocks)?;
            } else {
                // Base + adaptive mode
                write!(f, "{}us {} every {} blocks (adaptive +{}%)", 
                    self.duration_us, self.mode, self.apply_every_n_blocks, pct)?;
            }
        } else {
            // Fixed duration mode
            write!(f, "{}us {} every {} blocks", self.duration_us, self.mode, self.apply_every_n_blocks)?;
        }
        Ok(())
    }
}

impl fmt::Display for FileDistribution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileDistribution::Shared => write!(f, "shared"),
            FileDistribution::Partitioned => write!(f, "partitioned"),
            FileDistribution::PerWorker => write!(f, "per-worker"),
        }
    }
}

impl fmt::Display for FileLockMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileLockMode::None => write!(f, "none"),
            FileLockMode::Range => write!(f, "range"),
            FileLockMode::Full => write!(f, "full"),
        }
    }
}

impl fmt::Display for FadviseFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut flags = Vec::new();
        if self.sequential {
            flags.push("sequential");
        }
        if self.random {
            flags.push("random");
        }
        if self.willneed {
            flags.push("willneed");
        }
        if self.dontneed {
            flags.push("dontneed");
        }
        if self.noreuse {
            flags.push("noreuse");
        }
        if flags.is_empty() {
            write!(f, "none")
        } else {
            write!(f, "{}", flags.join("|"))
        }
    }
}

impl fmt::Display for MadviseFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut flags = Vec::new();
        if self.sequential {
            flags.push("sequential");
        }
        if self.random {
            flags.push("random");
        }
        if self.willneed {
            flags.push("willneed");
        }
        if self.dontneed {
            flags.push("dontneed");
        }
        if self.hugepage {
            flags.push("hugepage");
        }
        if self.nohugepage {
            flags.push("nohugepage");
        }
        if flags.is_empty() {
            write!(f, "none")
        } else {
            write!(f, "{}", flags.join("|"))
        }
    }
}

impl fmt::Display for EngineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineType::Sync => write!(f, "sync"),
            EngineType::IoUring => write!(f, "io_uring"),
            EngineType::Libaio => write!(f, "libaio"),
            EngineType::Mmap => write!(f, "mmap"),
        }
    }
}

impl Default for VerifyPattern {
    fn default() -> Self {
        Self::Random
    }
}

impl fmt::Display for VerifyPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyPattern::Zeros => write!(f, "zeros"),
            VerifyPattern::Ones => write!(f, "ones"),
            VerifyPattern::Random => write!(f, "random"),
            VerifyPattern::Sequential => write!(f, "sequential"),
        }
    }
}

// Validation methods

impl IOPattern {
    /// Validate the IO pattern
    pub fn validate(&self) -> Result<(), String> {
        if self.weight > 100 {
            return Err(format!("IOPattern weight must be 0-100, got {}", self.weight));
        }
        if self.block_size == 0 {
            return Err("IOPattern block_size must be greater than 0".to_string());
        }
        if self.block_size < 512 {
            return Err(format!(
                "IOPattern block_size must be at least 512 bytes, got {}",
                self.block_size
            ));
        }
        if self.block_size > 64 * 1024 * 1024 {
            return Err(format!(
                "IOPattern block_size must be at most 64MB, got {}",
                self.block_size
            ));
        }
        Ok(())
    }
}

impl DistributionType {
    /// Validate the distribution parameters
    pub fn validate(&self) -> Result<(), String> {
        match self {
            DistributionType::Uniform => Ok(()),
            DistributionType::Zipf { theta } => {
                if *theta < 0.0 || *theta > 3.0 {
                    Err(format!(
                        "Zipf theta must be in range 0.0-3.0, got {}",
                        theta
                    ))
                } else {
                    Ok(())
                }
            }
            DistributionType::Pareto { h } => {
                if *h < 0.0 || *h > 10.0 {
                    Err(format!("Pareto h must be in range 0.0-10.0, got {}", h))
                } else {
                    Ok(())
                }
            }
            DistributionType::Gaussian { stddev, center } => {
                if *stddev <= 0.0 {
                    Err(format!(
                        "Gaussian stddev must be greater than 0, got {}",
                        stddev
                    ))
                } else if *center < 0.0 || *center > 1.0 {
                    Err(format!(
                        "Gaussian center must be in range 0.0-1.0, got {}",
                        center
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }
}

impl CompletionMode {
    /// Validate the completion mode
    pub fn validate(&self) -> Result<(), String> {
        match self {
            CompletionMode::Duration { seconds } => {
                if *seconds == 0 {
                    Err("Duration must be greater than 0".to_string())
                } else {
                    Ok(())
                }
            }
            CompletionMode::TotalBytes { bytes } => {
                if *bytes == 0 {
                    Err("TotalBytes must be greater than 0".to_string())
                } else {
                    Ok(())
                }
            }
            CompletionMode::RunUntilComplete => Ok(()),
        }
    }
}

impl ThinkTimeConfig {
    /// Validate the think time configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.duration_us > 1_000_000 {
            return Err(format!(
                "Think time duration must be at most 1 second (1,000,000 us), got {}",
                self.duration_us
            ));
        }
        if self.apply_every_n_blocks == 0 {
            return Err("apply_every_n_blocks must be greater than 0".to_string());
        }
        if let Some(pct) = self.adaptive_percent {
            if pct > 100 {
                return Err(format!(
                    "adaptive_percent must be 0-100, got {}",
                    pct
                ));
            }
        }
        Ok(())
    }
}

impl FadviseFlags {
    /// Validate fadvise flags
    pub fn validate(&self) -> Result<(), String> {
        if self.sequential && self.random {
            return Err("Cannot specify both sequential and random fadvise flags".to_string());
        }
        if self.willneed && self.dontneed {
            return Err("Cannot specify both willneed and dontneed fadvise flags".to_string());
        }
        Ok(())
    }
}

impl MadviseFlags {
    /// Validate madvise flags
    pub fn validate(&self) -> Result<(), String> {
        if self.sequential && self.random {
            return Err("Cannot specify both sequential and random madvise flags".to_string());
        }
        if self.willneed && self.dontneed {
            return Err("Cannot specify both willneed and dontneed madvise flags".to_string());
        }
        if self.hugepage && self.nohugepage {
            return Err("Cannot specify both hugepage and nohugepage madvise flags".to_string());
        }
        Ok(())
    }
}

// Helper function for formatting bytes
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
