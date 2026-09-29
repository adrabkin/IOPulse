//! Block size + queue depth page.
//!
//! Block size accepts the same syntax as the CLI (`4k`, `64k`, `1M`...);
//! queue depth is a plain integer 1..=1024. When the engine is sync, queue
//! depth is shown but flagged as informational ("ignored by sync engine").

use crate::config::cli_convert::parse_size;
use crate::config::workload::EngineType;
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{Severity, ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Block,
    Queue,
}

#[derive(Debug)]
pub struct BlockQueuePage {
    block: Input,
    queue: Input,
    focus: Focus,
    pub page_index: usize,
}

impl BlockQueuePage {
    pub fn new(page_index: usize) -> Self {
        Self {
            block: Input::default().with_value("4k".to_string()),
            queue: Input::default().with_value("1".to_string()),
            focus: Focus::Block,
            page_index,
        }
    }


    fn flush_to_state(&self, state: &mut WizardState) {
        if let Ok(bs) = parse_size(self.block.value()) {
            state.config.workload.block_size = bs;
        }
        if let Ok(qd) = self.queue.value().parse::<usize>() {
            state.config.workload.queue_depth = qd;
        }
    }
}

impl Page for BlockQueuePage {
    fn title(&self) -> &str {
        "Block size & queue depth"
    }

    fn description(&self) -> &str {
        "Block size = bytes per IO. 4K matches OS page size (random IO, OLTP); \
        64K-1M is typical for streaming/analytics. Queue depth = how many IOs \
        can be in flight at once per worker. Higher QD increases throughput on \
        async engines (io_uring, libaio). The sync engine ignores QD — every \
        IO is a blocking syscall."
    }

    fn render(&self, frame: &mut Frame, area: Rect, state: &WizardState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(2),
                Constraint::Min(0),
            ])
            .split(area);

        let block_block = Block::default()
            .title(" Block size (e.g. 4k, 64k, 1M) ")
            .borders(Borders::ALL)
            .border_style(if self.focus == Focus::Block {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            });
        frame.render_widget(
            Paragraph::new(self.block.value()).block(block_block),
            chunks[0],
        );

        let queue_block = Block::default()
            .title(" Queue depth (1-1024) ")
            .borders(Borders::ALL)
            .border_style(if self.focus == Focus::Queue {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            });
        frame.render_widget(
            Paragraph::new(self.queue.value()).block(queue_block),
            chunks[1],
        );

        if state.config.workload.engine == EngineType::Sync {
            frame.render_widget(
                Paragraph::new("Note: queue depth is ignored by the sync engine.")
                    .style(Style::default().fg(Color::DarkGray)),
                chunks[2],
            );
        }
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut WizardState) -> PageOutcome {
        match key.code {
            KeyCode::Tab | KeyCode::Down | KeyCode::BackTab | KeyCode::Up => {
                self.focus = match self.focus {
                    Focus::Block => Focus::Queue,
                    Focus::Queue => Focus::Block,
                };
                PageOutcome::Stay
            }
            KeyCode::Enter => {
                self.flush_to_state(state);
                state.dirty = true;
                match self.focus {
                    Focus::Block => {
                        self.focus = Focus::Queue;
                        PageOutcome::Stay
                    }
                    Focus::Queue => PageOutcome::Next,
                }
            }
            KeyCode::Esc => PageOutcome::Quit,
            _ => {
                let target = match self.focus {
                    Focus::Block => &mut self.block,
                    Focus::Queue => &mut self.queue,
                };
                target.handle_event(&crossterm::event::Event::Key(key));
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
        }
    }

    fn sync_from_state(&mut self, state: &WizardState) {
        self.block = Input::default()
            .with_value(format!("{}", state.config.workload.block_size));
        self.queue = Input::default()
            .with_value(format!("{}", state.config.workload.queue_depth));
    }

    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if parse_size(self.block.value()).is_err() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!("Invalid block size: {}", self.block.value()),
            });
        }
        let qd = state.config.workload.queue_depth;
        if qd == 0 || qd > 1024 {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!("Queue depth must be 1-1024 (got {})", qd),
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

    fn type_string(page: &mut BlockQueuePage, state: &mut WizardState, s: &str) {
        for ch in s.chars() {
            page.handle_key(key(KeyCode::Char(ch)), state);
        }
    }

    fn clear_input(page: &mut BlockQueuePage, state: &mut WizardState, n: usize) {
        for _ in 0..n {
            page.handle_key(key(KeyCode::Backspace), state);
        }
    }

    #[test]
    fn test_block_default_is_4k() {
        let page = BlockQueuePage::new(5);
        let state = WizardState::new();
        assert_eq!(state.config.workload.block_size, 4096);
        assert_eq!(page.block.value(), "4k");
    }

    #[test]
    fn test_block_typing_64k_parses() {
        let mut page = BlockQueuePage::new(5);
        let mut state = WizardState::new();
        clear_input(&mut page, &mut state, 5);
        type_string(&mut page, &mut state, "64k");
        assert_eq!(state.config.workload.block_size, 64 * 1024);
    }

    #[test]
    fn test_block_invalid_size_validates() {
        let mut page = BlockQueuePage::new(5);
        let mut state = WizardState::new();
        clear_input(&mut page, &mut state, 5);
        type_string(&mut page, &mut state, "--bogus");
        let issues = page.validate(&state);
        assert!(issues.iter().any(|i| i.message.contains("Invalid block size")));
    }

    #[test]
    fn test_queue_depth_zero_validates() {
        let mut page = BlockQueuePage::new(5);
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Tab), &mut state);
        clear_input(&mut page, &mut state, 3);
        type_string(&mut page, &mut state, "0");
        let issues = page.validate(&state);
        assert!(issues.iter().any(|i| i.message.contains("1-1024")));
    }
}
