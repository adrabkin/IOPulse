//! Configuration validation

use super::*;
use anyhow::{Context, Result};

/// Validate complete configuration
pub fn validate_config(config: &Config) -> Result<()> {
    validate_workload(&config.workload)?;
    validate_targets(&config.targets)?;
    validate_workers(&config.workers)?;
    validate_output(&config.output)?;
    validate_runtime(&config.runtime)?;
    
    // Validate write conflicts (unless explicitly allowed)
    if !config.runtime.allow_write_conflicts {
        validate_write_conflicts(config)?;
    }

    Ok(())
}

/// Validate workload configuration
pub fn validate_workload(workload: &WorkloadConfig) -> Result<()> {
    // Validate read/write percentages
    if workload.read_percent + workload.write_percent != 100 {
        anyhow::bail!(
            "read_percent ({}) + write_percent ({}) must equal 100",
            workload.read_percent,
            workload.write_percent
        );
    }

    // Validate queue depth
    if workload.queue_depth == 0 || workload.queue_depth > 1024 {
        anyhow::bail!("queue_depth must be between 1 and 1024, got {}", workload.queue_depth);
    }

    // Validate read distribution weights
    if !workload.read_distribution.is_empty() {
        let total_weight: u32 = workload.read_distribution.iter().map(|p| p.weight as u32).sum();
        if total_weight != 100 {
            anyhow::bail!(
                "read_distribution weights must sum to 100, got {}",
                total_weight
            );
        }

        for (i, pattern) in workload.read_distribution.iter().enumerate() {
            validate_io_pattern(pattern, i, "read")?;
        }
    }

    // Validate write distribution weights
    if !workload.write_distribution.is_empty() {
        let total_weight: u32 = workload.write_distribution.iter().map(|p| p.weight as u32).sum();
        if total_weight != 100 {
            anyhow::bail!(
                "write_distribution weights must sum to 100, got {}",
                total_weight
            );
        }

        for (i, pattern) in workload.write_distribution.iter().enumerate() {
            validate_io_pattern(pattern, i, "write")?;
        }
    }

    // Validate distribution parameters
    validate_distribution(&workload.distribution)?;

    // Validate think time
    if let Some(ref think_time) = workload.think_time {
        validate_think_time(think_time)?;
    }

    Ok(())
}

/// Validate IO pattern
fn validate_io_pattern(pattern: &IOPattern, index: usize, op_type: &str) -> Result<()> {
    if pattern.weight == 0 {
        anyhow::bail!(
            "{} distribution pattern {} has zero weight",
            op_type,
            index
        );
    }

    if pattern.block_size < 512 {
        anyhow::bail!(
            "{} distribution pattern {} has block_size {} < 512 bytes",
            op_type,
            index,
            pattern.block_size
        );
    }

    if pattern.block_size > 64 * 1024 * 1024 {
        anyhow::bail!(
            "{} distribution pattern {} has block_size {} > 64MB",
            op_type,
            index,
            pattern.block_size
        );
    }

    // Check alignment (should be power of 2)
    if !pattern.block_size.is_power_of_two() {
        eprintln!(
            "Warning: {} distribution pattern {} block_size {} is not a power of 2",
            op_type, index, pattern.block_size
        );
    }

    Ok(())
}

/// Validate distribution parameters
fn validate_distribution(dist: &DistributionType) -> Result<()> {
    match dist {
        DistributionType::Zipf { theta } => {
            if *theta < 0.0 || *theta > 3.0 {
                anyhow::bail!("Zipf theta must be between 0.0 and 3.0, got {}", theta);
            }
        }
        DistributionType::Pareto { h } => {
            if *h < 0.0 || *h > 10.0 {
                anyhow::bail!("Pareto h must be between 0.0 and 10.0, got {}", h);
            }
        }
        DistributionType::Gaussian { stddev, center } => {
            if *stddev <= 0.0 {
                anyhow::bail!("Gaussian stddev must be positive, got {}", stddev);
            }
            if *center < 0.0 || *center > 1.0 {
                anyhow::bail!("Gaussian center must be between 0.0 and 1.0, got {}", center);
            }
        }
        DistributionType::Uniform => {}
    }

    Ok(())
}

/// Validate think time configuration
fn validate_think_time(think_time: &ThinkTimeConfig) -> Result<()> {
    if think_time.duration_us > 1_000_000 {
        anyhow::bail!(
            "think_time duration_us must be <= 1 second (1000000 us), got {}",
            think_time.duration_us
        );
    }

    if think_time.apply_every_n_blocks == 0 {
        anyhow::bail!("think_time apply_every_n_blocks must be at least 1");
    }

    if let Some(pct) = think_time.adaptive_percent {
        if pct > 100 {
            anyhow::bail!(
                "think_time adaptive_percent must be between 0 and 100, got {}",
                pct
            );
        }
    }

    Ok(())
}

/// Validate targets configuration
pub fn validate_targets(targets: &[TargetConfig]) -> Result<()> {
    if targets.is_empty() {
        anyhow::bail!("At least one target must be specified");
    }

    for (i, target) in targets.iter().enumerate() {
        validate_target(target, i)?;
    }

    Ok(())
}

/// Validate single target configuration
fn validate_target(target: &TargetConfig, index: usize) -> Result<()> {
    // Validate file size
    if let Some(size) = target.file_size {
        if size == 0 {
            anyhow::bail!("Target {} file_size must be greater than 0", index);
        }
    }

    // Validate layout config
    if let Some(ref layout) = target.layout_config {
        if layout.depth == 0 {
            anyhow::bail!("Target {} layout depth must be at least 1", index);
        }
        if layout.width == 0 {
            anyhow::bail!("Target {} layout width must be at least 1", index);
        }
        if layout.files_per_dir == 0 {
            anyhow::bail!("Target {} layout files_per_dir must be at least 1", index);
        }
    }

    // Validate conflicting flags
    if target.fadvise_flags.sequential && target.fadvise_flags.random {
        eprintln!(
            "Warning: Target {} has both sequential and random fadvise flags set",
            index
        );
    }

    if target.madvise_flags.sequential && target.madvise_flags.random {
        eprintln!(
            "Warning: Target {} has both sequential and random madvise flags set",
            index
        );
    }

    if target.madvise_flags.hugepage && target.madvise_flags.nohugepage {
        anyhow::bail!(
            "Target {} cannot have both hugepage and nohugepage madvise flags",
            index
        );
    }

    Ok(())
}

/// Validate workers configuration
pub fn validate_workers(workers: &WorkerConfig) -> Result<()> {
    if workers.threads == 0 {
        anyhow::bail!("workers.threads must be at least 1");
    }

    // Warn if thread count is very high
    if workers.threads > 1024 {
        eprintln!(
            "Warning: Very high thread count ({}), this may cause performance issues",
            workers.threads
        );
    }

    Ok(())
}

/// Validate output configuration
pub fn validate_output(output: &OutputConfig) -> Result<()> {
    if output.prometheus_port == 0 {
        anyhow::bail!("prometheus_port must be greater than 0");
    }

    if output.live_interval == Some(0) {
        anyhow::bail!("live_interval must be greater than 0");
    }

    Ok(())
}

/// Validate runtime configuration
pub fn validate_runtime(runtime: &RuntimeConfig) -> Result<()> {
    if let Some(max) = runtime.max_errors {
        if max == 0 {
            anyhow::bail!("max_errors must be greater than 0 if specified");
        }
    }

    if runtime.verify && runtime.verify_pattern.is_none() {
        eprintln!("Warning: verify enabled but no verify_pattern specified, using default");
    }

    Ok(())
}

/// Validate write conflict scenarios
/// 
/// Detects risky configurations where multiple workers may write to the same file
/// offsets simultaneously without coordination, potentially causing data corruption.
pub fn validate_write_conflicts(config: &Config) -> Result<()> {
    use crate::config::workload::FileDistribution;
    
    // Check if we have only one worker - no conflicts possible
    if config.workers.threads == 1 {
        return Ok(());
    }
    
    // Check each target
    for target in &config.targets {
        let is_shared = target.distribution == FileDistribution::Shared;
        let has_writes = config.workload.write_percent > 0;
        let is_random = config.workload.random;
        let no_locking = target.lock_mode == crate::config::workload::FileLockMode::None;
        
        // Detect risky scenario: shared + writes + random + no locks
        if is_shared && has_writes && is_random && no_locking {
            eprintln!();
            eprintln!("⚠️  WARNING: Potential write conflicts detected!");
            eprintln!();
            eprintln!("Configuration:");
            eprintln!("  - File distribution: shared (all workers access same files)");
            eprintln!("  - Write operations: {}%", config.workload.write_percent);
            eprintln!("  - Access pattern: random");
            eprintln!("  - Locking: none");
            eprintln!("  - Workers: {}", config.workers.threads);
            eprintln!();
            eprintln!("This configuration may cause data corruption because multiple workers");
            eprintln!("can write to the same file offsets simultaneously without coordination.");
            eprintln!();
            eprintln!("Real-world applications typically use one of these approaches:");
            eprintln!("  • File locking (databases, shared documents)");
            eprintln!("  • Partitioned regions (MPI-IO, parallel processing)");
            eprintln!("  • Separate files per process (logs, per-worker data)");
            eprintln!();
            eprintln!("Options to resolve:");
            eprintln!();
            eprintln!("  1. Add --lock-mode range");
            eprintln!("     Tests lock contention (realistic but slower)");
            eprintln!("     Example: {} --lock-mode range", 
                std::env::args().collect::<Vec<_>>().join(" "));
            eprintln!();
            eprintln!("  2. Use --file-distribution partitioned");
            eprintln!("     Each worker gets exclusive regions (no conflicts, faster)");
            eprintln!("     Example: {} --file-distribution partitioned", 
                std::env::args().collect::<Vec<_>>().join(" "));
            eprintln!();
            eprintln!("  3. Add --allow-write-conflicts");
            eprintln!("     Benchmark mode: measure raw performance, accept data corruption");
            eprintln!("     Example: {} --allow-write-conflicts", 
                std::env::args().collect::<Vec<_>>().join(" "));
            eprintln!();
            
            anyhow::bail!(
                "Explicit conflict handling required. Choose one of the options above.\n\
                 Use --allow-write-conflicts if you're benchmarking and don't care about data integrity."
            );
        }
    }
    
    Ok(())
}

/// Validate multi-phase configuration
pub fn validate_multi_phase_config(config: &MultiPhaseConfig) -> Result<()> {
    if config.phases.is_empty() {
        anyhow::bail!("At least one phase must be specified");
    }

    validate_targets(&config.targets)?;
    validate_workers(&config.workers)?;
    validate_output(&config.output)?;
    validate_runtime(&config.runtime)?;

    for (i, phase) in config.phases.iter().enumerate() {
        validate_phase(phase, i)?;
    }

    Ok(())
}

/// Validate phase configuration
fn validate_phase(phase: &PhaseConfig, index: usize) -> Result<()> {
    if phase.name.is_empty() {
        anyhow::bail!("Phase {} must have a non-empty name", index);
    }

    validate_workload(&phase.workload)
        .with_context(|| format!("Phase {} ({})", index, phase.name))?;

    if let Some(ref targets) = phase.targets {
        validate_targets(targets)
            .with_context(|| format!("Phase {} ({})", index, phase.name))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_validate_workload_percentages() {
        let mut workload = WorkloadConfig {
            read_percent: 70,
            write_percent: 30,
            read_distribution: vec![],
            write_distribution: vec![],
            block_size: 4096,
            queue_depth: 32,
            completion_mode: CompletionMode::RunUntilComplete,
            random: false,
            distribution: DistributionType::Uniform,
            think_time: None,
            engine: EngineType::Sync,
            direct: false,
            sync: false,
            heatmap: false,
            heatmap_buckets: 100,
            write_pattern: crate::config::workload::VerifyPattern::Random,
        };

        assert!(validate_workload(&workload).is_ok());

        workload.write_percent = 40;
        assert!(validate_workload(&workload).is_err());
    }

    #[test]
    fn test_validate_queue_depth() {
        let mut workload = WorkloadConfig {
            read_percent: 100,
            write_percent: 0,
            read_distribution: vec![],
            write_distribution: vec![],
            block_size: 4096,
            queue_depth: 0,
            completion_mode: CompletionMode::RunUntilComplete,
            random: false,
            distribution: DistributionType::Uniform,
            think_time: None,
            engine: EngineType::Sync,
            direct: false,
            sync: false,
            heatmap: false,
            heatmap_buckets: 100,
            write_pattern: crate::config::workload::VerifyPattern::Random,
        };

        assert!(validate_workload(&workload).is_err());

        workload.queue_depth = 1;
        assert!(validate_workload(&workload).is_ok());

        workload.queue_depth = 1025;
        assert!(validate_workload(&workload).is_err());
    }

    #[test]
    fn test_validate_distribution_weights() {
        let workload = WorkloadConfig {
            read_percent: 100,
            write_percent: 0,
            read_distribution: vec![
                IOPattern {
                    weight: 60,
                    access: AccessPattern::Random,
                    block_size: 4096,
                },
                IOPattern {
                    weight: 30,
                    access: AccessPattern::Sequential,
                    block_size: 131072,
                },
            ],
            write_distribution: vec![],
            block_size: 4096,
            queue_depth: 32,
            completion_mode: CompletionMode::RunUntilComplete,
            random: false,
            distribution: DistributionType::Uniform,
            think_time: None,
            engine: EngineType::Sync,
            direct: false,
            sync: false,
            heatmap: false,
            heatmap_buckets: 100,
            write_pattern: crate::config::workload::VerifyPattern::Random,
        };

        // Weights sum to 90, should fail
        assert!(validate_workload(&workload).is_err());
    }

    #[test]
    fn test_validate_distribution_params() {
        let dist = DistributionType::Zipf { theta: 1.5 };
        assert!(validate_distribution(&dist).is_ok());

        let dist = DistributionType::Zipf { theta: 3.5 };
        assert!(validate_distribution(&dist).is_err());

        let dist = DistributionType::Pareto { h: 0.9 };
        assert!(validate_distribution(&dist).is_ok());

        let dist = DistributionType::Pareto { h: 11.0 };
        assert!(validate_distribution(&dist).is_err());

        let dist = DistributionType::Gaussian {
            stddev: 0.1,
            center: 0.5,
        };
        assert!(validate_distribution(&dist).is_ok());

        let dist = DistributionType::Gaussian {
            stddev: 0.1,
            center: 1.5,
        };
        assert!(validate_distribution(&dist).is_err());
    }

    #[test]
    fn test_validate_targets() {
        let targets = vec![];
        assert!(validate_targets(&targets).is_err());

        let targets = vec![TargetConfig {
            path: PathBuf::from("/tmp/test"),
            target_type: TargetType::File,
            file_size: Some(1024 * 1024),
            num_files: None,
            num_dirs: None,
            layout_config: None,
            layout_manifest: None,
            export_layout_manifest: None,
            distribution: FileDistribution::Shared,
            fadvise_flags: FadviseFlags::default(),
            madvise_flags: MadviseFlags::default(),
            lock_mode: FileLockMode::None,
            preallocate: false,
            truncate_to_size: false,
            refill: false,
            refill_pattern: VerifyPattern::Random,
            no_refill: false,
        }];
        assert!(validate_targets(&targets).is_ok());
    }

    #[test]
    fn test_write_conflict_detection_read_only() {
        // Read-only workload should pass without warning
        let config = Config {
            workload: WorkloadConfig {
                read_percent: 100,
                write_percent: 0,
                read_distribution: vec![],
                write_distribution: vec![],
                block_size: 4096,
                queue_depth: 32,
                completion_mode: CompletionMode::Duration { seconds: 10 },
                random: true,
                distribution: DistributionType::Uniform,
                think_time: None,
                engine: EngineType::Sync,
                direct: false,
                sync: false,
                heatmap: false,
                heatmap_buckets: 100,
                write_pattern: VerifyPattern::Random,
            },
            targets: vec![TargetConfig {
                path: PathBuf::from("/tmp/test"),
                target_type: TargetType::File,
                file_size: Some(1024 * 1024 * 1024),
                num_files: None,
                num_dirs: None,
                layout_config: None,
                layout_manifest: None,
                export_layout_manifest: None,
                distribution: FileDistribution::Shared,
                fadvise_flags: FadviseFlags::default(),
                madvise_flags: MadviseFlags::default(),
                lock_mode: FileLockMode::None,
                preallocate: false,
                truncate_to_size: false,
                refill: false,
                refill_pattern: VerifyPattern::Random,
                no_refill: false,
            }],
            workers: WorkerConfig {
                threads: 8,
                cpu_cores: None,
                numa_zones: None,
                rate_limit_iops: None,
                rate_limit_throughput: None,
                offset_range: None,
            },
            output: OutputConfig::default(),
            runtime: RuntimeConfig::default(),
        };

        assert!(validate_write_conflicts(&config).is_ok());
    }

    #[test]
    fn test_write_conflict_detection_sequential() {
        // Sequential writes should pass without warning
        let config = Config {
            workload: WorkloadConfig {
                read_percent: 0,
                write_percent: 100,
                read_distribution: vec![],
                write_distribution: vec![],
                block_size: 4096,
                queue_depth: 32,
                completion_mode: CompletionMode::Duration { seconds: 10 },
                random: false, // Sequential
                distribution: DistributionType::Uniform,
                think_time: None,
                engine: EngineType::Sync,
                direct: false,
                sync: false,
                heatmap: false,
                heatmap_buckets: 100,
                write_pattern: VerifyPattern::Random,
            },
            targets: vec![TargetConfig {
                path: PathBuf::from("/tmp/test"),
                target_type: TargetType::File,
                file_size: Some(1024 * 1024 * 1024),
                num_files: None,
                num_dirs: None,
                layout_config: None,
                layout_manifest: None,
                export_layout_manifest: None,
                distribution: FileDistribution::Shared,
                fadvise_flags: FadviseFlags::default(),
                madvise_flags: MadviseFlags::default(),
                lock_mode: FileLockMode::None,
                preallocate: false,
                truncate_to_size: false,
                refill: false,
                refill_pattern: VerifyPattern::Random,
                no_refill: false,
            }],
            workers: WorkerConfig {
                threads: 8,
                cpu_cores: None,
                numa_zones: None,
                rate_limit_iops: None,
                rate_limit_throughput: None,
                offset_range: None,
            },
            output: OutputConfig::default(),
            runtime: RuntimeConfig::default(),
        };

        assert!(validate_write_conflicts(&config).is_ok());
    }

    #[test]
    fn test_write_conflict_detection_with_locking() {
        // Random writes with locking should pass
        let config = Config {
            workload: WorkloadConfig {
                read_percent: 0,
                write_percent: 100,
                read_distribution: vec![],
                write_distribution: vec![],
                block_size: 4096,
                queue_depth: 32,
                completion_mode: CompletionMode::Duration { seconds: 10 },
                random: true,
                distribution: DistributionType::Uniform,
                think_time: None,
                engine: EngineType::Sync,
                direct: false,
                sync: false,
                heatmap: false,
                heatmap_buckets: 100,
                write_pattern: VerifyPattern::Random,
            },
            targets: vec![TargetConfig {
                path: PathBuf::from("/tmp/test"),
                target_type: TargetType::File,
                file_size: Some(1024 * 1024 * 1024),
                num_files: None,
                num_dirs: None,
                layout_config: None,
                layout_manifest: None,
                export_layout_manifest: None,
                distribution: FileDistribution::Shared,
                fadvise_flags: FadviseFlags::default(),
                madvise_flags: MadviseFlags::default(),
                lock_mode: FileLockMode::Range, // Locking enabled
                preallocate: false,
                truncate_to_size: false,
                refill: false,
                refill_pattern: VerifyPattern::Random,
                no_refill: false,
            }],
            workers: WorkerConfig {
                threads: 8,
                cpu_cores: None,
                numa_zones: None,
                rate_limit_iops: None,
                rate_limit_throughput: None,
                offset_range: None,
            },
            output: OutputConfig::default(),
            runtime: RuntimeConfig::default(),
        };

        assert!(validate_write_conflicts(&config).is_ok());
    }

    #[test]
    fn test_write_conflict_detection_partitioned() {
        // Partitioned distribution should pass
        let config = Config {
            workload: WorkloadConfig {
                read_percent: 0,
                write_percent: 100,
                read_distribution: vec![],
                write_distribution: vec![],
                block_size: 4096,
                queue_depth: 32,
                completion_mode: CompletionMode::Duration { seconds: 10 },
                random: true,
                distribution: DistributionType::Uniform,
                think_time: None,
                engine: EngineType::Sync,
                direct: false,
                sync: false,
                heatmap: false,
                heatmap_buckets: 100,
                write_pattern: VerifyPattern::Random,
            },
            targets: vec![TargetConfig {
                path: PathBuf::from("/tmp/test"),
                target_type: TargetType::File,
                file_size: Some(1024 * 1024 * 1024),
                num_files: None,
                num_dirs: None,
                layout_config: None,
                layout_manifest: None,
                export_layout_manifest: None,
                distribution: FileDistribution::Partitioned, // Partitioned
                fadvise_flags: FadviseFlags::default(),
                madvise_flags: MadviseFlags::default(),
                lock_mode: FileLockMode::None,
                preallocate: false,
                truncate_to_size: false,
                refill: false,
                refill_pattern: VerifyPattern::Random,
                no_refill: false,
            }],
            workers: WorkerConfig {
                threads: 8,
                cpu_cores: None,
                numa_zones: None,
                rate_limit_iops: None,
                rate_limit_throughput: None,
                offset_range: None,
            },
            output: OutputConfig::default(),
            runtime: RuntimeConfig::default(),
        };

        assert!(validate_write_conflicts(&config).is_ok());
    }

    #[test]
    fn test_write_conflict_detection_single_worker() {
        // Single worker should pass (no conflicts possible)
        let config = Config {
            workload: WorkloadConfig {
                read_percent: 0,
                write_percent: 100,
                read_distribution: vec![],
                write_distribution: vec![],
                block_size: 4096,
                queue_depth: 32,
                completion_mode: CompletionMode::Duration { seconds: 10 },
                random: true,
                distribution: DistributionType::Uniform,
                think_time: None,
                engine: EngineType::Sync,
                direct: false,
                sync: false,
                heatmap: false,
                heatmap_buckets: 100,
                write_pattern: VerifyPattern::Random,
            },
            targets: vec![TargetConfig {
                path: PathBuf::from("/tmp/test"),
                target_type: TargetType::File,
                file_size: Some(1024 * 1024 * 1024),
                num_files: None,
                num_dirs: None,
                layout_config: None,
                layout_manifest: None,
                export_layout_manifest: None,
                distribution: FileDistribution::Shared,
                fadvise_flags: FadviseFlags::default(),
                madvise_flags: MadviseFlags::default(),
                lock_mode: FileLockMode::None,
                preallocate: false,
                truncate_to_size: false,
                refill: false,
                refill_pattern: VerifyPattern::Random,
                no_refill: false,
            }],
            workers: WorkerConfig {
                threads: 1, // Single worker
                cpu_cores: None,
                numa_zones: None,
                rate_limit_iops: None,
                rate_limit_throughput: None,
                offset_range: None,
            },
            output: OutputConfig::default(),
            runtime: RuntimeConfig::default(),
        };

        assert!(validate_write_conflicts(&config).is_ok());
    }

    #[test]
    fn test_write_conflict_detection_risky_scenario() {
        // Risky scenario: shared + random writes + no locks + multiple workers
        // This should FAIL
        let config = Config {
            workload: WorkloadConfig {
                read_percent: 0,
                write_percent: 100,
                read_distribution: vec![],
                write_distribution: vec![],
                block_size: 4096,
                queue_depth: 32,
                completion_mode: CompletionMode::Duration { seconds: 10 },
                random: true, // Random
                distribution: DistributionType::Uniform,
                think_time: None,
                engine: EngineType::Sync,
                direct: false,
                sync: false,
                heatmap: false,
                heatmap_buckets: 100,
                write_pattern: VerifyPattern::Random,
            },
            targets: vec![TargetConfig {
                path: PathBuf::from("/tmp/test"),
                target_type: TargetType::File,
                file_size: Some(1024 * 1024 * 1024),
                num_files: None,
                num_dirs: None,
                layout_config: None,
                layout_manifest: None,
                export_layout_manifest: None,
                distribution: FileDistribution::Shared, // Shared
                fadvise_flags: FadviseFlags::default(),
                madvise_flags: MadviseFlags::default(),
                lock_mode: FileLockMode::None, // No locking
                preallocate: false,
                truncate_to_size: false,
                refill: false,
                refill_pattern: VerifyPattern::Random,
                no_refill: false,
            }],
            workers: WorkerConfig {
                threads: 8, // Multiple workers
                cpu_cores: None,
                numa_zones: None,
                rate_limit_iops: None,
                rate_limit_throughput: None,
                offset_range: None,
            },
            output: OutputConfig::default(),
            runtime: RuntimeConfig::default(),
        };

        // This should fail with write conflict error
        assert!(validate_write_conflicts(&config).is_err());
    }
}
