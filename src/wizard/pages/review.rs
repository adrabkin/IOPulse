//! Review & Save page.
//!
//! Renders the full TOML for the user to confirm. `[s]` saves to
//! `state.output_path`, `[r]` saves to a tempfile and exec's into
//! `iopulse --config`, `[b]` returns to the previous page. The save and
//! exec mechanics live in Phase 5 — this page only routes outcomes.

use crate::config::MultiPhaseConfig;
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

#[derive(Debug)]
pub struct ReviewPage {
    pub page_index: usize,
    /// Vertical scroll offset for the TOML preview.
    scroll: u16,
}

impl ReviewPage {
    pub fn new(page_index: usize) -> Self {
        Self {
            page_index,
            scroll: 0,
        }
    }

    /// Serialize the wizard state to TOML for display. Used by the page render
    /// AND by the live preview pane (Phase 3) — kept here so both call sites
    /// produce byte-identical output.
    pub fn render_toml(state: &WizardState) -> String {
        let result = if let Some(mp) = &state.multi_phase {
            toml::to_string_pretty(&MultiPhaseRef(mp))
        } else {
            toml::to_string_pretty(&state.config)
        };
        result.unwrap_or_else(|e| format!("# (TOML serialization error: {})\n", e))
    }
}

/// Newtype wrapper so we can serialize MultiPhaseConfig without leaking
/// implementation noise into the public API.
struct MultiPhaseRef<'a>(&'a MultiPhaseConfig);

impl serde::Serialize for MultiPhaseRef<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl Page for ReviewPage {
    fn title(&self) -> &str {
        "Review & save"
    }

    fn description(&self) -> &str {
        "Final config. Press 's' to save the TOML, 'r' to save and run the \
        benchmark immediately. Use Esc to step back through pages and tweak \
        anything. Save path is shown in the status bar at the bottom."
    }

    fn keybindings(&self) -> &str {
        "s: save · r: save and run · ↑/↓ or PgUp/PgDn: scroll"
    }

    fn render(&self, frame: &mut Frame, area: Rect, state: &WizardState) {
        let toml = Self::render_toml(state);
        let footer = format!(
            "[s]ave to {}    [r]un now    [b]ack    [q]uit",
            state.output_path.display()
        );
        let body = format!("{}\n\n{}", toml, footer);
        let para = Paragraph::new(body)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0))
            .block(Block::default().title("Review").borders(Borders::ALL));
        frame.render_widget(para, area);
    }

    fn handle_key(&mut self, key: KeyEvent, _state: &mut WizardState) -> PageOutcome {
        match key.code {
            KeyCode::Char('s') => PageOutcome::Save,
            KeyCode::Char('r') => PageOutcome::RunNow,
            KeyCode::Char('b') => PageOutcome::Back,
            KeyCode::Char('q') | KeyCode::Esc => PageOutcome::Quit,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
                PageOutcome::Stay
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = self.scroll.saturating_add(1);
                PageOutcome::Stay
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
                PageOutcome::Stay
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(10);
                PageOutcome::Stay
            }
            _ => PageOutcome::Stay,
        }
    }

    fn validate(&self, _state: &WizardState) -> Vec<ValidationIssue> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_review_renders_workload_section() {
        let state = WizardState::new();
        let toml = ReviewPage::render_toml(&state);
        assert!(toml.contains("[workload]"), "TOML should contain workload section: {}", toml);
        assert!(toml.contains("read_percent = 100"));
    }

    #[test]
    fn test_review_renders_multi_phase_when_enabled() {
        use crate::config::PhaseConfig;
        let mut state = WizardState::new();
        state.multi_phase = Some(MultiPhaseConfig {
            targets: state.config.targets.clone(),
            workers: state.config.workers.clone(),
            output: state.config.output.clone(),
            runtime: state.config.runtime.clone(),
            phases: vec![PhaseConfig {
                name: "phase-1".to_string(),
                workload: state.config.workload.clone(),
                targets: None,
                stonewall: false,
            }],
        });
        let toml = ReviewPage::render_toml(&state);
        assert!(toml.contains("[[phases]]") || toml.contains("phases"), "{}", toml);
    }

    #[test]
    fn test_review_s_returns_save_outcome() {
        let mut page = ReviewPage::new(10);
        let mut state = WizardState::new();
        let outcome = page.handle_key(key(KeyCode::Char('s')), &mut state);
        assert_eq!(outcome, PageOutcome::Save);
    }

    #[test]
    fn test_review_r_returns_run_now() {
        let mut page = ReviewPage::new(10);
        let mut state = WizardState::new();
        let outcome = page.handle_key(key(KeyCode::Char('r')), &mut state);
        assert_eq!(outcome, PageOutcome::RunNow);
    }

    #[test]
    fn test_review_b_returns_back() {
        let mut page = ReviewPage::new(10);
        let mut state = WizardState::new();
        let outcome = page.handle_key(key(KeyCode::Char('b')), &mut state);
        assert_eq!(outcome, PageOutcome::Back);
    }
}
