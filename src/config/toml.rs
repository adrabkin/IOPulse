//! TOML configuration file parsing

use super::*;
use crate::config::cli::{Cli, DistributionType as CliDistType, EngineType as CliEngineType};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Parse TOML configuration file
pub fn parse_toml_file(path: &Path) -> Result<Config> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    parse_toml_string(&contents)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))
}

/// Parse TOML configuration from string
pub fn parse_toml_string(contents: &str) -> Result<Config> {
    let config: Config = ::toml::from_str(contents)
        .context("Failed to parse TOML configuration")?;

    Ok(config)
}

/// Parse multi-phase TOML configuration
pub fn parse_multi_phase_toml(path: &Path) -> Result<MultiPhaseConfig> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: MultiPhaseConfig = ::toml::from_str(&contents)
        .context("Failed to parse multi-phase TOML configuration")?;

    Ok(config)
}

/// Merge CLI arguments with TOML configuration (CLI takes precedence)
pub fn merge_cli_with_config(cli: &Cli, mut config: Config) -> Result<Config> {
    // Override workload settings from CLI
    if let Some(read_pct) = cli.read_percent {
        config.workload.read_percent = read_pct;
    }
    if let Some(write_pct) = cli.write_percent {
        config.workload.write_percent = write_pct;
    }

    // Override queue depth
    if cli.queue_depth != 1 {
        config.workload.queue_depth = cli.queue_depth;
    }

    // Override distribution
    config.workload.distribution = match cli.distribution {
        CliDistType::Uniform => DistributionType::Uniform,
        CliDistType::Zipf => DistributionType::Zipf { theta: cli.zipf_theta },
        CliDistType::Pareto => DistributionType::Pareto { h: cli.pareto_h },
        CliDistType::Gaussian => {
            let stddev = cli.gaussian_stddev.unwrap_or(0.1);
            DistributionType::Gaussian {
                stddev,
                center: cli.gaussian_center,
            }
        }
    };

    // Override completion mode
    if let Some(duration_str) = &cli.duration {
        let seconds = parse_duration(duration_str)?;
        if seconds == 0 {
            // Duration 0 means "run until file is complete"
            config.workload.completion_mode = CompletionMode::RunUntilComplete;
        } else {
            config.workload.completion_mode = CompletionMode::Duration { seconds };
        }
    } else if let Some(bytes_str) = &cli.total_bytes {
        let bytes = parse_size(bytes_str)?;
        config.workload.completion_mode = CompletionMode::TotalBytes { bytes };
    } else if cli.run_until_complete {
        config.workload.completion_mode = CompletionMode::RunUntilComplete;
    }

    // Override think time
    if let Some(think_str) = &cli.think_time {
        let duration_us = parse_duration_us(think_str)?;
        config.workload.think_time = Some(ThinkTimeConfig {
            duration_us,
            mode: match cli.think_mode {
                cli::ThinkMode::Sleep => ThinkTimeMode::Sleep,
                cli::ThinkMode::Spin => ThinkTimeMode::Spin,
            },
            apply_every_n_blocks: cli.think_every,
            adaptive_percent: cli.think_adaptive_percent,
        });
    }

    // Override engine
    config.workload.engine = match cli.engine {
        CliEngineType::Sync => EngineType::Sync,
        CliEngineType::IoUring => EngineType::IoUring,
        CliEngineType::Libaio => EngineType::Libaio,
        CliEngineType::Mmap => EngineType::Mmap,
    };

    // Override direct/sync flags
    if cli.direct {
        config.workload.direct = true;
    }
    if cli.sync {
        config.workload.sync = true;
    }

    // Override worker settings
    if cli.threads != 1 {
        config.workers.threads = cli.threads;
    }
    if let Some(ref cores) = cli.cpu_cores {
        config.workers.cpu_cores = Some(cores.clone());
    }
    if let Some(ref zones) = cli.numa_zones {
        config.workers.numa_zones = Some(zones.clone());
    }

    // Override output settings
    if let Some(ref path) = cli.json_output {
        config.output.json_output = Some(path.clone());
    }
    if let Some(ref path) = cli.csv_output {
        config.output.csv_output = Some(path.clone());
    }
    if cli.prometheus {
        config.output.prometheus = true;
        config.output.prometheus_port = cli.prometheus_port;
    }
    if cli.show_latency {
        config.output.show_latency = true;
    }
    if cli.show_histogram {
        config.output.show_histogram = true;
    }
    if cli.show_percentiles {
        config.output.show_percentiles = true;
    }
    if let Some(ref interval_str) = cli.live_interval {
        let seconds = parse_duration(interval_str)?;
        config.output.live_interval = Some(seconds);
    }
    if cli.no_live {
        config.output.no_live = true;
    }

    // Override runtime settings
    if cli.continue_on_error {
        config.runtime.continue_on_error = true;
    }
    if let Some(max) = cli.max_errors {
        config.runtime.max_errors = Some(max);
    }
    if cli.verify {
        config.runtime.verify = true;
    }
    if let Some(pattern) = cli.verify_pattern {
        config.runtime.verify_pattern = Some(match pattern {
            cli::VerifyPattern::Zeros => VerifyPattern::Zeros,
            cli::VerifyPattern::Ones => VerifyPattern::Ones,
            cli::VerifyPattern::Random => VerifyPattern::Random,
            cli::VerifyPattern::Sequential => VerifyPattern::Sequential,
        });
    }
    if cli.dry_run {
        config.runtime.dry_run = true;
    }

    // Override target settings if CLI provides target
    if let Some(ref target_path) = cli.target {
        // If config has no targets or CLI explicitly provides one, use CLI target
        if config.targets.is_empty() {
            config.targets.push(create_target_from_cli(cli)?);
        } else {
            // Override first target's path
            config.targets[0].path = target_path.clone();
            apply_cli_target_overrides(&mut config.targets[0], cli)?;
        }
    }

    // Apply CLI overrides to all targets
    for target in &mut config.targets {
        apply_cli_target_overrides(target, cli)?;
    }

    Ok(config)
}

/// Create a target configuration from CLI arguments
fn create_target_from_cli(cli: &Cli) -> Result<TargetConfig> {
    let target_path = cli.target.clone()
        .ok_or_else(|| anyhow::anyhow!("Target path required"))?;
    
    let target = TargetConfig {
        path: target_path,
        target_type: TargetType::File,
        file_size: cli.file_size.as_ref().map(|s| parse_size(s)).transpose()?,
        num_files: cli.num_files,
        num_dirs: cli.num_dirs,
        layout_config: None,
        layout_manifest: cli.layout_manifest.clone(),
        export_layout_manifest: cli.export_layout_manifest.clone(),
        distribution: match cli.file_distribution {
            cli::FileDistributionType::Shared => FileDistribution::Shared,
            cli::FileDistributionType::Partitioned => FileDistribution::Partitioned,
            cli::FileDistributionType::PerWorker => FileDistribution::PerWorker,
        },
        fadvise_flags: parse_fadvise_flags(cli.fadvise.as_deref())?,
        madvise_flags: parse_madvise_flags(cli.madvise.as_deref())?,
        lock_mode: match cli.lock_mode {
            cli::LockMode::None => FileLockMode::None,
            cli::LockMode::Range => FileLockMode::Range,
            cli::LockMode::Full => FileLockMode::Full,
        },
        preallocate: cli.preallocate,  // Default: false
        truncate_to_size: cli.truncate_to_size,
        refill: cli.refill,
        refill_pattern: match cli.refill_pattern {
            cli::VerifyPattern::Zeros => VerifyPattern::Zeros,
            cli::VerifyPattern::Ones => VerifyPattern::Ones,
            cli::VerifyPattern::Random => VerifyPattern::Random,
            cli::VerifyPattern::Sequential => VerifyPattern::Sequential,
        },
        no_refill: cli.no_refill,
    };

    Ok(target)
}

/// Apply CLI overrides to a target configuration
fn apply_cli_target_overrides(target: &mut TargetConfig, cli: &Cli) -> Result<()> {
    if let Some(ref size_str) = cli.file_size {
        target.file_size = Some(parse_size(size_str)?);
    }
    if let Some(num) = cli.num_files {
        target.num_files = Some(num);
    }
    if let Some(num) = cli.num_dirs {
        target.num_dirs = Some(num);
    }
    if cli.preallocate {  // Only preallocate if --preallocate flag is passed
        target.preallocate = true;
    }
    if cli.truncate_to_size {
        target.truncate_to_size = true;
    }

    // Override fadvise flags if provided
    if cli.fadvise.is_some() {
        target.fadvise_flags = parse_fadvise_flags(cli.fadvise.as_deref())?;
    }

    // Override madvise flags if provided
    if cli.madvise.is_some() {
        target.madvise_flags = parse_madvise_flags(cli.madvise.as_deref())?;
    }

    // Override lock mode if not default
    if !matches!(cli.lock_mode, cli::LockMode::None) {
        target.lock_mode = match cli.lock_mode {
            cli::LockMode::None => FileLockMode::None,
            cli::LockMode::Range => FileLockMode::Range,
            cli::LockMode::Full => FileLockMode::Full,
        };
    }

    // Override file distribution if not default
    if !matches!(cli.file_distribution, cli::FileDistributionType::Shared) {
        target.distribution = match cli.file_distribution {
            cli::FileDistributionType::Shared => FileDistribution::Shared,
            cli::FileDistributionType::Partitioned => FileDistribution::Partitioned,
            cli::FileDistributionType::PerWorker => FileDistribution::PerWorker,
        };
    }

    Ok(())
}

/// Parse duration string (e.g., "60s", "5m", "1h") to seconds
fn parse_duration(s: &str) -> Result<u64> {
    let s = s.trim();
    if s.is_empty() {
        anyhow::bail!("Empty duration string");
    }

    let (num_str, unit) = if s.ends_with("ms") {
        (&s[..s.len() - 2], "ms")
    } else {
        let unit_start = s.len() - 1;
        (&s[..unit_start], &s[unit_start..])
    };

    let num: u64 = num_str.parse()
        .with_context(|| format!("Invalid number in duration: {}", num_str))?;

    let seconds = match unit {
        "s" => num,
        "m" => num * 60,
        "h" => num * 3600,
        "ms" => {
            if num < 1000 {
                1 // Round up to 1 second
            } else {
                num / 1000
            }
        }
        _ => anyhow::bail!("Invalid duration unit: {}. Use s, m, h, or ms", unit),
    };

    Ok(seconds)
}

/// Parse duration string to microseconds (e.g., "100us", "1ms", "10ms")
fn parse_duration_us(s: &str) -> Result<u64> {
    let s = s.trim();
    if s.is_empty() {
        anyhow::bail!("Empty duration string");
    }

    let (num_str, unit) = if s.ends_with("us") {
        (&s[..s.len() - 2], "us")
    } else if s.ends_with("ms") {
        (&s[..s.len() - 2], "ms")
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], "s")
    } else {
        anyhow::bail!("Duration must end with us, ms, or s");
    };

    let num: u64 = num_str.parse()
        .with_context(|| format!("Invalid number in duration: {}", num_str))?;

    let microseconds = match unit {
        "us" => num,
        "ms" => num * 1000,
        "s" => num * 1_000_000,
        _ => anyhow::bail!("Invalid duration unit: {}", unit),
    };

    Ok(microseconds)
}

/// Parse size string (e.g., "1G", "100M", "4k") to bytes
fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim().to_uppercase();
    if s.is_empty() {
        anyhow::bail!("Empty size string");
    }

    let (num_str, multiplier) = if s.ends_with('K') {
        (&s[..s.len() - 1], 1024u64)
    } else if s.ends_with('M') {
        (&s[..s.len() - 1], 1024 * 1024)
    } else if s.ends_with('G') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024)
    } else if s.ends_with('T') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024 * 1024)
    } else {
        (s.as_str(), 1)
    };

    let num: u64 = num_str.parse()
        .with_context(|| format!("Invalid number in size: {}", num_str))?;

    Ok(num * multiplier)
}

/// Parse fadvise flags from comma-separated string
fn parse_fadvise_flags(s: Option<&str>) -> Result<FadviseFlags> {
    let mut flags = FadviseFlags::default();

    if let Some(s) = s {
        for flag in s.split(',') {
            match flag.trim().to_lowercase().as_str() {
                "seq" | "sequential" => flags.sequential = true,
                "rand" | "random" => flags.random = true,
                "willneed" => flags.willneed = true,
                "dontneed" => flags.dontneed = true,
                "noreuse" => flags.noreuse = true,
                "" => {}
                other => anyhow::bail!("Invalid fadvise flag: {}", other),
            }
        }
    }

    Ok(flags)
}

/// Parse madvise flags from comma-separated string
fn parse_madvise_flags(s: Option<&str>) -> Result<MadviseFlags> {
    let mut flags = MadviseFlags::default();

    if let Some(s) = s {
        for flag in s.split(',') {
            match flag.trim().to_lowercase().as_str() {
                "seq" | "sequential" => flags.sequential = true,
                "rand" | "random" => flags.random = true,
                "willneed" => flags.willneed = true,
                "dontneed" => flags.dontneed = true,
                "hugepage" => flags.hugepage = true,
                "nohugepage" => flags.nohugepage = true,
                "" => {}
                other => anyhow::bail!("Invalid madvise flag: {}", other),
            }
        }
    }

    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("60s").unwrap(), 60);
        assert_eq!(parse_duration("5m").unwrap(), 300);
        assert_eq!(parse_duration("1h").unwrap(), 3600);
        assert_eq!(parse_duration("1000ms").unwrap(), 1);
        assert_eq!(parse_duration("500ms").unwrap(), 1); // Rounds up
    }

    #[test]
    fn test_parse_duration_us() {
        assert_eq!(parse_duration_us("100us").unwrap(), 100);
        assert_eq!(parse_duration_us("1ms").unwrap(), 1000);
        assert_eq!(parse_duration_us("1s").unwrap(), 1_000_000);
    }

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("4k").unwrap(), 4096);
        assert_eq!(parse_size("1M").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("1024").unwrap(), 1024);
    }

    #[test]
    fn test_parse_fadvise_flags() {
        let flags = parse_fadvise_flags(Some("seq,willneed")).unwrap();
        assert!(flags.sequential);
        assert!(flags.willneed);
        assert!(!flags.random);

        let flags = parse_fadvise_flags(None).unwrap();
        assert!(!flags.sequential);
    }

    #[test]
    fn test_parse_madvise_flags() {
        let flags = parse_madvise_flags(Some("random,hugepage")).unwrap();
        assert!(flags.random);
        assert!(flags.hugepage);
        assert!(!flags.sequential);
    }

    #[test]
    fn test_parse_toml_basic() {
        let toml = r#"
[workload]
read_percent = 70
write_percent = 30
queue_depth = 32

[workload.completion_mode]
mode = "duration"
seconds = 60

[[targets]]
path = "/tmp/testfile"

[workers]
threads = 4
"#;

        let config = parse_toml_string(toml).unwrap();
        assert_eq!(config.workload.read_percent, 70);
        assert_eq!(config.workload.write_percent, 30);
        assert_eq!(config.workload.queue_depth, 32);
        assert_eq!(config.workers.threads, 4);
        assert_eq!(config.targets.len(), 1);
    }

    #[test]
    fn test_parse_toml_with_distributions() {
        let toml = r#"
[workload]
read_percent = 100
write_percent = 0
queue_depth = 1

[workload.completion_mode]
mode = "run_until_complete"

[workload.distribution]
type = "zipf"
theta = 1.5

[[targets]]
path = "/tmp/testfile"
"#;

        let config = parse_toml_string(toml).unwrap();
        match config.workload.distribution {
            DistributionType::Zipf { theta } => assert_eq!(theta, 1.5),
            _ => panic!("Expected Zipf distribution"),
        }
    }

    #[test]
    fn test_parse_toml_with_io_patterns() {
        let toml = r#"
[workload]
read_percent = 80
write_percent = 20
queue_depth = 16

[workload.completion_mode]
mode = "total_bytes"
bytes = 1073741824

[[workload.read_distribution]]
weight = 70
access = "random"
block_size = 4096

[[workload.read_distribution]]
weight = 30
access = "sequential"
block_size = 131072

[[targets]]
path = "/tmp/testfile"
"#;

        let config = parse_toml_string(toml).unwrap();
        assert_eq!(config.workload.read_distribution.len(), 2);
        assert_eq!(config.workload.read_distribution[0].weight, 70);
        assert_eq!(config.workload.read_distribution[0].block_size, 4096);
        assert_eq!(config.workload.read_distribution[1].weight, 30);
    }

    #[test]
    fn test_parse_toml_multi_phase() {
        let toml = r#"
[[targets]]
path = "/tmp/testfile"

[workers]
threads = 8

[[phases]]
name = "warmup"

[phases.workload]
read_percent = 100
write_percent = 0
queue_depth = 32

[phases.workload.completion_mode]
mode = "duration"
seconds = 30

[[phases]]
name = "main"

[phases.workload]
read_percent = 70
write_percent = 30
queue_depth = 64

[phases.workload.completion_mode]
mode = "duration"
seconds = 300
"#;

        let config: MultiPhaseConfig = ::toml::from_str(toml).unwrap();
        assert_eq!(config.phases.len(), 2);
        assert_eq!(config.phases[0].name, "warmup");
        assert_eq!(config.phases[1].name, "main");
        assert_eq!(config.phases[1].workload.queue_depth, 64);
    }
}
