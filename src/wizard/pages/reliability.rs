//! Reliability page: data verification, error handling, write-conflict opt-in.

use crate::config::workload::VerifyPattern;
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{Severity, ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

const VERIFY_PATTERNS: &[(VerifyPattern, &str)] = &[
    (VerifyPattern::Random, "random"),
    (VerifyPattern::Zeros, "zeros"),
    (VerifyPattern::Ones, "ones"),
    (VerifyPattern::Sequential, "sequential"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Verify,
    VerifyPattern,
    ContinueOnError,
    MaxErrors,
    ContinueOnWorkerFailure,
    AllowWriteConflicts,
}

const FOCUS_ORDER: &[Focus] = &[
    Focus::Verify,
    Focus::VerifyPattern,
    Focus::ContinueOnError,
    Focus::MaxErrors,
    Focus::ContinueOnWorkerFailure,
    Focus::AllowWriteConflicts,
];

#[derive(Debug)]
pub struct ReliabilityPage {
    verify_pattern_idx: usize,
    max_errors: Input,
    pub page_index: usize,
    focus: Focus,
}

impl ReliabilityPage {
    pub fn new(page_index: usize) -> Self {
        Self {
            verify_pattern_idx: 0,
            max_errors: Input::default(),
            page_index,
            focus: Focus::Verify,
        }
    }

    fn next_focus(&self, state: &WizardState) -> Focus {
        let mut i = FOCUS_ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        loop {
            i = (i + 1) % FOCUS_ORDER.len();
            let candidate = FOCUS_ORDER[i];
            if self.is_focusable(candidate, state) {
                return candidate;
            }
        }
    }
    fn prev_focus(&self, state: &WizardState) -> Focus {
        let mut i = FOCUS_ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        loop {
            i = (i + FOCUS_ORDER.len() - 1) % FOCUS_ORDER.len();
            let candidate = FOCUS_ORDER[i];
            if self.is_focusable(candidate, state) {
                return candidate;
            }
        }
    }

    fn is_focusable(&self, focus: Focus, state: &WizardState) -> bool {
        match focus {
            Focus::VerifyPattern => state.config.runtime.verify,
            Focus::MaxErrors => state.config.runtime.continue_on_error,
            _ => true,
        }
    }

    fn flush_to_state(&self, state: &mut WizardState) {
        state.config.runtime.verify_pattern = Some(VERIFY_PATTERNS[self.verify_pattern_idx].0);
        state.config.runtime.max_errors = self.max_errors.value().parse::<usize>().ok();
    }

    fn toggle(&self, focus: Focus, state: &mut WizardState) {
        match focus {
            Focus::Verify => state.config.runtime.verify ^= true,
            Focus::ContinueOnError => state.config.runtime.continue_on_error ^= true,
            Focus::ContinueOnWorkerFailure => {
                state.config.runtime.continue_on_worker_failure ^= true
            }
            Focus::AllowWriteConflicts => state.config.runtime.allow_write_conflicts ^= true,
            _ => {}
        }
    }
}

impl Page for ReliabilityPage {
    fn title(&self) -> &str {
        "Reliability"
    }

    fn description(&self) -> &str {
        "Data integrity + error handling. Verify writes a known pattern then \
        reads it back to confirm correctness — useful for storage stress \
        tests, slows down by ~2x. Continue-on-error keeps running past IO \
        failures (max errors caps the count). Allow-write-conflicts is \
        required for shared-write benchmarks where you don't care about \
        correctness."
    }

    fn keybindings(&self) -> &str {
        "Tab/Enter: next field · Space: toggle · ←/→ on pattern"
    }

    fn render(&self, frame: &mut Frame, area: Rect, state: &WizardState) {
        let verify_on = state.config.runtime.verify;
        let cont_on = state.config.runtime.continue_on_error;

        let mut constraints = vec![
            Constraint::Length(1), // Verify checkbox
        ];
        if verify_on {
            constraints.push(Constraint::Length(1)); // VerifyPattern
        }
        constraints.push(Constraint::Length(1)); // separator
        constraints.push(Constraint::Length(1)); // ContinueOnError
        if cont_on {
            constraints.push(Constraint::Length(3)); // MaxErrors
        }
        constraints.push(Constraint::Length(1)); // ContinueOnWorkerFailure
        constraints.push(Constraint::Length(1)); // separator
        constraints.push(Constraint::Length(1)); // AllowWriteConflicts
        constraints.push(Constraint::Min(0));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        let render_check = |frame: &mut Frame,
                            rect: Rect,
                            focus: Focus,
                            label: &str,
                            value: bool| {
            let mark = if value { "[x]" } else { "[ ]" };
            let style = if self.focus == focus {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            let p = Paragraph::new(format!("  {} {}", mark, label)).style(style);
            frame.render_widget(p, rect);
        };

        let render_cycler = |frame: &mut Frame,
                             rect: Rect,
                             focus: Focus,
                             label: &str,
                             value: &str| {
            let style = if self.focus == focus {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            let p = Paragraph::new(format!("    {} : < {} >", label, value)).style(style);
            frame.render_widget(p, rect);
        };

        let render_text = |frame: &mut Frame,
                           rect: Rect,
                           focus: Focus,
                           label: &str,
                           input: &Input,
                           placeholder: &str| {
            let block = Block::default()
                .title(format!(" {} ", label))
                .borders(Borders::ALL)
                .border_style(if self.focus == focus {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                });
            let value = input.value();
            let display = if value.is_empty() {
                Paragraph::new(placeholder).style(Style::default().fg(Color::DarkGray))
            } else {
                Paragraph::new(value)
            };
            frame.render_widget(display.block(block), rect);
        };

        let mut idx = 0;
        render_check(
            frame,
            chunks[idx],
            Focus::Verify,
            "Verify writes (pattern + readback)",
            verify_on,
        );
        idx += 1;
        if verify_on {
            render_cycler(
                frame,
                chunks[idx],
                Focus::VerifyPattern,
                "Verify pattern",
                VERIFY_PATTERNS[self.verify_pattern_idx].1,
            );
            idx += 1;
        }
        let sep =
            Paragraph::new("── Error handling ──").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(sep, chunks[idx]);
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::ContinueOnError,
            "Continue on IO errors instead of aborting",
            cont_on,
        );
        idx += 1;
        if cont_on {
            render_text(
                frame,
                chunks[idx],
                Focus::MaxErrors,
                "Max errors before abort (optional)",
                &self.max_errors,
                "(blank = no cap) e.g. 10",
            );
            idx += 1;
        }
        render_check(
            frame,
            chunks[idx],
            Focus::ContinueOnWorkerFailure,
            "Continue on worker failure (distributed mode)",
            state.config.runtime.continue_on_worker_failure,
        );
        idx += 1;
        let sep2 =
            Paragraph::new("── Shared-write conflicts ──").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(sep2, chunks[idx]);
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::AllowWriteConflicts,
            "Allow write conflicts (BENCHMARK MODE — may corrupt data)",
            state.config.runtime.allow_write_conflicts,
        );
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut WizardState) -> PageOutcome {
        match key.code {
            KeyCode::Tab | KeyCode::Down => {
                self.focus = self.next_focus(state);
                PageOutcome::Stay
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.focus = self.prev_focus(state);
                PageOutcome::Stay
            }
            KeyCode::Char(' ')
                if matches!(
                    self.focus,
                    Focus::Verify
                        | Focus::ContinueOnError
                        | Focus::ContinueOnWorkerFailure
                        | Focus::AllowWriteConflicts
                ) =>
            {
                self.toggle(self.focus, state);
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Left | KeyCode::Right if self.focus == Focus::VerifyPattern => {
                if key.code == KeyCode::Right {
                    self.verify_pattern_idx =
                        (self.verify_pattern_idx + 1) % VERIFY_PATTERNS.len();
                } else {
                    self.verify_pattern_idx =
                        (self.verify_pattern_idx + VERIFY_PATTERNS.len() - 1)
                            % VERIFY_PATTERNS.len();
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Enter => {
                self.flush_to_state(state);
                state.dirty = true;
                if self.focus == Focus::AllowWriteConflicts {
                    PageOutcome::Next
                } else {
                    self.focus = self.next_focus(state);
                    PageOutcome::Stay
                }
            }
            KeyCode::Esc => PageOutcome::Quit,
            _ if self.focus == Focus::MaxErrors => {
                self.max_errors
                    .handle_event(&crossterm::event::Event::Key(key));
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            _ => PageOutcome::Stay,
        }
    }

    fn sync_from_state(&mut self, state: &WizardState) {
        if let Some(p) = state.config.runtime.verify_pattern {
            self.verify_pattern_idx =
                VERIFY_PATTERNS.iter().position(|(v, _)| *v == p).unwrap_or(0);
        }
        if let Some(n) = state.config.runtime.max_errors {
            self.max_errors = Input::default().with_value(format!("{}", n));
        }
    }

    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if state.config.runtime.continue_on_error {
            let m = self.max_errors.value();
            if !m.is_empty() && m.parse::<usize>().ok().filter(|n| *n > 0).is_none() {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    page: self.page_index,
                    message: format!("Max errors must be a positive integer (got '{}')", m),
                });
            }
        }
        issues
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
    fn test_reliability_default_no_verify() {
        let page = ReliabilityPage::new(9);
        let state = WizardState::new();
        assert!(!state.config.runtime.verify);
        assert_eq!(page.focus, Focus::Verify);
    }

    #[test]
    fn test_reliability_toggle_verify_reveals_pattern() {
        let mut page = ReliabilityPage::new(9);
        let mut state = WizardState::new();
        assert!(!page.is_focusable(Focus::VerifyPattern, &state));
        page.handle_key(key(KeyCode::Char(' ')), &mut state);
        assert!(state.config.runtime.verify);
        assert!(page.is_focusable(Focus::VerifyPattern, &state));
    }

    #[test]
    fn test_reliability_continue_on_error_reveals_max_errors() {
        let mut page = ReliabilityPage::new(9);
        let mut state = WizardState::new();
        // Tab past Verify (and skip VerifyPattern when off) → ContinueOnError.
        page.handle_key(key(KeyCode::Tab), &mut state);
        assert_eq!(page.focus, Focus::ContinueOnError);
        page.handle_key(key(KeyCode::Char(' ')), &mut state);
        assert!(state.config.runtime.continue_on_error);
        assert!(page.is_focusable(Focus::MaxErrors, &state));
    }

    #[test]
    fn test_reliability_allow_write_conflicts() {
        let mut page = ReliabilityPage::new(9);
        let mut state = WizardState::new();
        // Tab through: Verify → ContinueOnError → ContinueOnWorkerFailure → AllowWriteConflicts.
        // VerifyPattern is skipped while verify is off; MaxErrors while continue_on_error is off.
        page.handle_key(key(KeyCode::Tab), &mut state);
        page.handle_key(key(KeyCode::Tab), &mut state);
        page.handle_key(key(KeyCode::Tab), &mut state);
        assert_eq!(page.focus, Focus::AllowWriteConflicts);
        page.handle_key(key(KeyCode::Char(' ')), &mut state);
        assert!(state.config.runtime.allow_write_conflicts);
    }

    #[test]
    fn test_reliability_continue_on_worker_failure() {
        let mut page = ReliabilityPage::new(9);
        let mut state = WizardState::new();
        // Verify → ContinueOnError → ContinueOnWorkerFailure
        page.handle_key(key(KeyCode::Tab), &mut state);
        page.handle_key(key(KeyCode::Tab), &mut state);
        assert_eq!(page.focus, Focus::ContinueOnWorkerFailure);
        page.handle_key(key(KeyCode::Char(' ')), &mut state);
        assert!(state.config.runtime.continue_on_worker_failure);
    }
}
