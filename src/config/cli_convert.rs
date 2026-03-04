//! CLI to Config conversion utilities

use crate::config::cli;
use crate::config::workload;
use anyhow::{Context, Result};

/// Parse a size string (e.g., "1G", "100M", "4k") to bytes
pub fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim().to_lowercase();
    
    let (num_str, multiplier) = if s.ends_with("k") || s.ends_with("kb") {
        (s.trim_end_matches("kb").trim_end_matches("k"), 1024u64)
    } else if s.ends_with("m") || s.ends_with("mb") {
        (s.trim_end_matches("mb").trim_end_matches("m"), 1024 * 1024)
    } else if s.ends_with("g") || s.ends_with("gb") {
        (s.trim_end_matches("gb").trim_end_matches("g"), 1024 * 1024 * 1024)
    } else if s.ends_with("t") || s.ends_with("tb") {
        (s.trim_end_matches("tb").trim_end_matches("t"), 1024 * 1024 * 1024 * 1024)
    } else {
        (s.as_str(), 1)
    };
    
    let num: u64 = num_str.parse()
        .with_context(|| format!("Invalid size format: {}", s))?;
    
    Ok(num * multiplier)
}

/// Parse a duration string (e.g., "60s", "5m", "1h") to seconds
pub fn parse_duration(s: &str) -> Result<u64> {
    let s = s.trim().to_lowercase();
    
    let (num_str, multiplier) = if s.ends_with("s") || s.ends_with("sec") {
        (s.trim_end_matches("sec").trim_end_matches("s"), 1u64)
    } else if s.ends_with("m") || s.ends_with("min") {
        (s.trim_end_matches("min").trim_end_matches("m"), 60)
    } else if s.ends_with("h") || s.ends_with("hr") {
        (s.trim_end_matches("hr").trim_end_matches("h"), 3600)
    } else {
        (s.as_str(), 1)
    };
    
    let num: u64 = num_str.parse()
        .with_context(|| format!("Invalid duration format: {}", s))?;
    
    Ok(num * multiplier)
}

/// Parse a time string (e.g., "100us", "1ms", "10ms") to microseconds
pub fn parse_time_us(s: &str) -> Result<u64> {
    let s = s.trim().to_lowercase();
    
    let (num_str, multiplier) = if s.ends_with("us") {
        (s.trim_end_matches("us"), 1u64)
    } else if s.ends_with("ms") {
        (s.trim_end_matches("ms"), 1000)
    } else if s.ends_with("s") {
        (s.trim_end_matches("s"), 1_000_000)
    } else {
        (s.as_str(), 1)
    };
    
    let num: u64 = num_str.parse()
        .with_context(|| format!("Invalid time format: {}", s))?;
    
    Ok(num * multiplier)
}

/// Convert CLI EngineType to workload EngineType
pub fn convert_engine_type(cli_type: cli::EngineType) -> workload::EngineType {
    match cli_type {
        cli::EngineType::Sync => workload::EngineType::Sync,
        cli::EngineType::IoUring => workload::EngineType::IoUring,
        cli::EngineType::Libaio => workload::EngineType::Libaio,
        cli::EngineType::Mmap => workload::EngineType::Mmap,
    }
}

/// Convert CLI DistributionType to workload DistributionType
pub fn convert_distribution_type(
    cli_type: cli::DistributionType,
    zipf_theta: f64,
    pareto_h: f64,
    gaussian_stddev: Option<f64>,
    gaussian_center: f64,
) -> Result<workload::DistributionType> {
    match cli_type {
        cli::DistributionType::Uniform => Ok(workload::DistributionType::Uniform),
        cli::DistributionType::Zipf => Ok(workload::DistributionType::Zipf { theta: zipf_theta }),
        cli::DistributionType::Pareto => Ok(workload::DistributionType::Pareto { h: pareto_h }),
        cli::DistributionType::Gaussian => {
            let stddev = gaussian_stddev
                .ok_or_else(|| anyhow::anyhow!("gaussian_stddev required for gaussian distribution"))?;
            Ok(workload::DistributionType::Gaussian {
                stddev,
                center: gaussian_center,
            })
        }
    }
}

/// Convert CLI VerifyPattern to workload VerifyPattern
pub fn convert_verify_pattern(cli_pattern: cli::VerifyPattern) -> workload::VerifyPattern {
    match cli_pattern {
        cli::VerifyPattern::Zeros => workload::VerifyPattern::Zeros,
        cli::VerifyPattern::Ones => workload::VerifyPattern::Ones,
        cli::VerifyPattern::Random => workload::VerifyPattern::Random,
        cli::VerifyPattern::Sequential => workload::VerifyPattern::Sequential,
    }
}

/// Convert CLI LockMode to workload FileLockMode
pub fn convert_lock_mode(cli_mode: cli::LockMode) -> workload::FileLockMode {
    match cli_mode {
        cli::LockMode::None => workload::FileLockMode::None,
        cli::LockMode::Range => workload::FileLockMode::Range,
        cli::LockMode::Full => workload::FileLockMode::Full,
    }
}

/// Convert CLI FileDistributionType to workload FileDistribution
pub fn convert_file_distribution(cli_dist: cli::FileDistributionType) -> workload::FileDistribution {
    match cli_dist {
        cli::FileDistributionType::Shared => workload::FileDistribution::Shared,
        cli::FileDistributionType::Partitioned => workload::FileDistribution::Partitioned,
        cli::FileDistributionType::PerWorker => workload::FileDistribution::PerWorker,
    }
}

/// Convert CLI ThinkMode to workload ThinkTimeMode
pub fn convert_think_mode(cli_mode: cli::ThinkMode) -> workload::ThinkTimeMode {
    match cli_mode {
        cli::ThinkMode::Sleep => workload::ThinkTimeMode::Sleep,
        cli::ThinkMode::Spin => workload::ThinkTimeMode::Spin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_size_bytes() {
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert_eq!(parse_size("512").unwrap(), 512);
    }
    
    #[test]
    fn test_parse_size_kb() {
        assert_eq!(parse_size("4k").unwrap(), 4096);
        assert_eq!(parse_size("4K").unwrap(), 4096);
        assert_eq!(parse_size("4kb").unwrap(), 4096);
        assert_eq!(parse_size("4KB").unwrap(), 4096);
    }
    
    #[test]
    fn test_parse_size_mb() {
        assert_eq!(parse_size("1m").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("100M").unwrap(), 100 * 1024 * 1024);
        assert_eq!(parse_size("1mb").unwrap(), 1024 * 1024);
    }
    
    #[test]
    fn test_parse_size_gb() {
        assert_eq!(parse_size("1g").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("10G").unwrap(), 10 * 1024 * 1024 * 1024);
    }
    
    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("60").unwrap(), 60);
        assert_eq!(parse_duration("60s").unwrap(), 60);
        assert_eq!(parse_duration("60sec").unwrap(), 60);
    }
    
    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("5m").unwrap(), 300);
        assert_eq!(parse_duration("5min").unwrap(), 300);
    }
    
    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("1h").unwrap(), 3600);
        assert_eq!(parse_duration("2hr").unwrap(), 7200);
    }
    
    #[test]
    fn test_parse_time_us() {
        assert_eq!(parse_time_us("100us").unwrap(), 100);
        assert_eq!(parse_time_us("1ms").unwrap(), 1000);
        assert_eq!(parse_time_us("1s").unwrap(), 1_000_000);
    }
}
