//! Engine page: IO engine + direct/sync flags.
//!
//! List selector for engine (sync, io_uring, libaio, mmap), plus toggle keys
//! for `[d]irect` (O_DIRECT) and `[s]ync` (O_SYNC). Validation warns when
//! O_DIRECT is on but block size is not 4K-aligned.

use crate::config::workload::EngineType;
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{Severity, ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

const ENGINES: &[(EngineType, &str)] = &[
    (EngineType::Sync, "sync — pread/pwrite (default)"),
    (EngineType::IoUring, "io_uring — Linux 5.1+ async"),
    (EngineType::Libaio, "libaio — Linux async"),
    (EngineType::Mmap, "mmap — memory-mapped"),
];

#[derive(Debug)]
pub struct EnginePage {
    selected: usize,
    pub page_index: usize,
}

impl EnginePage {
    pub fn new(page_index: usize) -> Self {
        Self {
            selected: 0,
            page_index,
        }
    }

}

impl Page for EnginePage {
    fn title(&self) -> &str {
        "I/O engine"
    }

    fn description(&self) -> &str {
        "The kernel interface used for reads/writes.  \
        sync = simple pread/pwrite, works everywhere.  \
        io_uring = Linux 5.1+ async, best peak performance with high queue depth.  \
        libaio = older Linux async (use only if io_uring unavailable).  \
        mmap = memory-mapped, good for sequential streaming.  \
        Direct (O_DIRECT) bypasses the page cache — measures the disk, not \
        cached pages, but requires 4K-aligned blocks. Sync (O_SYNC) forces \
        flush after every write."
    }

    fn keybindings(&self) -> &str {
        "↑/↓: pick engine · d: toggle direct · s: toggle sync · Enter: next"
    }

    fn render(&self, frame: &mut Frame, area: Rect, state: &WizardState) {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);

        let items: Vec<ListItem> = ENGINES
            .iter()
            .map(|(_, label)| ListItem::new(*label))
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Engine ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut list_state = ListState::default();
        list_state.select(Some(self.selected));
        frame.render_stateful_widget(list, panes[0], &mut list_state);

        let toggles = format!(
            " [d]irect: {}\n [s]ync:   {}",
            if state.config.workload.direct {
                "ON"
            } else {
                "off"
            },
            if state.config.workload.sync {
                "ON"
            } else {
                "off"
            },
        );
        let para = Paragraph::new(toggles)
            .block(Block::default().title(" Toggles ").borders(Borders::ALL));
        frame.render_widget(para, panes[1]);
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut WizardState) -> PageOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                state.config.workload.engine = ENGINES[self.selected].0;
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < ENGINES.len() {
                    self.selected += 1;
                }
                state.config.workload.engine = ENGINES[self.selected].0;
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Char('d') => {
                state.config.workload.direct = !state.config.workload.direct;
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Char('s') => {
                state.config.workload.sync = !state.config.workload.sync;
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Enter => PageOutcome::Next,
            KeyCode::Esc => PageOutcome::Quit,
            _ => PageOutcome::Stay,
        }
    }

    fn sync_from_state(&mut self, state: &WizardState) {
        let engine = state.config.workload.engine;
        if let Some(idx) = ENGINES.iter().position(|(e, _)| *e == engine) {
            self.selected = idx;
        }
    }

    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let workload = &state.config.workload;

        // O_DIRECT requires aligned blocks. 4K is the safe minimum on every
        // real filesystem we care about.
        if workload.direct && workload.block_size % 4096 != 0 {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                page: self.page_index,
                message: format!(
                    "O_DIRECT requires 4K-aligned block size; current is {} bytes",
                    workload.block_size
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

    #[test]
    fn test_engine_default_is_sync() {
        let page = EnginePage::new(2);
        let state = WizardState::new();
        assert_eq!(state.config.workload.engine, EngineType::Sync);
        assert_eq!(page.selected, 0);
    }

    #[test]
    fn test_engine_down_selects_io_uring() {
        let mut page = EnginePage::new(2);
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Down), &mut state);
        assert_eq!(state.config.workload.engine, EngineType::IoUring);
    }

    #[test]
    fn test_engine_d_toggles_direct() {
        let mut page = EnginePage::new(2);
        let mut state = WizardState::new();
        assert!(!state.config.workload.direct);
        page.handle_key(key(KeyCode::Char('d')), &mut state);
        assert!(state.config.workload.direct);
        page.handle_key(key(KeyCode::Char('d')), &mut state);
        assert!(!state.config.workload.direct);
    }

    #[test]
    fn test_engine_validate_warns_on_direct_with_unaligned_block() {
        let page = EnginePage::new(2);
        let mut state = WizardState::new();
        state.config.workload.direct = true;
        state.config.workload.block_size = 1023; // not 4K-aligned
        let issues = page.validate(&state);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert!(issues[0].message.contains("4K"));
    }

    #[test]
    fn test_engine_validate_clean_when_direct_off() {
        let page = EnginePage::new(2);
        let mut state = WizardState::new();
        state.config.workload.direct = false;
        state.config.workload.block_size = 1023;
        assert!(page.validate(&state).is_empty());
    }
}
