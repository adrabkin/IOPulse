//! Output page: where to write results, what to print, and runtime debug knobs.

use crate::config::cli_convert::parse_duration;
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{Severity, ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::path::PathBuf;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    JsonPath,
    JsonName,
    JsonInterval,
    JsonHistogram,
    PerWorker,
    NoAggregate,
    CsvPath,
    LiveInterval,
    NoLive,
    ShowLatency,
    ShowPercentiles,
    ShowHistogram,
    Heatmap,
    HeatmapBuckets,
    Prometheus,
    PrometheusPort,
    Verbosity,
    Debug,
    DryRun,
}

const FOCUS_ORDER: &[Focus] = &[
    Focus::JsonPath,
    Focus::JsonName,
    Focus::JsonInterval,
    Focus::JsonHistogram,
    Focus::PerWorker,
    Focus::NoAggregate,
    Focus::CsvPath,
    Focus::LiveInterval,
    Focus::NoLive,
    Focus::ShowLatency,
    Focus::ShowPercentiles,
    Focus::ShowHistogram,
    Focus::Heatmap,
    Focus::HeatmapBuckets,
    Focus::Prometheus,
    Focus::PrometheusPort,
    Focus::Verbosity,
    Focus::Debug,
    Focus::DryRun,
];

#[derive(Debug)]
pub struct OutputPage {
    json_path: Input,
    json_name: Input,
    json_interval: Input,
    csv_path: Input,
    live_interval: Input,
    heatmap_buckets: Input,
    prometheus_port: Input,
    verbosity: Input,
    focus: Focus,
    pub page_index: usize,
}

impl OutputPage {
    pub fn new(page_index: usize) -> Self {
        Self {
            json_path: Input::default(),
            json_name: Input::default(),
            json_interval: Input::default(),
            csv_path: Input::default(),
            live_interval: Input::default(),
            heatmap_buckets: Input::default(),
            prometheus_port: Input::default(),
            verbosity: Input::default(),
            focus: Focus::JsonPath,
            page_index,
        }
    }

    fn flush_to_state(&self, state: &mut WizardState) {
        let json = self.json_path.value();
        state.config.output.json_output = if json.is_empty() {
            None
        } else {
            Some(PathBuf::from(json))
        };
        if !self.json_name.value().is_empty() {
            state.config.output.json_name = self.json_name.value().to_string();
        }
        let ji = self.json_interval.value();
        state.config.output.json_interval = if ji.is_empty() {
            None
        } else {
            parse_duration(ji).ok()
        };
        let csv = self.csv_path.value();
        state.config.output.csv_output = if csv.is_empty() {
            None
        } else {
            Some(PathBuf::from(csv))
        };
        let live = self.live_interval.value();
        state.config.output.live_interval = if live.is_empty() {
            None
        } else {
            parse_duration(live).ok()
        };
        if let Ok(b) = self.heatmap_buckets.value().parse::<usize>() {
            state.config.workload.heatmap_buckets = b;
        }
        if let Ok(p) = self.prometheus_port.value().parse::<u16>() {
            state.config.output.prometheus_port = p;
        }
        if let Ok(v) = self.verbosity.value().parse::<u8>() {
            state.config.output.verbosity = v;
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

    /// HeatmapBuckets is only focusable when heatmap is on; PrometheusPort
    /// only when prometheus is on.
    fn is_focusable(&self, focus: Focus, state: &WizardState) -> bool {
        match focus {
            Focus::HeatmapBuckets => state.config.workload.heatmap,
            Focus::PrometheusPort => state.config.output.prometheus,
            _ => true,
        }
    }

    fn is_text_field(focus: Focus) -> bool {
        matches!(
            focus,
            Focus::JsonPath
                | Focus::JsonName
                | Focus::JsonInterval
                | Focus::CsvPath
                | Focus::LiveInterval
                | Focus::HeatmapBuckets
                | Focus::PrometheusPort
                | Focus::Verbosity
        )
    }

    fn is_toggle(focus: Focus) -> bool {
        matches!(
            focus,
            Focus::JsonHistogram
                | Focus::PerWorker
                | Focus::NoAggregate
                | Focus::NoLive
                | Focus::ShowLatency
                | Focus::ShowPercentiles
                | Focus::ShowHistogram
                | Focus::Heatmap
                | Focus::Prometheus
                | Focus::Debug
                | Focus::DryRun
        )
    }

    fn toggle(&self, focus: Focus, state: &mut WizardState) {
        match focus {
            Focus::JsonHistogram => state.config.output.json_histogram ^= true,
            Focus::PerWorker => state.config.output.per_worker_output ^= true,
            Focus::NoAggregate => state.config.output.no_aggregate ^= true,
            Focus::NoLive => state.config.output.no_live ^= true,
            Focus::ShowLatency => state.config.output.show_latency ^= true,
            Focus::ShowPercentiles => state.config.output.show_percentiles ^= true,
            Focus::ShowHistogram => state.config.output.show_histogram ^= true,
            Focus::Heatmap => state.config.workload.heatmap ^= true,
            Focus::Prometheus => state.config.output.prometheus ^= true,
            Focus::Debug => state.config.runtime.debug ^= true,
            Focus::DryRun => state.config.runtime.dry_run ^= true,
            _ => {}
        }
    }
}

impl Page for OutputPage {
    fn title(&self) -> &str {
        "Output"
    }

    fn description(&self) -> &str {
        "Where results land + which extras to compute. All file paths are \
        optional. Toggles add latency analysis, heatmaps, and Prometheus \
        metrics. Debug prints timing diagnostics; Dry-run validates the \
        config and exits without doing IO."
    }

    fn keybindings(&self) -> &str {
        "Tab/Enter: next field · Space: toggle checkbox"
    }

    fn render(&self, frame: &mut Frame, area: Rect, state: &WizardState) {
        let heatmap_on = state.config.workload.heatmap;
        let prom_on = state.config.output.prometheus;

        let mut constraints = vec![
            // JSON section
            Constraint::Length(1), // header
            Constraint::Length(3), // json path
            Constraint::Length(3), // json name
            Constraint::Length(3), // json interval
            Constraint::Length(1), // json histogram toggle
            Constraint::Length(1), // per-worker toggle
            Constraint::Length(1), // no-aggregate toggle
            // CSV
            Constraint::Length(1), // header
            Constraint::Length(3), // csv path
            // Live
            Constraint::Length(1), // header
            Constraint::Length(3), // live interval
            Constraint::Length(1), // no-live toggle
            // Latency stats
            Constraint::Length(1), // header
            Constraint::Length(1), // show latency
            Constraint::Length(1), // show percentiles
            Constraint::Length(1), // show histogram
            // Heatmap
            Constraint::Length(1), // header
            Constraint::Length(1), // heatmap toggle
        ];
        if heatmap_on {
            constraints.push(Constraint::Length(3)); // heatmap buckets
        }
        // Prometheus
        constraints.push(Constraint::Length(1)); // header
        constraints.push(Constraint::Length(1)); // prometheus toggle
        if prom_on {
            constraints.push(Constraint::Length(3)); // prom port
        }
        // Runtime
        constraints.push(Constraint::Length(1)); // header
        constraints.push(Constraint::Length(3)); // verbosity
        constraints.push(Constraint::Length(1)); // debug
        constraints.push(Constraint::Length(1)); // dry-run
        constraints.push(Constraint::Min(0));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        let mut idx = 0;

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
            let para = Paragraph::new(format!("  {} {}", mark, label)).style(style);
            frame.render_widget(para, rect);
        };

        let render_header = |frame: &mut Frame, rect: Rect, label: &str| {
            let para = Paragraph::new(format!("── {} ──", label))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(para, rect);
        };

        // JSON section
        render_header(frame, chunks[idx], "JSON output");
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::JsonPath,
            "JSON file path (optional)",
            &self.json_path,
            "(blank = no JSON) e.g. /tmp/results.json",
        );
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::JsonName,
            "JSON aggregate name",
            &self.json_name,
            "default = aggregate",
        );
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::JsonInterval,
            "JSON time-series interval (optional)",
            &self.json_interval,
            "(blank = single end-of-run) e.g. 1s",
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::JsonHistogram,
            "Write separate histogram file (all 112 buckets)",
            state.config.output.json_histogram,
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::PerWorker,
            "Include per-worker stats in time-series",
            state.config.output.per_worker_output,
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::NoAggregate,
            "Skip aggregate file (distributed runs only)",
            state.config.output.no_aggregate,
        );
        idx += 1;

        // CSV section
        render_header(frame, chunks[idx], "CSV output");
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::CsvPath,
            "CSV file path (optional)",
            &self.csv_path,
            "(blank = no CSV) e.g. /tmp/results.csv",
        );
        idx += 1;

        // Live section
        render_header(frame, chunks[idx], "Live updates");
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::LiveInterval,
            "Live update interval (optional)",
            &self.live_interval,
            "(blank = end-of-run only) e.g. 1s, 500ms",
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::NoLive,
            "Disable live stats entirely",
            state.config.output.no_live,
        );
        idx += 1;

        // Latency stats
        render_header(frame, chunks[idx], "Latency stats");
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::ShowLatency,
            "Show latency stats (min / mean / max)",
            state.config.output.show_latency,
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::ShowPercentiles,
            "Show latency percentiles (p50, p99, p99.9)",
            state.config.output.show_percentiles,
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::ShowHistogram,
            "Show full latency histogram",
            state.config.output.show_histogram,
        );
        idx += 1;

        // Heatmap section
        render_header(frame, chunks[idx], "Heatmap");
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::Heatmap,
            "Block access heatmap (5-10% overhead)",
            heatmap_on,
        );
        idx += 1;
        if heatmap_on {
            render_text(
                frame,
                chunks[idx],
                Focus::HeatmapBuckets,
                "Heatmap buckets",
                &self.heatmap_buckets,
                "default = 100",
            );
            idx += 1;
        }

        // Prometheus
        render_header(frame, chunks[idx], "Prometheus");
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::Prometheus,
            "Expose Prometheus metrics endpoint",
            prom_on,
        );
        idx += 1;
        if prom_on {
            render_text(
                frame,
                chunks[idx],
                Focus::PrometheusPort,
                "Prometheus port",
                &self.prometheus_port,
                "default = 9090",
            );
            idx += 1;
        }

        // Runtime
        render_header(frame, chunks[idx], "Runtime");
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::Verbosity,
            "Verbosity (0-3)",
            &self.verbosity,
            "default = 0 (quiet)",
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::Debug,
            "Debug output (timing, file ops)",
            state.config.runtime.debug,
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::DryRun,
            "Dry-run (validate config, don't execute)",
            state.config.runtime.dry_run,
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
            KeyCode::Char(' ') if Self::is_toggle(self.focus) => {
                self.toggle(self.focus, state);
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Enter => {
                self.flush_to_state(state);
                state.dirty = true;
                if self.focus == Focus::DryRun {
                    PageOutcome::Next
                } else {
                    self.focus = self.next_focus(state);
                    PageOutcome::Stay
                }
            }
            KeyCode::Esc => PageOutcome::Quit,
            _ if Self::is_text_field(self.focus) => {
                let target = match self.focus {
                    Focus::JsonPath => &mut self.json_path,
                    Focus::JsonName => &mut self.json_name,
                    Focus::JsonInterval => &mut self.json_interval,
                    Focus::CsvPath => &mut self.csv_path,
                    Focus::LiveInterval => &mut self.live_interval,
                    Focus::HeatmapBuckets => &mut self.heatmap_buckets,
                    Focus::PrometheusPort => &mut self.prometheus_port,
                    Focus::Verbosity => &mut self.verbosity,
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
        if let Some(p) = &state.config.output.json_output {
            self.json_path = Input::default().with_value(p.to_string_lossy().into_owned());
        }
        self.json_name = Input::default().with_value(state.config.output.json_name.clone());
        if let Some(s) = state.config.output.json_interval {
            self.json_interval = Input::default().with_value(format!("{}s", s));
        }
        if let Some(p) = &state.config.output.csv_output {
            self.csv_path = Input::default().with_value(p.to_string_lossy().into_owned());
        }
        if let Some(s) = state.config.output.live_interval {
            self.live_interval = Input::default().with_value(format!("{}s", s));
        }
        self.heatmap_buckets =
            Input::default().with_value(format!("{}", state.config.workload.heatmap_buckets));
        self.prometheus_port =
            Input::default().with_value(format!("{}", state.config.output.prometheus_port));
        self.verbosity =
            Input::default().with_value(format!("{}", state.config.output.verbosity));
    }

    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let live = self.live_interval.value();
        if !live.is_empty() && parse_duration(live).is_err() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!("Invalid live interval: {}", live),
            });
        }
        let ji = self.json_interval.value();
        if !ji.is_empty() && parse_duration(ji).is_err() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!("Invalid JSON interval: {}", ji),
            });
        }
        if state.config.workload.heatmap {
            let b = self.heatmap_buckets.value();
            if !b.is_empty() && b.parse::<usize>().ok().filter(|n| *n > 0).is_none() {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    page: self.page_index,
                    message: format!("Heatmap buckets must be a positive integer (got '{}')", b),
                });
            }
        }
        if state.config.output.prometheus {
            let p = self.prometheus_port.value();
            if !p.is_empty() && p.parse::<u16>().ok().filter(|n| *n > 0).is_none() {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    page: self.page_index,
                    message: format!("Prometheus port must be 1-65535 (got '{}')", p),
                });
            }
        }
        let v = self.verbosity.value();
        if !v.is_empty() && v.parse::<u8>().ok().filter(|n| *n <= 3).is_none() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!("Verbosity must be 0-3 (got '{}')", v),
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

    fn type_string(page: &mut OutputPage, state: &mut WizardState, s: &str) {
        for ch in s.chars() {
            page.handle_key(key(KeyCode::Char(ch)), state);
        }
    }

    fn tab_to(page: &mut OutputPage, state: &mut WizardState, target: Focus) {
        for _ in 0..50 {
            if page.focus == target {
                return;
            }
            page.handle_key(key(KeyCode::Tab), state);
        }
        panic!("never reached focus {:?}", target);
    }

    #[test]
    fn test_output_default_all_fields_empty() {
        let page = OutputPage::new(8);
        let state = WizardState::new();
        assert!(state.config.output.json_output.is_none());
        assert!(state.config.output.csv_output.is_none());
        assert!(page.validate(&state).is_empty());
    }

    #[test]
    fn test_output_json_path_set() {
        let mut page = OutputPage::new(8);
        let mut state = WizardState::new();
        type_string(&mut page, &mut state, "/tmp/run.json");
        assert_eq!(
            state.config.output.json_output,
            Some(PathBuf::from("/tmp/run.json"))
        );
    }

    #[test]
    fn test_output_invalid_live_interval_validates() {
        let mut page = OutputPage::new(8);
        let mut state = WizardState::new();
        tab_to(&mut page, &mut state, Focus::LiveInterval);
        type_string(&mut page, &mut state, "abc");
        let issues = page.validate(&state);
        assert!(issues.iter().any(|i| i.message.contains("Invalid live interval")));
    }

    #[test]
    fn test_output_space_toggles_show_latency() {
        let mut page = OutputPage::new(8);
        let mut state = WizardState::new();
        tab_to(&mut page, &mut state, Focus::ShowLatency);
        assert!(!state.config.output.show_latency);
        page.handle_key(key(KeyCode::Char(' ')), &mut state);
        assert!(state.config.output.show_latency);
    }

    #[test]
    fn test_output_heatmap_reveals_buckets() {
        let mut page = OutputPage::new(8);
        let mut state = WizardState::new();
        tab_to(&mut page, &mut state, Focus::Heatmap);
        // Buckets is hidden when heatmap is off.
        assert!(!page.is_focusable(Focus::HeatmapBuckets, &state));
        page.handle_key(key(KeyCode::Char(' ')), &mut state);
        assert!(state.config.workload.heatmap);
        assert!(page.is_focusable(Focus::HeatmapBuckets, &state));
    }

    #[test]
    fn test_output_dry_run_toggle() {
        let mut page = OutputPage::new(8);
        let mut state = WizardState::new();
        tab_to(&mut page, &mut state, Focus::DryRun);
        assert!(!state.config.runtime.dry_run);
        page.handle_key(key(KeyCode::Char(' ')), &mut state);
        assert!(state.config.runtime.dry_run);
    }
}
