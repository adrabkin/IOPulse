//! Phases page: multi-phase test toggle.
//!
//! `[m]` toggles `state.multi_phase` between None and Some. When Some, `[a]dd`
//! pushes a new phase (initialized from the current single-phase workload),
//! `[d]el` removes the last phase, Up/Down navigates the phase list.
//!
//! v1 keeps the per-phase editing simple: each new phase inherits the current
//! workload as its starting point; deeper per-phase customization (separate
//! workload mix, separate distribution) is a v2 follow-up that the plan
//! explicitly defers.

use crate::config::{MultiPhaseConfig, PhaseConfig};
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

#[derive(Debug)]
pub struct PhasesPage {
    selected: usize,
    pub page_index: usize,
}

impl PhasesPage {
    pub fn new(page_index: usize) -> Self {
        Self {
            selected: 0,
            page_index,
        }
    }
}

impl Page for PhasesPage {
    fn title(&self) -> &str {
        "Phases (optional)"
    }

    fn description(&self) -> &str {
        "Multi-phase tests run several workloads back-to-back in one job \
        (e.g. ramp-up read, then write, then read again). For most users a \
        single-phase test is what you want — just press Enter to continue. \
        Press 'm' to toggle multi-phase mode; 'a' adds a phase, 'd' removes \
        the last one."
    }

    fn keybindings(&self) -> &str {
        "m: toggle multi-phase · a: add · d: remove · Enter: next"
    }

    fn render(&self, frame: &mut Frame, area: Rect, state: &WizardState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        let header = match &state.multi_phase {
            None => "Single-phase test ([m]ulti-phase: off)".to_string(),
            Some(mp) => format!(
                "Multi-phase test ({} phase(s)) — [a]dd, [d]el, [m] off",
                mp.phases.len()
            ),
        };
        frame.render_widget(
            Paragraph::new(header).block(Block::default().borders(Borders::ALL)),
            chunks[0],
        );

        if let Some(mp) = &state.multi_phase {
            let items: Vec<ListItem> = mp
                .phases
                .iter()
                .map(|p| ListItem::new(format!("{}: {}", p.name, p.workload)))
                .collect();
            let list = List::new(items)
                .block(Block::default().title("Phases").borders(Borders::ALL))
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            let mut list_state = ListState::default();
            list_state.select(Some(self.selected.min(mp.phases.len().saturating_sub(1))));
            frame.render_stateful_widget(list, chunks[1], &mut list_state);
        }
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut WizardState) -> PageOutcome {
        match key.code {
            KeyCode::Char('m') => {
                state.multi_phase = match state.multi_phase.take() {
                    None => Some(MultiPhaseConfig {
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
                    }),
                    Some(_) => None,
                };
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Char('a') => {
                if let Some(mp) = state.multi_phase.as_mut() {
                    let n = mp.phases.len() + 1;
                    mp.phases.push(PhaseConfig {
                        name: format!("phase-{}", n),
                        workload: state.config.workload.clone(),
                        targets: None,
                        stonewall: false,
                    });
                    state.dirty = true;
                }
                PageOutcome::Stay
            }
            KeyCode::Char('d') => {
                if let Some(mp) = state.multi_phase.as_mut() {
                    if mp.phases.len() > 1 {
                        mp.phases.pop();
                        if self.selected >= mp.phases.len() {
                            self.selected = mp.phases.len().saturating_sub(1);
                        }
                        state.dirty = true;
                    }
                }
                PageOutcome::Stay
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                PageOutcome::Stay
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(mp) = state.multi_phase.as_ref() {
                    if self.selected + 1 < mp.phases.len() {
                        self.selected += 1;
                    }
                }
                PageOutcome::Stay
            }
            KeyCode::Enter => PageOutcome::Next,
            KeyCode::Esc => PageOutcome::Quit,
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
    fn test_phases_default_single_phase() {
        let state = WizardState::new();
        assert!(state.multi_phase.is_none());
    }

    #[test]
    fn test_phases_m_toggles_multi_phase() {
        let mut page = PhasesPage::new(9);
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Char('m')), &mut state);
        let mp = state.multi_phase.as_ref().expect("multi_phase enabled");
        assert_eq!(mp.phases.len(), 1);
        assert_eq!(mp.phases[0].name, "phase-1");

        page.handle_key(key(KeyCode::Char('m')), &mut state);
        assert!(state.multi_phase.is_none());
    }

    #[test]
    fn test_phases_a_adds_phase() {
        let mut page = PhasesPage::new(9);
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Char('m')), &mut state); // enable
        page.handle_key(key(KeyCode::Char('a')), &mut state); // add
        page.handle_key(key(KeyCode::Char('a')), &mut state); // add
        let mp = state.multi_phase.as_ref().unwrap();
        assert_eq!(mp.phases.len(), 3);
        assert_eq!(mp.phases[2].name, "phase-3");
    }

    #[test]
    fn test_phases_d_keeps_at_least_one_phase() {
        let mut page = PhasesPage::new(9);
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Char('m')), &mut state); // enable; 1 phase
        page.handle_key(key(KeyCode::Char('d')), &mut state); // can't go below 1
        let mp = state.multi_phase.as_ref().unwrap();
        assert_eq!(mp.phases.len(), 1);
    }
}
