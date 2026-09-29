//! Save / run-now mechanics for the wizard.
//!
//! `save_to` writes the wizard's TOML to a target path atomically (write to
//! a sibling tempfile, fsync, rename). `exec_iopulse` saves to a temporary
//! file *via the `tempfile` crate* (so the path is unpredictable and the
//! file is created with O_EXCL) and then exec's the current binary with
//! `--config <tempfile>`. exec() never returns on success.

use crate::wizard::pages::review::ReviewPage;
use crate::wizard::state::WizardState;
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;

/// Atomically write the wizard's TOML to `path`. Strategy: write to a sibling
/// tempfile (same directory so the rename is atomic on POSIX), fsync to
/// ensure durability, then rename onto the target.
///
/// Security: the tempfile is opened with `O_EXCL | O_NOFOLLOW` so an
/// attacker pre-creating a symlink at the predictable tempfile path can't
/// redirect the write. (`fs::File::create` would have followed such a
/// symlink and truncated the target.)
pub fn save_to(state: &WizardState, path: &Path) -> Result<()> {
    let toml_str = ReviewPage::render_toml(state);

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating directory {}", parent.display()))?;

    // Try a few suffixes; if the first is taken (someone else, or a stale
    // file from a previous run), fall through to the next. This bounds
    // retries so we never spin.
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..16 {
        let tmp_name = format!(".iopulse_wizard.{}.{}.tmp", std::process::id(), attempt);
        let tmp_path = parent.join(&tmp_name);

        let mut tmp = match OpenOptions::new()
            .write(true)
            .create_new(true) // O_EXCL — fail if the path already exists
            .custom_flags(libc::O_NOFOLLOW)
            .mode(0o600) // owner-only; no group/other read.
            .open(&tmp_path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Try the next suffix.
                last_err = Some(anyhow::anyhow!("tempfile collision at {}", tmp_path.display()));
                continue;
            }
            Err(e) => {
                return Err(anyhow::Error::from(e))
                    .with_context(|| format!("creating tempfile {}", tmp_path.display()));
            }
        };

        let write_result = (|| -> Result<()> {
            tmp.write_all(toml_str.as_bytes())
                .with_context(|| format!("writing tempfile {}", tmp_path.display()))?;
            tmp.sync_all()
                .with_context(|| format!("fsync tempfile {}", tmp_path.display()))?;
            drop(tmp);
            std::fs::rename(&tmp_path, path).with_context(|| {
                format!("renaming {} → {}", tmp_path.display(), path.display())
            })?;
            Ok(())
        })();

        if write_result.is_err() {
            let _ = std::fs::remove_file(&tmp_path);
        }
        return write_result;
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("unable to allocate tempfile after 16 tries")))
}

/// Save to a private tempfile under the user's runtime/config dir and exec
/// the current binary with `--config <tempfile>`. On success this never
/// returns. On failure the error is propagated.
///
/// Tempfile location: `$XDG_RUNTIME_DIR/iopulse-wizard-<random>.toml` if
/// available (per-user, mode 0700 by spec), else `~/.cache/iopulse/`. We
/// avoid `/tmp` because it's world-writable and the wizard's PID is
/// predictable.
pub fn exec_iopulse(state: &WizardState) -> Result<()> {
    let tmp_path = stage_run_tempfile(state)?;
    let exe = std::env::current_exe().context("locating current executable")?;
    let err = std::process::Command::new(exe)
        .arg("--config")
        .arg(&tmp_path)
        .exec();
    Err(anyhow::anyhow!("exec failed: {}", err))
}

/// Write the wizard's TOML to a private per-user tempfile and return the path.
/// Public for testability — `exec_iopulse` cannot itself be unit-tested
/// because exec() replaces the process.
pub fn stage_run_tempfile(state: &WizardState) -> Result<std::path::PathBuf> {
    use std::path::PathBuf;

    let dir = if let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(rt)
    } else if let Some(home) = std::env::var_os("HOME") {
        let dir = PathBuf::from(home).join(".cache").join("iopulse");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        dir
    } else {
        anyhow::bail!("neither XDG_RUNTIME_DIR nor HOME is set; cannot stage tempfile");
    };

    let tmp = tempfile::Builder::new()
        .prefix("iopulse-wizard-")
        .suffix(".toml")
        .tempfile_in(&dir)
        .with_context(|| format!("creating tempfile in {}", dir.display()))?;
    let path = tmp.path().to_path_buf();
    let toml_str = ReviewPage::render_toml(state);
    std::fs::write(&path, toml_str)
        .with_context(|| format!("writing tempfile {}", path.display()))?;
    // Persist (don't delete on drop) so exec'd child can read it.
    let (_, persisted) = tmp
        .keep()
        .map_err(|e| anyhow::anyhow!("persisting tempfile: {}", e))?;
    Ok(persisted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_save_to_writes_target() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("wizard.toml");
        let state = WizardState::new();
        save_to(&state, &path).expect("save");
        let written = fs::read_to_string(&path).expect("read back");
        assert!(written.contains("[workload]"));
    }

    #[test]
    fn test_save_to_does_not_leave_tempfile() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("wizard.toml");
        let state = WizardState::new();
        save_to(&state, &path).expect("save");
        // Walk the tempdir; the only file should be wizard.toml.
        let entries: Vec<_> = fs::read_dir(dir.path())
            .expect("readdir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], "wizard.toml");
    }

    #[test]
    fn test_save_to_overwrites_existing() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("wizard.toml");
        fs::write(&path, "old content").expect("seed");

        let state = WizardState::new();
        save_to(&state, &path).expect("save");

        let written = fs::read_to_string(&path).expect("read back");
        assert!(written.contains("[workload]"));
        assert!(!written.contains("old content"));
    }

    #[test]
    fn test_save_to_creates_parent_directories() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("nested/deeper/wizard.toml");
        let state = WizardState::new();
        save_to(&state, &path).expect("save creates parent");
        assert!(path.exists());
    }

    #[test]
    fn test_save_to_refuses_symlinked_tempfile() {
        // If an attacker pre-creates a symlink at the predictable tempfile
        // path, save_to must NOT follow it. We simulate by pre-creating
        // every possible suffix as a symlink and expecting save_to to fail
        // (after exhausting its 16 attempts).
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("wizard.toml");
        let victim = dir.path().join("victim.txt");
        fs::write(&victim, b"important").expect("seed victim");

        // Plant symlinks for all 16 attempt suffixes.
        for attempt in 0..16 {
            let tmp_name = format!(".iopulse_wizard.{}.{}.tmp", std::process::id(), attempt);
            let link = dir.path().join(&tmp_name);
            std::os::unix::fs::symlink(&victim, &link).expect("symlink");
        }

        let state = WizardState::new();
        let result = save_to(&state, &target);
        assert!(result.is_err(), "save_to should refuse symlinked tempfile");

        // Victim file must be unchanged.
        let after = fs::read(&victim).expect("victim still readable");
        assert_eq!(after, b"important", "victim was overwritten through symlink");
    }

    #[test]
    fn test_stage_run_tempfile_writes_unpredictable_path() {
        // Stage twice; the paths must differ (random suffix).
        let dir = tempdir().expect("tempdir");
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());

        let state = WizardState::new();
        let p1 = stage_run_tempfile(&state).expect("stage 1");
        let p2 = stage_run_tempfile(&state).expect("stage 2");
        assert_ne!(p1, p2, "tempfile paths must be unpredictable");
        assert!(p1.starts_with(dir.path()));
        assert!(fs::read_to_string(&p1).unwrap().contains("[workload]"));

        std::env::remove_var("XDG_RUNTIME_DIR");
    }

    // exec_iopulse can't be tested directly without forking — exec replaces
    // the current process. stage_run_tempfile covers the staging logic.
}
