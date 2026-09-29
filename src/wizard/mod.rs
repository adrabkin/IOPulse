//! Interactive TUI config builder.
//!
//! Entry point for `iopulse wizard`. Pages and the live preview pane land in
//! Phases 2-3; this module currently boots the event loop with a blank screen
//! and exits on `q` / Ctrl-C with the terminal restored.

pub mod app;
pub mod pages;
pub mod preview;
pub mod save;
pub mod state;

use crate::config::cli::WizardArgs;
use crate::wizard::app::{install_panic_hook, run_loop_drawing, LoopExit, TerminalGuard};
use crate::wizard::pages::build_pages_synced;
use crate::wizard::state::WizardState;
use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::path::PathBuf;

/// Run the wizard. Builds initial state, enters raw mode, and dispatches the
/// event loop until the user quits. The TerminalGuard restores cooked mode on
/// Drop regardless of how this returns.
pub fn run(args: WizardArgs) -> Result<()> {
    install_panic_hook();

    let mut state = WizardState::new();
    state.output_path = PathBuf::from(args.output);
    if let Some(resume_path) = args.resume.as_deref() {
        match std::fs::read_to_string(resume_path) {
            Ok(toml_str) => match toml::from_str(&toml_str) {
                Ok(cfg) => state.config = cfg,
                Err(e) => eprintln!("warning: --resume parse failed: {}", e),
            },
            Err(e) => eprintln!("warning: --resume read failed: {}", e),
        }
    }

    // Build pages with their UI state synced from `state.config`. Without
    // this, --resume would show resumed values in the live preview but page
    // widgets would default-init, and the next keystroke would clobber the
    // resumed config with the page's defaults.
    let mut pages = build_pages_synced(&state);

    // Drive the event loop within a guard scope, then act on the LoopExit
    // *outside* the guard — that way the terminal is restored before any
    // post-wizard output (save messages, exec, etc.).
    let exit = {
        let _guard = TerminalGuard::enter()?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;
        run_loop_drawing(&mut state, &mut terminal, &mut pages)?
    };

    match exit {
        LoopExit::Quit | LoopExit::EventSourceDrained => Ok(()),
        LoopExit::Save => {
            save::save_to(&state, &state.output_path)?;
            println!("Saved configuration to {}", state.output_path.display());
            println!();
            println!("To run this benchmark:");
            println!("  {}", launch_command(&state));
            Ok(())
        }
        LoopExit::RunNow => {
            // exec replaces the current process — only returns on failure.
            save::exec_iopulse(&state)
        }
    }
}

/// Build the user-facing launch command for the saved config, tailored to the
/// chosen execution mode. Standalone: `iopulse --config <file>`. Coordinator
/// uses --host-list or --clients-file (whichever was set), and adds
/// --worker-port when set. Service uses --listen-port.
fn launch_command(state: &WizardState) -> String {
    use crate::config::cli::ExecutionMode;
    let cfg = state.output_path.display();
    match state.execution_mode {
        ExecutionMode::Standalone => format!("iopulse --config {}", cfg),
        ExecutionMode::Coordinator => {
            let mut parts = vec!["iopulse".to_string(), "--mode".into(), "coordinator".into()];
            if let Some(hosts) = state.mode_detail.as_deref().filter(|s| !s.is_empty()) {
                parts.push("--host-list".into());
                parts.push(hosts.into());
            } else if let Some(file) = state.clients_file.as_deref() {
                parts.push("--clients-file".into());
                parts.push(file.into());
            } else {
                parts.push("--host-list".into());
                parts.push("<hosts>".into());
            }
            if let Some(port) = state.worker_port {
                parts.push("--worker-port".into());
                parts.push(format!("{}", port));
            }
            parts.push("--config".into());
            parts.push(format!("{}", cfg));
            parts.join(" ")
        }
        ExecutionMode::Service => match state.mode_detail.as_deref() {
            Some(port) => format!("iopulse --mode service --listen-port {}", port),
            None => "iopulse --mode service".to_string(),
        },
    }
}
