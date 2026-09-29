//! Completion mode page.
//!
//! Mutually exclusive choice between Duration / Total bytes / Run until
//! complete. The relevant value field is revealed based on the selection.

use crate::config::cli_convert::{parse_duration, parse_size};
use crate::config::workload::CompletionMode;
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{Severity, ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Duration,
    TotalBytes,
    RunUntilComplete,
}

const KINDS: &[(Kind, &str)] = &[
    (Kind::Duration, "Duration (e.g. 60s, 5m)"),
    (Kind::TotalBytes, "Total bytes (e.g. 10G, 1T)"),
    (Kind::RunUntilComplete, "Run until file complete"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Value,
}

#[derive(Debug)]
pub struct CompletionPage {
    selected: usize,
    value: Input,
    focus: Focus,
    pub page_index: usize,
}

impl CompletionPage {
    pub fn new(page_index: usize) -> Self {
        Self {
            selected: 0,
            value: Input::default().with_value("10s".to_string()),
            focus: Focus::List,
            page_index,
        }
    }

    fn current_kind(&self) -> Kind {
        KINDS[self.selected].0
    }

    fn flush_to_state(&self, state: &mut WizardState) {
        state.config.workload.completion_mode = match self.current_kind() {
            Kind::Duration => CompletionMode::Duration {
                seconds: parse_duration(self.value.value()).unwrap_or(10),
            },
            Kind::TotalBytes => CompletionMode::TotalBytes {
                bytes: parse_size(self.value.value()).unwrap_or(0),
            },
            Kind::RunUntilComplete => CompletionMode::RunUntilComplete,
        };
    }
}

impl Page for CompletionPage {
    fn title(&self) -> &str {
        "When to stop"
    }

    fn description(&self) -> &str {
        "How the benchmark decides it's done. \
        Duration = run for a fixed wall-clock time (most common: 30s, 1m, 5m). \
        Total bytes = stop after transferring N bytes (e.g. 100G). \
        Run until file complete = stop when every block in the file has been \
        touched (good for full-file scans)."
    }

    fn keybindings(&self) -> &str {
        "↑/↓: pick · Tab: edit value · Enter: next"
    }

    fn render(&self, frame: &mut Frame, area: Rect, _state: &WizardState) {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let items: Vec<ListItem> = KINDS
            .iter()
            .map(|(_, label)| ListItem::new(*label))
            .collect();
        let list_focus = self.focus == Focus::List;
        let list_block = Block::default()
            .title(if list_focus { " Mode (focused) " } else { " Mode " })
            .borders(Borders::ALL)
            .border_style(if list_focus {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            });
        let list = List::new(items)
            .block(list_block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut list_state = ListState::default();
        list_state.select(Some(self.selected));
        frame.render_stateful_widget(list, panes[0], &mut list_state);

        let value_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(panes[1]);

        if self.current_kind() != Kind::RunUntilComplete {
            let label = match self.current_kind() {
                Kind::Duration => " Duration (e.g. 30s, 5m, 1h) ",
                Kind::TotalBytes => " Total bytes (e.g. 10G, 1T) ",
                Kind::RunUntilComplete => unreachable!(),
            };
            let value_block = Block::default()
                .title(label)
                .borders(Borders::ALL)
                .border_style(if self.focus == Focus::Value {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                });
            frame.render_widget(
                Paragraph::new(self.value.value()).block(value_block),
                value_chunks[0],
            );
        } else {
            let info = Paragraph::new(
                "No additional input required.\nThe benchmark stops when every block has been touched.",
            )
            .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(info, value_chunks[0]);
        }
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut WizardState) -> PageOutcome {
        match (self.focus, key.code) {
            (Focus::List, KeyCode::Up | KeyCode::Char('k')) => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::List, KeyCode::Down | KeyCode::Char('j')) => {
                if self.selected + 1 < KINDS.len() {
                    self.selected += 1;
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (_, KeyCode::Tab) => {
                self.focus = match (self.focus, self.current_kind()) {
                    (Focus::List, Kind::RunUntilComplete) => Focus::List,
                    (Focus::List, _) => Focus::Value,
                    (Focus::Value, _) => Focus::List,
                };
                PageOutcome::Stay
            }
            (_, KeyCode::Enter) => {
                self.flush_to_state(state);
                state.dirty = true;
                // RunUntilComplete has no value field — advance directly.
                // Otherwise: Enter on list → focus the value field; Enter on
                // value field → advance to next page.
                match (self.focus, self.current_kind()) {
                    (_, Kind::RunUntilComplete) => PageOutcome::Next,
                    (Focus::List, _) => {
                        self.focus = Focus::Value;
                        PageOutcome::Stay
                    }
                    (Focus::Value, _) => PageOutcome::Next,
                }
            }
            (_, KeyCode::Esc) => PageOutcome::Quit,
            (Focus::Value, _) => {
                self.value
                    .handle_event(&crossterm::event::Event::Key(key));
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::List, _) => PageOutcome::Stay,
        }
    }

    fn sync_from_state(&mut self, state: &WizardState) {
        match &state.config.workload.completion_mode {
            CompletionMode::Duration { seconds } => {
                self.selected = 0;
                self.value = Input::default().with_value(format!("{}s", seconds));
            }
            CompletionMode::TotalBytes { bytes } => {
                self.selected = 1;
                self.value = Input::default().with_value(format!("{}", bytes));
            }
            CompletionMode::RunUntilComplete => {
                self.selected = 2;
            }
        }
    }

    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        match &state.config.workload.completion_mode {
            CompletionMode::Duration { seconds } if *seconds == 0 => {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    page: self.page_index,
                    message: "Duration must be > 0".to_string(),
                });
            }
            CompletionMode::TotalBytes { bytes } if *bytes == 0 => {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    page: self.page_index,
                    message: "Total bytes must be > 0".to_string(),
                });
            }
            _ => {}
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
    fn test_completion_default_is_duration_10s() {
        let page = CompletionPage::new(6);
        assert_eq!(page.current_kind(), Kind::Duration);
    }

    #[test]
    fn test_completion_select_total_bytes() {
        let mut page = CompletionPage::new(6);
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Down), &mut state);
        match state.config.workload.completion_mode {
            CompletionMode::TotalBytes { .. } => {}
            other => panic!("expected TotalBytes, got {:?}", other),
        }
    }

    #[test]
    fn test_completion_run_until_complete() {
        let mut page = CompletionPage::new(6);
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Down), &mut state);
        page.handle_key(key(KeyCode::Down), &mut state);
        match state.config.workload.completion_mode {
            CompletionMode::RunUntilComplete => {}
            other => panic!("expected RunUntilComplete, got {:?}", other),
        }
    }
}
