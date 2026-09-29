//! Workers page: parallelism + locking + NUMA pinning.
//!
//! Threads count, CPU pin list, NUMA zone list, file distribution mode, and
//! per-IO file lock mode (none / range / full).

use crate::config::workload::{FileDistribution, FileLockMode};
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{Severity, ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

const DISTRIBUTIONS: &[(FileDistribution, &str)] = &[
    (FileDistribution::Shared, "Shared — all workers all files"),
    (FileDistribution::Partitioned, "Partitioned — files split"),
    (FileDistribution::PerWorker, "Per-worker — one file each"),
];

const LOCK_MODES: &[(FileLockMode, &str, &str)] = &[
    (FileLockMode::None, "none", "no locking (fastest, may corrupt shared writes)"),
    (FileLockMode::Range, "range", "lock byte range per IO (realistic shared write)"),
    (FileLockMode::Full, "full", "lock entire file per IO (worst-case contention)"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Threads,
    Cores,
    Numa,
    RateIops,
    RateThroughput,
    Distribution,
    LockMode,
}

const FOCUS_ORDER: &[Focus] = &[
    Focus::Threads,
    Focus::Cores,
    Focus::Numa,
    Focus::RateIops,
    Focus::RateThroughput,
    Focus::Distribution,
    Focus::LockMode,
];

#[derive(Debug)]
pub struct WorkersPage {
    threads: Input,
    cores: Input,
    numa: Input,
    rate_iops: Input,
    rate_throughput: Input,
    dist_selected: usize,
    lock_selected: usize,
    focus: Focus,
    pub page_index: usize,
}

impl WorkersPage {
    pub fn new(page_index: usize) -> Self {
        let cpus = num_cpus::get().to_string();
        Self {
            threads: Input::default().with_value(cpus),
            cores: Input::default(),
            numa: Input::default(),
            rate_iops: Input::default(),
            rate_throughput: Input::default(),
            dist_selected: 0,
            lock_selected: 0,
            focus: Focus::Threads,
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

    fn is_text_field(focus: Focus) -> bool {
        matches!(
            focus,
            Focus::Threads | Focus::Cores | Focus::Numa | Focus::RateIops | Focus::RateThroughput
        )
    }

    fn flush_to_state(&self, state: &mut WizardState) {
        if let Ok(t) = self.threads.value().parse::<usize>() {
            state.config.workers.threads = t;
        }
        let cores_value = self.cores.value();
        state.config.workers.cpu_cores = if cores_value.is_empty() {
            None
        } else {
            Some(cores_value.to_string())
        };
        let numa_value = self.numa.value();
        state.config.workers.numa_zones = if numa_value.is_empty() {
            None
        } else {
            Some(numa_value.to_string())
        };
        state.config.workers.rate_limit_iops = self.rate_iops.value().parse::<u64>().ok();
        state.config.workers.rate_limit_throughput =
            self.rate_throughput.value().parse::<u64>().ok();
        if let Some(target) = state.config.targets.first_mut() {
            target.distribution = DISTRIBUTIONS[self.dist_selected].0;
            target.lock_mode = LOCK_MODES[self.lock_selected].0;
        }
    }
}

impl Page for WorkersPage {
    fn title(&self) -> &str {
        "Workers"
    }

    fn description(&self) -> &str {
        "Parallelism + how workers share files. Threads = parallel worker count. \
        CPU cores / NUMA zones pin workers to specific cores or NUMA nodes (optional). \
        File distribution: Shared (all workers all files), Partitioned (split regions), \
        Per-worker (one file each). Lock mode controls per-IO file locking — needed \
        when writing to a shared file."
    }

    fn keybindings(&self) -> &str {
        "Tab/Enter: next field · ↑/↓ on lists: pick"
    }

    fn render(&self, frame: &mut Frame, area: Rect, _state: &WizardState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Threads
                Constraint::Length(3), // Cores
                Constraint::Length(3), // NUMA
                Constraint::Length(3), // Rate IOPS
                Constraint::Length(3), // Rate throughput
                Constraint::Min(5),    // Distribution list
                Constraint::Min(5),    // Lock mode list
            ])
            .split(area);

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

        render_text(
            frame,
            chunks[0],
            Focus::Threads,
            "Threads",
            &self.threads,
            "default = CPU count",
        );
        render_text(
            frame,
            chunks[1],
            Focus::Cores,
            "CPU cores (optional)",
            &self.cores,
            "(blank = all cores) e.g. 0,1,2,3 or 0-7",
        );
        render_text(
            frame,
            chunks[2],
            Focus::Numa,
            "NUMA zones (optional)",
            &self.numa,
            "(blank = no pinning) e.g. 0,1",
        );
        render_text(
            frame,
            chunks[3],
            Focus::RateIops,
            "Per-worker IOPS limit (optional)",
            &self.rate_iops,
            "(blank = no limit) e.g. 1000",
        );
        render_text(
            frame,
            chunks[4],
            Focus::RateThroughput,
            "Per-worker throughput limit, bytes/sec (optional)",
            &self.rate_throughput,
            "(blank = no limit) e.g. 100000000 (= 100MB/s)",
        );

        let dist_focus = self.focus == Focus::Distribution;
        let dist_items: Vec<ListItem> = DISTRIBUTIONS
            .iter()
            .map(|(_, label)| ListItem::new(*label))
            .collect();
        let dist_block = Block::default()
            .title(if dist_focus {
                " File distribution (focused) "
            } else {
                " File distribution "
            })
            .borders(Borders::ALL)
            .border_style(if dist_focus {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            });
        let dist_list = List::new(dist_items)
            .block(dist_block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut dist_state = ListState::default();
        dist_state.select(Some(self.dist_selected));
        frame.render_stateful_widget(dist_list, chunks[5], &mut dist_state);

        let lock_focus = self.focus == Focus::LockMode;
        let lock_items: Vec<ListItem> = LOCK_MODES
            .iter()
            .map(|(_, name, desc)| ListItem::new(format!("{:6} — {}", name, desc)))
            .collect();
        let lock_block = Block::default()
            .title(if lock_focus {
                " Lock mode (focused) "
            } else {
                " Lock mode "
            })
            .borders(Borders::ALL)
            .border_style(if lock_focus {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            });
        let lock_list = List::new(lock_items)
            .block(lock_block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut lock_state = ListState::default();
        lock_state.select(Some(self.lock_selected));
        frame.render_stateful_widget(lock_list, chunks[6], &mut lock_state);
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
            (Focus::Distribution, KeyCode::Up) => {
                if self.dist_selected > 0 {
                    self.dist_selected -= 1;
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::Distribution, KeyCode::Down) => {
                if self.dist_selected + 1 < DISTRIBUTIONS.len() {
                    self.dist_selected += 1;
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::LockMode, KeyCode::Up) => {
                if self.lock_selected > 0 {
                    self.lock_selected -= 1;
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::LockMode, KeyCode::Down) => {
                if self.lock_selected + 1 < LOCK_MODES.len() {
                    self.lock_selected += 1;
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            // Up/Down on text fields cycles to neighbor fields.
            (focus, KeyCode::Up) if Self::is_text_field(focus) => {
                self.focus = self.prev_focus();
                PageOutcome::Stay
            }
            (focus, KeyCode::Down) if Self::is_text_field(focus) => {
                self.focus = self.next_focus();
                PageOutcome::Stay
            }
            (_, KeyCode::Enter) => {
                self.flush_to_state(state);
                state.dirty = true;
                if self.focus == Focus::LockMode {
                    PageOutcome::Next
                } else {
                    self.focus = self.next_focus();
                    PageOutcome::Stay
                }
            }
            (_, KeyCode::Esc) => PageOutcome::Quit,
            (focus, _) if Self::is_text_field(focus) => {
                let target = match self.focus {
                    Focus::Threads => &mut self.threads,
                    Focus::Cores => &mut self.cores,
                    Focus::Numa => &mut self.numa,
                    Focus::RateIops => &mut self.rate_iops,
                    Focus::RateThroughput => &mut self.rate_throughput,
                    _ => unreachable!(),
                };
                target.handle_event(&crossterm::event::Event::Key(key));
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            _ => PageOutcome::Stay,
        }
    }

    fn sync_from_state(&mut self, state: &WizardState) {
        self.threads = Input::default().with_value(format!("{}", state.config.workers.threads));
        if let Some(cores) = state.config.workers.cpu_cores.as_deref() {
            self.cores = Input::default().with_value(cores.to_string());
        }
        if let Some(numa) = state.config.workers.numa_zones.as_deref() {
            self.numa = Input::default().with_value(numa.to_string());
        }
        if let Some(n) = state.config.workers.rate_limit_iops {
            self.rate_iops = Input::default().with_value(format!("{}", n));
        }
        if let Some(n) = state.config.workers.rate_limit_throughput {
            self.rate_throughput = Input::default().with_value(format!("{}", n));
        }
        if let Some(target) = state.config.targets.first() {
            if let Some(idx) = DISTRIBUTIONS
                .iter()
                .position(|(d, _)| *d == target.distribution)
            {
                self.dist_selected = idx;
            }
            if let Some(idx) = LOCK_MODES.iter().position(|(m, _, _)| *m == target.lock_mode) {
                self.lock_selected = idx;
            }
        }
    }

    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if state.config.workers.threads == 0 {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: "Threads must be > 0".to_string(),
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

    fn type_string(page: &mut WorkersPage, state: &mut WizardState, s: &str) {
        for ch in s.chars() {
            page.handle_key(key(KeyCode::Char(ch)), state);
        }
    }

    #[test]
    fn test_workers_default_threads_is_num_cpus() {
        let page = WorkersPage::new(7);
        assert_eq!(page.threads.value(), num_cpus::get().to_string());
    }

    #[test]
    fn test_workers_typing_threads_updates_config() {
        let mut page = WorkersPage::new(7);
        let mut state = WizardState::new();
        for _ in 0..4 {
            page.handle_key(key(KeyCode::Backspace), &mut state);
        }
        type_string(&mut page, &mut state, "8");
        assert_eq!(state.config.workers.threads, 8);
    }

    #[test]
    fn test_workers_numa_zones_lands_in_state() {
        let mut page = WorkersPage::new(7);
        let mut state = WizardState::new();
        // Tab to NUMA (Threads, Cores, NUMA).
        page.handle_key(key(KeyCode::Tab), &mut state);
        page.handle_key(key(KeyCode::Tab), &mut state);
        assert_eq!(page.focus, Focus::Numa);
        type_string(&mut page, &mut state, "0,1");
        assert_eq!(state.config.workers.numa_zones.as_deref(), Some("0,1"));
    }

    #[test]
    fn test_workers_distribution_selection() {
        let mut page = WorkersPage::new(7);
        let mut state = WizardState::new();
        // Tab to Distribution: Threads → Cores → NUMA → RateIops → RateThroughput → Distribution.
        for _ in 0..5 {
            page.handle_key(key(KeyCode::Tab), &mut state);
        }
        assert_eq!(page.focus, Focus::Distribution);
        page.handle_key(key(KeyCode::Down), &mut state);
        assert_eq!(
            state.config.targets[0].distribution,
            FileDistribution::Partitioned
        );
    }

    #[test]
    fn test_workers_lock_mode_selection() {
        let mut page = WorkersPage::new(7);
        let mut state = WizardState::new();
        // Tab to LockMode (7th focus position).
        for _ in 0..6 {
            page.handle_key(key(KeyCode::Tab), &mut state);
        }
        assert_eq!(page.focus, Focus::LockMode);
        page.handle_key(key(KeyCode::Down), &mut state);
        assert_eq!(state.config.targets[0].lock_mode, FileLockMode::Range);
    }

    #[test]
    fn test_workers_rate_limit_iops_lands_in_state() {
        let mut page = WorkersPage::new(7);
        let mut state = WizardState::new();
        // Tab to RateIops: Threads → Cores → NUMA → RateIops
        for _ in 0..3 {
            page.handle_key(key(KeyCode::Tab), &mut state);
        }
        assert_eq!(page.focus, Focus::RateIops);
        type_string(&mut page, &mut state, "1000");
        assert_eq!(state.config.workers.rate_limit_iops, Some(1000));
    }

    #[test]
    fn test_workers_cpu_cores_optional() {
        let mut page = WorkersPage::new(7);
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Tab), &mut state); // -> Cores
        type_string(&mut page, &mut state, "0,1,2,3");
        assert_eq!(state.config.workers.cpu_cores.as_deref(), Some("0,1,2,3"));
    }
}
