//! Workload page: read% / write% mix + sequential vs random access.

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
enum Focus {
    Read,
    Write,
    Pattern,
}

const FOCUS_ORDER: &[Focus] = &[Focus::Read, Focus::Write, Focus::Pattern];

const PATTERNS: &[(bool, &str, &str)] = &[
    (false, "Sequential", "blocks accessed in order — streaming workloads"),
    (true, "Random", "offsets picked by the distribution — OLTP, key-value"),
];

#[derive(Debug)]
pub struct WorkloadPage {
    read: Input,
    write: Input,
    /// 0 = Sequential, 1 = Random.
    pattern_selected: usize,
    focus: Focus,
    pub page_index: usize,
}

impl WorkloadPage {
    pub fn new(page_index: usize) -> Self {
        Self {
            read: Input::default().with_value("100".to_string()),
            write: Input::default().with_value("0".to_string()),
            pattern_selected: 0,
            focus: Focus::Read,
            page_index,
        }
    }

    fn next_focus(&self) -> Focus {
        let i = FOCUS_ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        FOCUS_ORDER[(i + 1) % FOCUS_ORDER.len()]
    }
    fn prev_focus(&self) -> Focus {
        let i = FOCUS_ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        FOCUS_ORDER[(i + FOCUS_ORDER.len() - 1) % FOCUS_ORDER.len()]
    }

    fn flush_to_state(&self, state: &mut WizardState) {
        if let Ok(r) = self.read.value().parse::<u8>() {
            state.config.workload.read_percent = r;
        }
        if let Ok(w) = self.write.value().parse::<u8>() {
            state.config.workload.write_percent = w;
        }
        state.config.workload.random = PATTERNS[self.pattern_selected].0;
    }
}

impl Page for WorkloadPage {
    fn title(&self) -> &str {
        "Read / Write mix"
    }

    fn description(&self) -> &str {
        "Read/write proportion (must sum to 100) and access pattern. \
        Common mixes: 100/0 = read-only, 0/100 = write-only, 70/30 = OLTP. \
        Sequential access streams blocks in order; random access uses the \
        distribution you'll choose on the next step."
    }

    fn keybindings(&self) -> &str {
        "Tab/Enter: next field · ↑/↓ on pattern: pick"
    }

    fn render(&self, frame: &mut Frame, area: Rect, _state: &WizardState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Read %
                Constraint::Length(3), // Write %
                Constraint::Min(4),    // Access pattern list
            ])
            .split(area);

        let read_block = Block::default()
            .title(" Read % ")
            .borders(Borders::ALL)
            .border_style(if self.focus == Focus::Read {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            });
        frame.render_widget(
            Paragraph::new(self.read.value()).block(read_block),
            chunks[0],
        );

        let write_block = Block::default()
            .title(" Write % ")
            .borders(Borders::ALL)
            .border_style(if self.focus == Focus::Write {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            });
        frame.render_widget(
            Paragraph::new(self.write.value()).block(write_block),
            chunks[1],
        );

        let items: Vec<ListItem> = PATTERNS
            .iter()
            .map(|(_, name, desc)| ListItem::new(format!("{:11} — {}", name, desc)))
            .collect();
        let pattern_focus = self.focus == Focus::Pattern;
        let list = List::new(items)
            .block(
                Block::default()
                    .title(if pattern_focus {
                        " Access pattern (focused) "
                    } else {
                        " Access pattern "
                    })
                    .borders(Borders::ALL)
                    .border_style(if pattern_focus {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default()
                    }),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut list_state = ListState::default();
        list_state.select(Some(self.pattern_selected));
        frame.render_stateful_widget(list, chunks[2], &mut list_state);
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut WizardState) -> PageOutcome {
        match (self.focus, key.code) {
            (_, KeyCode::Tab) => {
                self.focus = self.next_focus();
                PageOutcome::Stay
            }
            (_, KeyCode::BackTab) => {
                self.focus = self.prev_focus();
                PageOutcome::Stay
            }
            (Focus::Pattern, KeyCode::Up) => {
                if self.pattern_selected > 0 {
                    self.pattern_selected -= 1;
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::Pattern, KeyCode::Down) => {
                if self.pattern_selected + 1 < PATTERNS.len() {
                    self.pattern_selected += 1;
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            // On the text fields, Up/Down switch fields (matches the rest of
            // the wizard).
            (Focus::Read | Focus::Write, KeyCode::Up | KeyCode::Down) => {
                self.focus = match (self.focus, key.code) {
                    (Focus::Read, KeyCode::Down) => Focus::Write,
                    (Focus::Write, KeyCode::Down) => Focus::Pattern,
                    (Focus::Read, KeyCode::Up) => Focus::Pattern,
                    (Focus::Write, KeyCode::Up) => Focus::Read,
                    _ => self.focus,
                };
                PageOutcome::Stay
            }
            (_, KeyCode::Enter) => {
                self.flush_to_state(state);
                state.dirty = true;
                match self.focus {
                    Focus::Read => {
                        self.focus = Focus::Write;
                        PageOutcome::Stay
                    }
                    Focus::Write => {
                        self.focus = Focus::Pattern;
                        PageOutcome::Stay
                    }
                    Focus::Pattern => PageOutcome::Next,
                }
            }
            (_, KeyCode::Esc) => PageOutcome::Quit,
            (Focus::Read | Focus::Write, _) => {
                let target = match self.focus {
                    Focus::Read => &mut self.read,
                    Focus::Write => &mut self.write,
                    _ => unreachable!(),
                };
                target.handle_event(&crossterm::event::Event::Key(key));
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::Pattern, _) => PageOutcome::Stay,
        }
    }

    fn sync_from_state(&mut self, state: &WizardState) {
        self.read = Input::default().with_value(format!("{}", state.config.workload.read_percent));
        self.write =
            Input::default().with_value(format!("{}", state.config.workload.write_percent));
        self.pattern_selected = if state.config.workload.random { 1 } else { 0 };
    }

    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let r = state.config.workload.read_percent;
        let w = state.config.workload.write_percent;
        if r as u16 + w as u16 != 100 {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!(
                    "Read + Write percents must sum to 100 (got {} + {} = {})",
                    r,
                    w,
                    r as u16 + w as u16
                ),
            });
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

    fn type_string(page: &mut WorkloadPage, state: &mut WizardState, s: &str) {
        for ch in s.chars() {
            page.handle_key(key(KeyCode::Char(ch)), state);
        }
    }

    fn clear_input(page: &mut WorkloadPage, state: &mut WizardState) {
        for _ in 0..6 {
            page.handle_key(key(KeyCode::Backspace), state);
        }
    }

    #[test]
    fn test_workload_default_is_100_0_sequential() {
        let page = WorkloadPage::new(3);
        let state = WizardState::new();
        assert_eq!(page.read.value(), "100");
        assert_eq!(page.write.value(), "0");
        assert_eq!(page.pattern_selected, 0);
        assert!(!state.config.workload.random);
    }

    #[test]
    fn test_workload_70_30_split() {
        let mut page = WorkloadPage::new(3);
        let mut state = WizardState::new();
        clear_input(&mut page, &mut state);
        type_string(&mut page, &mut state, "70");
        page.handle_key(key(KeyCode::Tab), &mut state);
        clear_input(&mut page, &mut state);
        type_string(&mut page, &mut state, "30");
        assert_eq!(state.config.workload.read_percent, 70);
        assert_eq!(state.config.workload.write_percent, 30);
        assert!(page.validate(&state).is_empty());
    }

    #[test]
    fn test_workload_validates_sum_not_100() {
        let mut page = WorkloadPage::new(3);
        let mut state = WizardState::new();
        clear_input(&mut page, &mut state);
        type_string(&mut page, &mut state, "60");
        page.handle_key(key(KeyCode::Tab), &mut state);
        clear_input(&mut page, &mut state);
        type_string(&mut page, &mut state, "30");
        let issues = page.validate(&state);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("100"));
    }

    #[test]
    fn test_workload_pattern_random_selection() {
        let mut page = WorkloadPage::new(3);
        let mut state = WizardState::new();
        // Tab to Pattern field.
        page.handle_key(key(KeyCode::Tab), &mut state);
        page.handle_key(key(KeyCode::Tab), &mut state);
        assert_eq!(page.focus, Focus::Pattern);
        assert!(!state.config.workload.random);
        page.handle_key(key(KeyCode::Down), &mut state);
        assert!(state.config.workload.random);
        page.handle_key(key(KeyCode::Up), &mut state);
        assert!(!state.config.workload.random);
    }

    #[test]
    fn test_workload_enter_walks_three_fields() {
        let mut page = WorkloadPage::new(3);
        let mut state = WizardState::new();
        // Enter on Read → Write
        let outcome = page.handle_key(key(KeyCode::Enter), &mut state);
        assert_eq!(outcome, PageOutcome::Stay);
        assert_eq!(page.focus, Focus::Write);
        // Enter on Write → Pattern
        let outcome = page.handle_key(key(KeyCode::Enter), &mut state);
        assert_eq!(outcome, PageOutcome::Stay);
        assert_eq!(page.focus, Focus::Pattern);
        // Enter on Pattern → Next
        let outcome = page.handle_key(key(KeyCode::Enter), &mut state);
        assert_eq!(outcome, PageOutcome::Next);
    }
}
