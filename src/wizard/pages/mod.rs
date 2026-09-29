//! Wizard pages.
//!
//! Each page implements the `Page` trait and is responsible for rendering itself,
//! handling input, and reporting validation issues for its own concern. The wizard's
//! event loop dispatches key events to the current page and acts on the returned
//! `PageOutcome` (advance, go back, save, etc.).

pub mod advanced;
pub mod block_queue;
pub mod completion;
pub mod distribution;
pub mod engine;
pub mod mode;
pub mod output;
pub mod phases;
pub mod reliability;
pub mod review;
pub mod target;
pub mod workers;
pub mod workload;

use crate::wizard::state::{ValidationIssue, WizardState};
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

/// Boxed list of every wizard page in display order. Indices here are the
/// canonical `current_page` values.
pub fn build_all_pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(mode::ModePage::new()),
        Box::new(target::TargetPage::new(1)),
        Box::new(engine::EnginePage::new(2)),
        Box::new(workload::WorkloadPage::new(3)),
        Box::new(distribution::DistributionPage::new(4)),
        Box::new(advanced::AdvancedPage::new(5)),
        Box::new(block_queue::BlockQueuePage::new(6)),
        Box::new(completion::CompletionPage::new(7)),
        Box::new(workers::WorkersPage::new(8)),
        Box::new(reliability::ReliabilityPage::new(9)),
        Box::new(output::OutputPage::new(10)),
        Box::new(phases::PhasesPage::new(11)),
        Box::new(review::ReviewPage::new(12)),
    ]
}

/// Build the page list AND sync each page's UI state from the resumed
/// config. Without this step, pages with their own widget state (text inputs,
/// list cursors) would show defaults even though the config has values —
/// the next keystroke would then overwrite the resumed config with the
/// page's defaults. Used after `wizard --resume <path>`.
pub fn build_pages_synced(state: &WizardState) -> Vec<Box<dyn Page>> {
    let mut pages = build_all_pages();
    for page in pages.iter_mut() {
        page.sync_from_state(state);
    }
    pages
}

/// Number of wizard pages. Single source of truth for boundary checks.
pub const PAGE_COUNT: usize = 13;

/// Side-effect a page asks the wizard to perform after `handle_key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageOutcome {
    /// Stay on this page; nothing to do.
    Stay,
    /// Advance to the next page.
    Next,
    /// Go back to the previous page.
    Back,
    /// Quit the wizard without saving.
    Quit,
    /// Save the current config to `state.output_path`.
    Save,
    /// Save to a tempfile and exec into `iopulse --config`.
    RunNow,
}

/// One wizard page. Pages own their input state (text inputs, list selections, etc.)
/// but mutate the shared `WizardState` to record decisions.
pub trait Page {
    /// Display title shown in the page header.
    fn title(&self) -> &str;

    /// Explanatory text shown above the fields — tells the user what this
    /// step is asking and why it matters. Multi-line is fine; word-wrap is
    /// applied at render time.
    fn description(&self) -> &str {
        ""
    }

    /// Footer hint listing the keybindings active on this page. Combined
    /// with the global hints (Esc back, Ctrl+C quit) by the event loop.
    fn keybindings(&self) -> &str {
        "Tab: next field · Enter: next page"
    }

    /// Render the page's interactive widgets into `area`. The header,
    /// description, and footer are rendered by the event loop.
    fn render(&self, frame: &mut Frame, area: Rect, state: &WizardState);

    /// Handle a key event. May mutate either the page's own state or `state`.
    /// Pages should NOT handle Esc or Ctrl+C — those are caught globally.
    fn handle_key(&mut self, key: KeyEvent, state: &mut WizardState) -> PageOutcome;

    /// Compute validation issues this page is responsible for. Run on every dirty pass.
    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue>;

    /// Pull page-local UI state (text inputs, list cursors) out of `state.config`.
    /// Called once after `--resume` so the wizard reflects the resumed values
    /// instead of the page's own defaults. Default no-op for pages without
    /// any UI state to sync.
    fn sync_from_state(&mut self, _state: &WizardState) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wizard::state::{Severity, ValidationIssue, WizardState};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Minimal page that records every key it receives, advances on Enter, and
    /// reports a Warning when its `should_warn` flag is set.
    struct MockPage {
        keys_received: Vec<KeyCode>,
        should_warn: bool,
    }

    impl MockPage {
        fn new() -> Self {
            Self {
                keys_received: vec![],
                should_warn: false,
            }
        }
    }

    impl Page for MockPage {
        fn title(&self) -> &str {
            "Mock"
        }

        fn render(&self, _frame: &mut Frame, _area: Rect, _state: &WizardState) {}

        fn handle_key(&mut self, key: KeyEvent, state: &mut WizardState) -> PageOutcome {
            self.keys_received.push(key.code);
            match key.code {
                KeyCode::Enter => {
                    state.dirty = true;
                    PageOutcome::Next
                }
                KeyCode::Esc => PageOutcome::Quit,
                _ => PageOutcome::Stay,
            }
        }

        fn validate(&self, _state: &WizardState) -> Vec<ValidationIssue> {
            if self.should_warn {
                vec![ValidationIssue {
                    severity: Severity::Warning,
                    page: 0,
                    message: "mock warning".to_string(),
                }]
            } else {
                vec![]
            }
        }
    }

    #[test]
    fn test_page_trait_handle_key_records_and_returns_outcome() {
        let mut page = MockPage::new();
        let mut state = WizardState::new();

        let outcome = page.handle_key(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            &mut state,
        );
        assert_eq!(outcome, PageOutcome::Stay);
        assert!(!state.dirty);

        let outcome = page.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut state,
        );
        assert_eq!(outcome, PageOutcome::Next);
        assert!(state.dirty, "Enter should mark state dirty");

        let outcome = page.handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut state,
        );
        assert_eq!(outcome, PageOutcome::Quit);

        assert_eq!(
            page.keys_received,
            vec![KeyCode::Char('a'), KeyCode::Enter, KeyCode::Esc]
        );
    }

    #[test]
    fn test_page_trait_validate_reports_issues() {
        let mut page = MockPage::new();
        let state = WizardState::new();
        assert!(page.validate(&state).is_empty());

        page.should_warn = true;
        let issues = page.validate(&state);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert_eq!(issues[0].message, "mock warning");
    }

    #[test]
    fn test_page_trait_render_does_not_panic() {
        let page = MockPage::new();
        let state = WizardState::new();
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal init");
        terminal
            .draw(|frame| {
                let area = frame.area();
                page.render(frame, area, &state);
            })
            .expect("render does not panic");
    }
}
