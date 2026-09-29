//! Distribution page: random distribution + per-distribution params.
//!
//! Selects between Uniform / Zipf / Pareto / Gaussian. The relevant param
//! field(s) are revealed based on the choice (theta for Zipf, h for Pareto,
//! stddev + center for Gaussian; Uniform has none).

use crate::config::workload::DistributionType;
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
    Uniform,
    Zipf,
    Pareto,
    Gaussian,
}

const KINDS: &[(Kind, &str)] = &[
    (Kind::Uniform, "Uniform — flat random"),
    (Kind::Zipf, "Zipf — power law (hot keys)"),
    (Kind::Pareto, "Pareto — 80/20 rule"),
    (Kind::Gaussian, "Gaussian — bell around center"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Param1,
    Param2,
}

#[derive(Debug)]
pub struct DistributionPage {
    selected: usize,
    /// theta (Zipf) or h (Pareto) or stddev (Gaussian).
    param1: Input,
    /// center (Gaussian only).
    param2: Input,
    focus: Focus,
    pub page_index: usize,
}

impl DistributionPage {
    pub fn new(page_index: usize) -> Self {
        Self {
            selected: 0,
            param1: Input::default().with_value("1.2".to_string()),
            param2: Input::default().with_value("0.5".to_string()),
            focus: Focus::List,
            page_index,
        }
    }

    fn current_kind(&self) -> Kind {
        KINDS[self.selected].0
    }

    fn flush_to_state(&self, state: &mut WizardState) {
        let p1 = self.param1.value().parse::<f64>().unwrap_or(1.2);
        let p2 = self.param2.value().parse::<f64>().unwrap_or(0.5);
        state.config.workload.distribution = match self.current_kind() {
            Kind::Uniform => DistributionType::Uniform,
            Kind::Zipf => DistributionType::Zipf { theta: p1 },
            Kind::Pareto => DistributionType::Pareto { h: p1 },
            Kind::Gaussian => DistributionType::Gaussian {
                stddev: p1,
                center: p2,
            },
        };
    }
}

impl Page for DistributionPage {
    fn title(&self) -> &str {
        "Access distribution"
    }

    fn description(&self) -> &str {
        "How offsets are picked across the file. \
        Uniform = every block equally likely (raw disk benchmarking). \
        Zipf = a few \"hot\" blocks accessed often (caches, key-value stores). \
        Pareto = 80/20 — heavy tail (real-world content access). \
        Gaussian = bell curve around a center point (working-set tests). \
        Higher Zipf theta = hotter hot keys; Pareto h is the shape parameter."
    }

    fn keybindings(&self) -> &str {
        "↑/↓: pick · Tab: edit params · Enter: next"
    }

    fn render(&self, frame: &mut Frame, area: Rect, _state: &WizardState) {
        // Side-by-side: list of distributions on the left, parameter fields
        // for the currently-selected distribution on the right. Keeps the
        // selector and "what its parameters mean" visually together.
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
            .title(if list_focus { " Distribution (focused) " } else { " Distribution " })
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

        let kind = self.current_kind();
        let (label1, show2, label2) = match kind {
            Kind::Uniform => ("(no parameters needed)", false, ""),
            Kind::Zipf => ("theta (0.0 – 3.0)", false, ""),
            Kind::Pareto => ("h (0.0 – 10.0)", false, ""),
            Kind::Gaussian => ("stddev", true, "center (0.0 – 1.0)"),
        };

        let param_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Min(0)])
            .split(panes[1]);

        let p1_block = Block::default()
            .title(format!(" {} ", label1))
            .borders(Borders::ALL)
            .border_style(if self.focus == Focus::Param1 {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            });
        frame.render_widget(
            Paragraph::new(self.param1.value()).block(p1_block),
            param_chunks[0],
        );

        if show2 {
            let p2_block = Block::default()
                .title(format!(" {} ", label2))
                .borders(Borders::ALL)
                .border_style(if self.focus == Focus::Param2 {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                });
            frame.render_widget(
                Paragraph::new(self.param2.value()).block(p2_block),
                param_chunks[1],
            );
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
                    (Focus::List, Kind::Uniform) => Focus::List,
                    (Focus::List, _) => Focus::Param1,
                    (Focus::Param1, Kind::Gaussian) => Focus::Param2,
                    (Focus::Param1, _) => Focus::List,
                    (Focus::Param2, _) => Focus::List,
                };
                PageOutcome::Stay
            }
            (_, KeyCode::Enter) => {
                self.flush_to_state(state);
                state.dirty = true;
                // Walk Enter through fields based on the current distribution
                // shape: Uniform has no params (advance directly); Zipf/Pareto
                // have one param; Gaussian has two.
                let kind = self.current_kind();
                match (self.focus, kind) {
                    (Focus::List, Kind::Uniform) => PageOutcome::Next,
                    (Focus::List, _) => {
                        self.focus = Focus::Param1;
                        PageOutcome::Stay
                    }
                    (Focus::Param1, Kind::Gaussian) => {
                        self.focus = Focus::Param2;
                        PageOutcome::Stay
                    }
                    (Focus::Param1, _) => PageOutcome::Next,
                    (Focus::Param2, _) => PageOutcome::Next,
                }
            }
            (_, KeyCode::Esc) => PageOutcome::Quit,
            (Focus::Param1, _) => {
                self.param1
                    .handle_event(&crossterm::event::Event::Key(key));
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::Param2, _) => {
                self.param2
                    .handle_event(&crossterm::event::Event::Key(key));
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            (Focus::List, _) => PageOutcome::Stay,
        }
    }

    fn sync_from_state(&mut self, state: &WizardState) {
        let (kind, p1, p2) = match state.config.workload.distribution {
            DistributionType::Uniform => (Kind::Uniform, None, None),
            DistributionType::Zipf { theta } => (Kind::Zipf, Some(theta), None),
            DistributionType::Pareto { h } => (Kind::Pareto, Some(h), None),
            DistributionType::Gaussian { stddev, center } => {
                (Kind::Gaussian, Some(stddev), Some(center))
            }
        };
        self.selected = KINDS.iter().position(|(k, _)| *k == kind).unwrap_or(0);
        if let Some(v) = p1 {
            self.param1 = Input::default().with_value(format!("{}", v));
        }
        if let Some(v) = p2 {
            self.param2 = Input::default().with_value(format!("{}", v));
        }
    }

    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        match state.config.workload.distribution {
            DistributionType::Uniform => {}
            DistributionType::Zipf { theta } => {
                if !(0.0..=3.0).contains(&theta) {
                    issues.push(ValidationIssue {
                        severity: Severity::Error,
                        page: self.page_index,
                        message: format!("Zipf theta must be 0.0-3.0 (got {})", theta),
                    });
                }
            }
            DistributionType::Pareto { h } => {
                if !(0.0..=10.0).contains(&h) {
                    issues.push(ValidationIssue {
                        severity: Severity::Error,
                        page: self.page_index,
                        message: format!("Pareto h must be 0.0-10.0 (got {})", h),
                    });
                }
            }
            DistributionType::Gaussian { center, .. } => {
                if !(0.0..=1.0).contains(&center) {
                    issues.push(ValidationIssue {
                        severity: Severity::Error,
                        page: self.page_index,
                        message: format!("Gaussian center must be 0.0-1.0 (got {})", center),
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_distribution_default_is_uniform() {
        let page = DistributionPage::new(4);
        assert_eq!(page.current_kind(), Kind::Uniform);
    }

    #[test]
    fn test_distribution_down_to_zipf_writes_theta() {
        let mut page = DistributionPage::new(4);
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Down), &mut state); // -> Zipf
        match state.config.workload.distribution {
            DistributionType::Zipf { theta } => assert_eq!(theta, 1.2),
            other => panic!("expected Zipf, got {:?}", other),
        }
    }

    #[test]
    fn test_distribution_validate_zipf_out_of_range() {
        let page = DistributionPage::new(4);
        let mut state = WizardState::new();
        state.config.workload.distribution = DistributionType::Zipf { theta: 5.0 };
        let issues = page.validate(&state);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("0.0-3.0"));
    }

    #[test]
    fn test_distribution_sync_from_state_pareto() {
        let mut page = DistributionPage::new(4);
        let mut state = WizardState::new();
        state.config.workload.distribution = DistributionType::Pareto { h: 2.0 };
        page.sync_from_state(&state);
        assert_eq!(page.current_kind(), Kind::Pareto);
        assert_eq!(page.param1.value(), "2");
    }

    #[test]
    fn test_distribution_gaussian_revealed_param2_focus_path() {
        let mut page = DistributionPage::new(4);
        let mut state = WizardState::new();
        // Down 3 times to reach Gaussian
        for _ in 0..3 {
            page.handle_key(key(KeyCode::Down), &mut state);
        }
        assert_eq!(page.current_kind(), Kind::Gaussian);
        // Tab: List -> Param1
        page.handle_key(key(KeyCode::Tab), &mut state);
        assert_eq!(page.focus, Focus::Param1);
        // Tab again on Gaussian: Param1 -> Param2
        page.handle_key(key(KeyCode::Tab), &mut state);
        assert_eq!(page.focus, Focus::Param2);
    }
}
