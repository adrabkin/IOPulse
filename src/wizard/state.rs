//! Wizard state model.
//!
//! `WizardState` is the single source of truth while the user is in the wizard.
//! It owns a live `Config` (mutated directly by pages — no separate state struct,
//! per design §3) plus wizard-specific UI state (current page, dirty flag,
//! validation cache, output path, optional multi-phase config).

use crate::config::cli::ExecutionMode;
use crate::config::workload::{
    CompletionMode, DistributionType, EngineType, FadviseFlags, FileDistribution, FileLockMode,
    MadviseFlags, VerifyPattern,
};
use crate::config::{
    Config, MultiPhaseConfig, OutputConfig, RuntimeConfig, TargetConfig, TargetType, WorkerConfig,
    WorkloadConfig,
};
use std::path::PathBuf;

/// Severity of a validation issue surfaced to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A single validation issue surfaced in the status bar / detail popup.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: Severity,
    /// Index of the page that produced this issue.
    pub page: usize,
    /// Human-readable message.
    pub message: String,
}

/// Live wizard state. Pages mutate `config` directly; `dirty` flips on every mutation
/// so the validation pass can be skipped when nothing changed.
#[derive(Debug)]
pub struct WizardState {
    /// The config being built.
    pub config: Config,
    /// Execution mode (standalone / coordinator / service). Lives outside
    /// `Config` because it's a CLI flag, not a config field — captured here
    /// so the Save phase can write it into the launch command alongside the TOML.
    pub execution_mode: ExecutionMode,
    /// Mode-specific detail string. Coordinator: comma-separated host:port list.
    /// Service: listen port. Standalone: None.
    pub mode_detail: Option<String>,
    /// Coordinator-only: optional file path for the host list (alternative
    /// to the inline `mode_detail` host list).
    pub clients_file: Option<String>,
    /// Coordinator-only: default port to dial workers on when the host list
    /// entries omit ports. Empty / None = use clap's default (9999).
    pub worker_port: Option<u16>,
    /// Index of the page currently focused (0-based).
    pub current_page: usize,
    /// True when `config` has changed since the last validation pass.
    pub dirty: bool,
    /// Validation issues from the most recent pass.
    pub validation: Vec<ValidationIssue>,
    /// Path the wizard will write its TOML to on save.
    pub output_path: PathBuf,
    /// Optional multi-phase config (mutually exclusive with single-phase `config`).
    pub multi_phase: Option<MultiPhaseConfig>,
    /// Transient feedback message shown in the status bar (e.g., "cannot save:
    /// 2 errors"). Cleared on the next dirty-validation cycle.
    pub flash: Option<String>,
}

impl WizardState {
    /// Build a starter `WizardState` with a sensible default `Config`.
    ///
    /// `Config` itself does not implement `Default` (it requires `targets` and
    /// `workload.completion_mode`), so the wizard provides starter values:
    /// 100% read, 4K block, queue depth 1, 10s duration, sync engine, target
    /// path empty (the user fills this in on the Target page).
    pub fn new() -> Self {
        Self {
            config: starter_config(),
            execution_mode: ExecutionMode::Standalone,
            mode_detail: None,
            clients_file: None,
            worker_port: None,
            current_page: 0,
            dirty: false,
            validation: vec![],
            output_path: PathBuf::from("./iopulse.toml"),
            multi_phase: None,
            flash: None,
        }
    }

    /// Run the global config validator and seed `self.validation` with any
    /// errors it surfaces. Page-level issues are merged in by the event loop
    /// before this is consulted by the status bar.
    ///
    /// This is the source of truth for "is the current config valid"; the
    /// wizard reuses `validator::validate_config` rather than reimplementing
    /// rules locally so the wizard's surface always matches what the runtime
    /// will accept.
    pub fn run_global_validation(&mut self) {
        self.validation.clear();
        if let Err(e) = crate::config::validator::validate_config(&self.config) {
            self.validation.push(ValidationIssue {
                severity: Severity::Error,
                page: self.current_page,
                message: e.to_string(),
            });
        }
        self.dirty = false;
    }

    /// Total counts of (errors, warnings) currently in `self.validation`.
    pub fn issue_counts(&self) -> (usize, usize) {
        let mut errors = 0;
        let mut warnings = 0;
        for issue in &self.validation {
            match issue.severity {
                Severity::Error => errors += 1,
                Severity::Warning => warnings += 1,
            }
        }
        (errors, warnings)
    }

    /// Errors on the current page or earlier. Used to gate forward navigation:
    /// errors on pages the user hasn't reached yet shouldn't block them from
    /// getting there, since visiting the page is how they fix the error.
    /// Global validator errors (which carry the current page index) always count.
    pub fn errors_blocking_advance(&self) -> usize {
        self.validation
            .iter()
            .filter(|i| i.severity == Severity::Error && i.page <= self.current_page)
            .count()
    }
}

impl Default for WizardState {
    fn default() -> Self {
        Self::new()
    }
}

fn starter_config() -> Config {
    Config {
        workload: WorkloadConfig {
            read_percent: 100,
            write_percent: 0,
            read_distribution: vec![],
            write_distribution: vec![],
            block_size: 4096,
            queue_depth: 1,
            completion_mode: CompletionMode::Duration { seconds: 10 },
            random: false,
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
            path: PathBuf::new(),
            target_type: TargetType::File,
            file_size: None,
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
        workers: WorkerConfig::default(),
        output: OutputConfig::default(),
        runtime: RuntimeConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wizard_state_default_is_valid_config() {
        let state = WizardState::default();
        assert_eq!(state.current_page, 0);
        assert!(!state.dirty);
        assert!(state.validation.is_empty());
        assert!(state.multi_phase.is_none());
        assert_eq!(state.output_path, PathBuf::from("./iopulse.toml"));

        // The starter Config must serialize cleanly. (Validation may still flag
        // "target path is empty" — that's expected; the user fills it in.)
        let _toml =
            toml::to_string_pretty(&state.config).expect("starter Config serializes to TOML");
    }

    #[test]
    fn test_starter_config_passes_workload_validation() {
        let state = WizardState::new();
        state
            .config
            .workload
            .validate()
            .expect("starter workload is valid");
    }

    #[test]
    fn test_run_global_validation_flags_bad_workload_mix() {
        let mut state = WizardState::new();
        // Force a workload that's structurally invalid: read+write != 100.
        state.config.workload.read_percent = 60;
        state.config.workload.write_percent = 30;
        state.run_global_validation();
        let (errors, _warnings) = state.issue_counts();
        assert!(errors >= 1, "expected at least one error from validator");
    }

    #[test]
    fn test_issue_counts_zero_when_clean() {
        let state = WizardState::new();
        // No validation has run → counts are zero.
        let (errors, warnings) = state.issue_counts();
        assert_eq!(errors, 0);
        assert_eq!(warnings, 0);
    }

    #[test]
    fn test_run_global_validation_clears_dirty_flag() {
        let mut state = WizardState::new();
        state.dirty = true;
        state.run_global_validation();
        assert!(!state.dirty);
    }
}
