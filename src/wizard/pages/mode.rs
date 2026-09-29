//! Mode page: standalone / coordinator / service.

use crate::config::cli::ExecutionMode;
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{Severity, ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

const MODES: &[(ExecutionMode, &str, &str)] = &[
    (
        ExecutionMode::Standalone,
        "Standalone",
        "single machine — runs the benchmark locally",
    ),
    (
        ExecutionMode::Coordinator,
        "Coordinator",
        "this host orchestrates a distributed run across many workers",
    ),
    (
        ExecutionMode::Service,
        "Service",
        "this host is a worker that a coordinator will drive",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    /// Coordinator: host list. Service: listen port.
    Detail,
    /// Coordinator-only: optional file with hosts (one per line).
    ClientsFile,
    /// Coordinator-only: default port to dial workers on.
    WorkerPort,
}

const COORDINATOR_FOCUS: &[Focus] = &[
    Focus::List,
    Focus::Detail,
    Focus::ClientsFile,
    Focus::WorkerPort,
];

#[derive(Debug)]
pub struct ModePage {
    selected: usize,
    detail: Input,
    clients_file: Input,
    worker_port: Input,
    focus: Focus,
}

impl ModePage {
    pub fn new() -> Self {
        Self {
            selected: 0,
            detail: Input::default(),
            clients_file: Input::default(),
            worker_port: Input::default(),
            focus: Focus::List,
        }
    }

    pub fn selected_mode(&self) -> ExecutionMode {
        MODES[self.selected].0
    }

    /// Build the focus order based on the current mode.
    fn focus_order(&self) -> &'static [Focus] {
        match self.selected_mode() {
            ExecutionMode::Standalone => &[Focus::List],
            ExecutionMode::Coordinator => COORDINATOR_FOCUS,
            ExecutionMode::Service => &[Focus::List, Focus::Detail],
        }
    }

    fn next_focus(&self) -> Focus {
        let order = self.focus_order();
        let i = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        order[(i + 1) % order.len()]
    }
    fn prev_focus(&self) -> Focus {
        let order = self.focus_order();
        let i = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        order[(i + order.len() - 1) % order.len()]
    }

    fn flush_to_state(&self, state: &mut WizardState) {
        state.execution_mode = self.selected_mode();
        state.mode_detail = if self.detail.value().is_empty() {
            None
        } else {
            Some(self.detail.value().to_string())
        };
        state.clients_file = if self.clients_file.value().is_empty() {
            None
        } else {
            Some(self.clients_file.value().to_string())
        };
        state.worker_port = self.worker_port.value().parse::<u16>().ok();
    }

    fn detail_label(&self) -> &'static str {
        match self.selected_mode() {
            ExecutionMode::Standalone => "(no extra info needed)",
            ExecutionMode::Coordinator => "Worker hosts (comma-separated host:port)",
            ExecutionMode::Service => "Listen port (default 9999)",
        }
    }

    fn detail_placeholder(&self) -> &'static str {
        match self.selected_mode() {
            ExecutionMode::Standalone => "",
            ExecutionMode::Coordinator => "e.g. host1:9999,host2:9999,host3:9999",
            ExecutionMode::Service => "e.g. 9999",
        }
    }
}

impl Default for ModePage {
    fn default() -> Self {
        Self::new()
    }
}

impl Page for ModePage {
    fn title(&self) -> &str {
        "Mode"
    }

    fn description(&self) -> &str {
        "Standalone runs locally. Coordinator drives a distributed run across \
        worker hosts (each running IOPulse in Service mode). Service mode is \
        for hosts that act as workers. Coordinator extras: clients file is \
        an alternative to the inline host list (one host per line); worker \
        port is the default dial port for hosts in the list that omit it."
    }

    fn keybindings(&self) -> &str {
        "↑/↓: pick mode · Tab/Enter: next field"
    }

    fn render(&self, frame: &mut Frame, area: Rect, _state: &WizardState) {
        let mode = self.selected_mode();
        let mut constraints = vec![Constraint::Min(6), Constraint::Length(3)];
        if mode == ExecutionMode::Coordinator {
            constraints.push(Constraint::Length(3)); // ClientsFile
            constraints.push(Constraint::Length(3)); // WorkerPort
        }
        constraints.push(Constraint::Min(0));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        let items: Vec<ListItem> = MODES
            .iter()
            .map(|(_, name, desc)| ListItem::new(format!("{:13} — {}", name, desc)))
            .collect();
        let list_focus = self.focus == Focus::List;
        let list = List::new(items)
            .block(
                Block::default()
                    .title(if list_focus { " Mode (focused) " } else { " Mode " })
                    .borders(Borders::ALL)
                    .border_style(if list_focus {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default()
                    }),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut list_state = ListState::default();
        list_state.select(Some(self.selected));
        frame.render_stateful_widget(list, chunks[0], &mut list_state);

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

        // Detail (host list / listen port) — visible for Coordinator/Service.
        if mode != ExecutionMode::Standalone {
            render_text(
                frame,
                chunks[1],
                Focus::Detail,
                self.detail_label(),
                &self.detail,
                self.detail_placeholder(),
            );
        } else {
            let info = Paragraph::new("Standalone mode runs locally — no further input needed.")
                .style(Style::default().fg(Color::DarkGray))
                .wrap(Wrap { trim: true });
            frame.render_widget(info, chunks[1]);
        }

        // Coordinator extras: clients file + worker port.
        if mode == ExecutionMode::Coordinator {
            render_text(
                frame,
                chunks[2],
                Focus::ClientsFile,
                "Clients file (optional, alternative to host list)",
                &self.clients_file,
                "(blank = use host list above) e.g. /etc/iopulse/hosts.txt",
            );
            render_text(
                frame,
                chunks[3],
                Focus::WorkerPort,
                "Default worker port (optional)",
                &self.worker_port,
                "(blank = 9999) e.g. 9999",
            );
        }
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut WizardState) -> PageOutcome {
        match (self.focus, key.code) {
            (Focus::List, KeyCode::Up) => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::List, KeyCode::Down) => {
                if self.selected + 1 < MODES.len() {
                    self.selected += 1;
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (_, KeyCode::Tab) => {
                self.focus = self.next_focus();
                PageOutcome::Stay
            }
            (_, KeyCode::BackTab) => {
                self.focus = self.prev_focus();
                PageOutcome::Stay
            }
            (_, KeyCode::Enter) => {
                self.flush_to_state(state);
                state.dirty = true;
                let order = self.focus_order();
                let last = *order.last().unwrap();
                if self.focus == last {
                    PageOutcome::Next
                } else {
                    self.focus = self.next_focus();
                    PageOutcome::Stay
                }
            }
            (_, KeyCode::Esc) => PageOutcome::Quit,
            (Focus::Detail, _) => {
                self.detail
                    .handle_event(&crossterm::event::Event::Key(key));
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::ClientsFile, _) => {
                self.clients_file
                    .handle_event(&crossterm::event::Event::Key(key));
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::WorkerPort, _) => {
                self.worker_port
                    .handle_event(&crossterm::event::Event::Key(key));
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::List, _) => PageOutcome::Stay,
        }
    }

    fn sync_from_state(&mut self, state: &WizardState) {
        self.selected = MODES
            .iter()
            .position(|(m, _, _)| *m == state.execution_mode)
            .unwrap_or(0);
        if let Some(d) = &state.mode_detail {
            self.detail = Input::default().with_value(d.clone());
        }
        if let Some(f) = &state.clients_file {
            self.clients_file = Input::default().with_value(f.clone());
        }
        if let Some(p) = state.worker_port {
            self.worker_port = Input::default().with_value(format!("{}", p));
        }
    }

    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        match state.execution_mode {
            ExecutionMode::Standalone => {}
            ExecutionMode::Coordinator => {
                let host_list = self.detail.value();
                let cfile = self.clients_file.value();
                if host_list.is_empty() && cfile.is_empty() {
                    issues.push(ValidationIssue {
                        severity: Severity::Error,
                        page: 0,
                        message: "Coordinator needs either a host list or a clients file"
                            .to_string(),
                    });
                }
                let wp = self.worker_port.value();
                if !wp.is_empty() && wp.parse::<u16>().ok().filter(|n| *n > 0).is_none() {
                    issues.push(ValidationIssue {
                        severity: Severity::Error,
                        page: 0,
                        message: format!("Worker port must be 1-65535 (got '{}')", wp),
                    });
                }
            }
            ExecutionMode::Service => {
                let v = self.detail.value();
                if !v.is_empty() && v.parse::<u16>().is_err() {
                    issues.push(ValidationIssue {
                        severity: Severity::Error,
                        page: 0,
                        message: format!("Listen port must be a number 1-65535 (got '{}')", v),
                    });
                }
            }
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_string(page: &mut ModePage, state: &mut WizardState, s: &str) {
        for ch in s.chars() {
            page.handle_key(key(KeyCode::Char(ch)), state);
        }
    }

    #[test]
    fn test_mode_default_is_standalone() {
        let page = ModePage::new();
        assert_eq!(page.selected_mode(), ExecutionMode::Standalone);
    }

    #[test]
    fn test_mode_down_selects_coordinator() {
        let mut page = ModePage::new();
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Down), &mut state);
        assert_eq!(state.execution_mode, ExecutionMode::Coordinator);
    }

    #[test]
    fn test_mode_standalone_enter_advances_directly() {
        let mut page = ModePage::new();
        let mut state = WizardState::new();
        let outcome = page.handle_key(key(KeyCode::Enter), &mut state);
        assert_eq!(outcome, PageOutcome::Next);
    }

    #[test]
    fn test_mode_coordinator_walks_4_fields() {
        let mut page = ModePage::new();
        let mut state = WizardState::new();
        // Pick coordinator
        page.handle_key(key(KeyCode::Down), &mut state);
        // Enter walks: List → Detail → ClientsFile → WorkerPort → Next
        page.handle_key(key(KeyCode::Enter), &mut state);
        assert_eq!(page.focus, Focus::Detail);
        type_string(&mut page, &mut state, "host1:9999");
        page.handle_key(key(KeyCode::Enter), &mut state);
        assert_eq!(page.focus, Focus::ClientsFile);
        page.handle_key(key(KeyCode::Enter), &mut state);
        assert_eq!(page.focus, Focus::WorkerPort);
        type_string(&mut page, &mut state, "9999");
        let outcome = page.handle_key(key(KeyCode::Enter), &mut state);
        assert_eq!(outcome, PageOutcome::Next);
        assert_eq!(state.worker_port, Some(9999));
    }

    #[test]
    fn test_mode_coordinator_clients_file_alone_is_valid() {
        let mut page = ModePage::new();
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Down), &mut state); // Coordinator
        page.handle_key(key(KeyCode::Enter), &mut state); // → Detail
        // Skip detail, go to clients_file.
        page.handle_key(key(KeyCode::Enter), &mut state); // → ClientsFile
        type_string(&mut page, &mut state, "/etc/iopulse/hosts.txt");
        // Validation should now pass (empty host_list but present clients_file).
        let issues = page.validate(&state);
        assert!(
            !issues.iter().any(|i| i.message.contains("host list")),
            "got: {:?}",
            issues
        );
        assert_eq!(state.clients_file.as_deref(), Some("/etc/iopulse/hosts.txt"));
    }

    #[test]
    fn test_mode_coordinator_worker_port_validates() {
        let mut page = ModePage::new();
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Down), &mut state); // Coordinator
        page.handle_key(key(KeyCode::Enter), &mut state); // → Detail
        type_string(&mut page, &mut state, "host1:9999");
        page.handle_key(key(KeyCode::Enter), &mut state); // → ClientsFile
        page.handle_key(key(KeyCode::Enter), &mut state); // → WorkerPort
        type_string(&mut page, &mut state, "abc");
        let issues = page.validate(&state);
        assert!(issues.iter().any(|i| i.message.contains("Worker port")));
    }

    #[test]
    fn test_mode_service_validates_port() {
        let mut page = ModePage::new();
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Down), &mut state);
        page.handle_key(key(KeyCode::Down), &mut state); // Service
        page.handle_key(key(KeyCode::Enter), &mut state); // → Detail
        type_string(&mut page, &mut state, "abc");
        let issues = page.validate(&state);
        assert!(issues.iter().any(|i| i.message.contains("number")));
    }

    #[test]
    fn test_mode_renders_without_panic() {
        let page = ModePage::new();
        let state = WizardState::new();
        let backend = TestBackend::new(60, 16);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| page.render(frame, frame.area(), &state))
            .expect("render");
    }
}
