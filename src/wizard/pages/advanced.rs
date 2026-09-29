//! Advanced workload page: think time, write pattern, kernel cache hints.

use crate::config::cli_convert::parse_time_us;
use crate::config::workload::{
    ThinkTimeConfig, ThinkTimeMode, VerifyPattern,
};
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{Severity, ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

const WRITE_PATTERNS: &[(VerifyPattern, &str)] = &[
    (VerifyPattern::Random, "random"),
    (VerifyPattern::Zeros, "zeros"),
    (VerifyPattern::Ones, "ones"),
    (VerifyPattern::Sequential, "sequential"),
];

const THINK_MODES: &[(ThinkTimeMode, &str)] = &[
    (ThinkTimeMode::Sleep, "sleep"),
    (ThinkTimeMode::Spin, "spin"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    WritePattern,
    ThinkTime,
    ThinkMode,
    ThinkEvery,
    ThinkAdaptive,
    FadviseSequential,
    FadviseRandom,
    FadviseWillneed,
    FadviseDontneed,
    FadviseNoreuse,
    MadviseSequential,
    MadviseRandom,
    MadviseWillneed,
    MadviseDontneed,
    MadviseHugepage,
    MadviseNohugepage,
}

const FOCUS_ORDER: &[Focus] = &[
    Focus::WritePattern,
    Focus::ThinkTime,
    Focus::ThinkMode,
    Focus::ThinkEvery,
    Focus::ThinkAdaptive,
    Focus::FadviseSequential,
    Focus::FadviseRandom,
    Focus::FadviseWillneed,
    Focus::FadviseDontneed,
    Focus::FadviseNoreuse,
    Focus::MadviseSequential,
    Focus::MadviseRandom,
    Focus::MadviseWillneed,
    Focus::MadviseDontneed,
    Focus::MadviseHugepage,
    Focus::MadviseNohugepage,
];

#[derive(Debug)]
pub struct AdvancedPage {
    write_pattern_idx: usize,
    think_time: Input,
    think_mode_idx: usize,
    think_every: Input,
    think_adaptive: Input,
    pub page_index: usize,
    focus: Focus,
}

impl AdvancedPage {
    pub fn new(page_index: usize) -> Self {
        Self {
            write_pattern_idx: 0,
            think_time: Input::default(),
            think_mode_idx: 0,
            think_every: Input::default(),
            think_adaptive: Input::default(),
            page_index,
            focus: Focus::WritePattern,
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
            Focus::ThinkTime | Focus::ThinkEvery | Focus::ThinkAdaptive
        )
    }

    fn is_cycler(focus: Focus) -> bool {
        matches!(focus, Focus::WritePattern | Focus::ThinkMode)
    }

    fn is_toggle(focus: Focus) -> bool {
        matches!(
            focus,
            Focus::FadviseSequential
                | Focus::FadviseRandom
                | Focus::FadviseWillneed
                | Focus::FadviseDontneed
                | Focus::FadviseNoreuse
                | Focus::MadviseSequential
                | Focus::MadviseRandom
                | Focus::MadviseWillneed
                | Focus::MadviseDontneed
                | Focus::MadviseHugepage
                | Focus::MadviseNohugepage
        )
    }

    fn cycle(&mut self, focus: Focus, forward: bool) {
        let (idx, len) = match focus {
            Focus::WritePattern => (&mut self.write_pattern_idx, WRITE_PATTERNS.len()),
            Focus::ThinkMode => (&mut self.think_mode_idx, THINK_MODES.len()),
            _ => return,
        };
        if forward {
            *idx = (*idx + 1) % len;
        } else {
            *idx = (*idx + len - 1) % len;
        }
    }

    fn toggle(&self, focus: Focus, state: &mut WizardState) {
        if let Some(target) = state.config.targets.first_mut() {
            match focus {
                Focus::FadviseSequential => target.fadvise_flags.sequential ^= true,
                Focus::FadviseRandom => target.fadvise_flags.random ^= true,
                Focus::FadviseWillneed => target.fadvise_flags.willneed ^= true,
                Focus::FadviseDontneed => target.fadvise_flags.dontneed ^= true,
                Focus::FadviseNoreuse => target.fadvise_flags.noreuse ^= true,
                Focus::MadviseSequential => target.madvise_flags.sequential ^= true,
                Focus::MadviseRandom => target.madvise_flags.random ^= true,
                Focus::MadviseWillneed => target.madvise_flags.willneed ^= true,
                Focus::MadviseDontneed => target.madvise_flags.dontneed ^= true,
                Focus::MadviseHugepage => target.madvise_flags.hugepage ^= true,
                Focus::MadviseNohugepage => target.madvise_flags.nohugepage ^= true,
                _ => {}
            }
        }
    }

    fn flush_to_state(&self, state: &mut WizardState) {
        state.config.workload.write_pattern = WRITE_PATTERNS[self.write_pattern_idx].0;

        let think_str = self.think_time.value();
        let adaptive: Option<u8> = self.think_adaptive.value().parse().ok();
        if !think_str.is_empty() || adaptive.is_some() {
            let duration_us = if think_str.is_empty() {
                0
            } else {
                parse_time_us(think_str).unwrap_or(0)
            };
            let every: usize = self
                .think_every
                .value()
                .parse()
                .unwrap_or(1)
                .max(1);
            state.config.workload.think_time = Some(ThinkTimeConfig {
                duration_us,
                mode: THINK_MODES[self.think_mode_idx].0,
                apply_every_n_blocks: every,
                adaptive_percent: adaptive,
            });
        } else {
            state.config.workload.think_time = None;
        }
    }
}

impl Page for AdvancedPage {
    fn title(&self) -> &str {
        "Advanced workload"
    }

    fn description(&self) -> &str {
        "Optional knobs most users don't need. Write pattern controls the bytes \
        written to disk (matters for compression / dedup tests). Think time \
        adds an inter-IO delay to simulate application work. Fadvise/madvise \
        give the kernel cache hints. Leave defaults to skip."
    }

    fn keybindings(&self) -> &str {
        "Tab/Enter: next field · ←/→ on cyclers · Space: toggle"
    }

    fn render(&self, frame: &mut Frame, area: Rect, _state: &WizardState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // header: write
                Constraint::Length(1), // write pattern cycler
                Constraint::Length(1), // header: think time
                Constraint::Length(3), // think_time text
                Constraint::Length(1), // think mode cycler
                Constraint::Length(3), // think_every
                Constraint::Length(3), // think_adaptive
                Constraint::Length(1), // header: fadvise
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1), // header: madvise
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(area);

        let render_header = |frame: &mut Frame, rect: Rect, label: &str| {
            let p = Paragraph::new(format!("── {} ──", label))
                .style(Style::default().fg(Color::DarkGray));
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
            let p = Paragraph::new(format!("  {} : < {} >", label, value)).style(style);
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

        let target_opt = _state.config.targets.first();
        let fadvise = target_opt.map(|t| &t.fadvise_flags);
        let madvise = target_opt.map(|t| &t.madvise_flags);

        let mut idx = 0;
        render_header(frame, chunks[idx], "Write pattern");
        idx += 1;
        render_cycler(
            frame,
            chunks[idx],
            Focus::WritePattern,
            "Pattern",
            WRITE_PATTERNS[self.write_pattern_idx].1,
        );
        idx += 1;

        render_header(frame, chunks[idx], "Think time (inter-IO delay)");
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::ThinkTime,
            "Think time per IO (optional)",
            &self.think_time,
            "(blank = no delay) e.g. 100us, 1ms, 10ms",
        );
        idx += 1;
        render_cycler(
            frame,
            chunks[idx],
            Focus::ThinkMode,
            "Think mode",
            THINK_MODES[self.think_mode_idx].1,
        );
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::ThinkEvery,
            "Apply every N blocks",
            &self.think_every,
            "default = 1",
        );
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::ThinkAdaptive,
            "Adaptive % of IO latency (0-100)",
            &self.think_adaptive,
            "(blank = fixed) e.g. 50",
        );
        idx += 1;

        render_header(frame, chunks[idx], "fadvise hints (file-scoped)");
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::FadviseSequential,
            "POSIX_FADV_SEQUENTIAL",
            fadvise.map(|f| f.sequential).unwrap_or(false),
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::FadviseRandom,
            "POSIX_FADV_RANDOM",
            fadvise.map(|f| f.random).unwrap_or(false),
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::FadviseWillneed,
            "POSIX_FADV_WILLNEED",
            fadvise.map(|f| f.willneed).unwrap_or(false),
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::FadviseDontneed,
            "POSIX_FADV_DONTNEED",
            fadvise.map(|f| f.dontneed).unwrap_or(false),
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::FadviseNoreuse,
            "POSIX_FADV_NOREUSE",
            fadvise.map(|f| f.noreuse).unwrap_or(false),
        );
        idx += 1;

        render_header(frame, chunks[idx], "madvise hints (mmap engine only)");
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::MadviseSequential,
            "MADV_SEQUENTIAL",
            madvise.map(|m| m.sequential).unwrap_or(false),
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::MadviseRandom,
            "MADV_RANDOM",
            madvise.map(|m| m.random).unwrap_or(false),
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::MadviseWillneed,
            "MADV_WILLNEED",
            madvise.map(|m| m.willneed).unwrap_or(false),
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::MadviseDontneed,
            "MADV_DONTNEED",
            madvise.map(|m| m.dontneed).unwrap_or(false),
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::MadviseHugepage,
            "MADV_HUGEPAGE",
            madvise.map(|m| m.hugepage).unwrap_or(false),
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::MadviseNohugepage,
            "MADV_NOHUGEPAGE",
            madvise.map(|m| m.nohugepage).unwrap_or(false),
        );
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut WizardState) -> PageOutcome {
        match key.code {
            KeyCode::Tab | KeyCode::Down => {
                self.focus = self.next_focus();
                PageOutcome::Stay
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.focus = self.prev_focus();
                PageOutcome::Stay
            }
            KeyCode::Char(' ') if Self::is_toggle(self.focus) => {
                self.toggle(self.focus, state);
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Left | KeyCode::Right if Self::is_cycler(self.focus) => {
                self.cycle(self.focus, key.code == KeyCode::Right);
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Enter => {
                self.flush_to_state(state);
                state.dirty = true;
                if self.focus == Focus::MadviseNohugepage {
                    PageOutcome::Next
                } else {
                    self.focus = self.next_focus();
                    PageOutcome::Stay
                }
            }
            KeyCode::Esc => PageOutcome::Quit,
            _ if Self::is_text_field(self.focus) => {
                let target = match self.focus {
                    Focus::ThinkTime => &mut self.think_time,
                    Focus::ThinkEvery => &mut self.think_every,
                    Focus::ThinkAdaptive => &mut self.think_adaptive,
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
        self.write_pattern_idx = WRITE_PATTERNS
            .iter()
            .position(|(p, _)| *p == state.config.workload.write_pattern)
            .unwrap_or(0);
        if let Some(tt) = &state.config.workload.think_time {
            self.think_time = Input::default().with_value(format!("{}us", tt.duration_us));
            self.think_mode_idx = THINK_MODES
                .iter()
                .position(|(m, _)| *m == tt.mode)
                .unwrap_or(0);
            self.think_every = Input::default().with_value(format!("{}", tt.apply_every_n_blocks));
            if let Some(a) = tt.adaptive_percent {
                self.think_adaptive = Input::default().with_value(format!("{}", a));
            }
        }
    }

    fn validate(&self, _state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let tt = self.think_time.value();
        if !tt.is_empty() && parse_time_us(tt).is_err() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!("Invalid think time: {} (use e.g. 100us, 1ms)", tt),
            });
        }
        let ta = self.think_adaptive.value();
        if !ta.is_empty() && ta.parse::<u8>().ok().filter(|n| *n <= 100).is_none() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!("Adaptive percent must be 0-100 (got '{}')", ta),
            });
        }
        let te = self.think_every.value();
        if !te.is_empty() && te.parse::<usize>().ok().filter(|n| *n > 0).is_none() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!("Think-every must be a positive integer (got '{}')", te),
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

    fn type_string(page: &mut AdvancedPage, state: &mut WizardState, s: &str) {
        for ch in s.chars() {
            page.handle_key(key(KeyCode::Char(ch)), state);
        }
    }

    #[test]
    fn test_advanced_default_write_pattern_random() {
        let page = AdvancedPage::new(5);
        assert_eq!(WRITE_PATTERNS[page.write_pattern_idx].0, VerifyPattern::Random);
    }

    #[test]
    fn test_advanced_cycle_write_pattern() {
        let mut page = AdvancedPage::new(5);
        let mut state = WizardState::new();
        page.handle_key(key(KeyCode::Right), &mut state);
        assert_eq!(WRITE_PATTERNS[page.write_pattern_idx].0, VerifyPattern::Zeros);
        assert_eq!(state.config.workload.write_pattern, VerifyPattern::Zeros);
    }

    #[test]
    fn test_advanced_think_time_creates_config() {
        let mut page = AdvancedPage::new(5);
        let mut state = WizardState::new();
        // Tab to ThinkTime.
        page.handle_key(key(KeyCode::Tab), &mut state);
        assert_eq!(page.focus, Focus::ThinkTime);
        type_string(&mut page, &mut state, "100us");
        assert!(state.config.workload.think_time.is_some());
        assert_eq!(state.config.workload.think_time.as_ref().unwrap().duration_us, 100);
    }

    #[test]
    fn test_advanced_fadvise_toggle() {
        let mut page = AdvancedPage::new(5);
        let mut state = WizardState::new();
        // Tab past write pattern + 4 think fields = 5 tabs to FadviseSequential.
        for _ in 0..5 {
            page.handle_key(key(KeyCode::Tab), &mut state);
        }
        assert_eq!(page.focus, Focus::FadviseSequential);
        page.handle_key(key(KeyCode::Char(' ')), &mut state);
        assert!(state.config.targets[0].fadvise_flags.sequential);
    }
}
