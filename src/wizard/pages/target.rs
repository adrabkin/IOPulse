//! Target page: where IOPulse reads/writes from + tree shape + file prep.
//!
//! Sections (top to bottom):
//!   1. Path + file size
//!   2. Optional directory tree (depth/width tree OR num_files/num_dirs flat)
//!   3. File prep toggles (preallocate, truncate, refill, no_refill) and
//!      refill_pattern selector when refill is on.

use crate::config::cli_convert::parse_size;
use crate::config::workload::VerifyPattern;
use crate::config::{LayoutConfig, NamingPattern};
use crate::wizard::pages::{Page, PageOutcome};
use crate::wizard::state::{Severity, ValidationIssue, WizardState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::path::PathBuf;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Path,
    FileSize,
    DirDepth,
    DirWidth,
    TotalFiles,
    NamingPattern,
    NumFiles,
    NumDirs,
    LayoutManifest,
    ExportLayoutManifest,
    Preallocate,
    TruncateToSize,
    Refill,
    RefillPattern,
    NoRefill,
}

const FOCUS_ORDER: &[Focus] = &[
    Focus::Path,
    Focus::FileSize,
    Focus::DirDepth,
    Focus::DirWidth,
    Focus::TotalFiles,
    Focus::NamingPattern,
    Focus::NumFiles,
    Focus::NumDirs,
    Focus::LayoutManifest,
    Focus::ExportLayoutManifest,
    Focus::Preallocate,
    Focus::TruncateToSize,
    Focus::Refill,
    Focus::RefillPattern,
    Focus::NoRefill,
];

const NAMING_PATTERNS: &[(NamingPattern, &str)] = &[
    (NamingPattern::Sequential, "sequential"),
    (NamingPattern::Random, "random"),
    (NamingPattern::Prefixed, "prefixed"),
];

const REFILL_PATTERNS: &[(VerifyPattern, &str)] = &[
    (VerifyPattern::Random, "random"),
    (VerifyPattern::Zeros, "zeros"),
    (VerifyPattern::Ones, "ones"),
    (VerifyPattern::Sequential, "sequential"),
];

#[derive(Debug)]
pub struct TargetPage {
    path: Input,
    file_size: Input,
    dir_depth: Input,
    dir_width: Input,
    total_files: Input,
    num_files: Input,
    num_dirs: Input,
    layout_manifest: Input,
    export_layout_manifest: Input,
    refill_pattern_idx: usize,
    naming_pattern_idx: usize,
    focus: Focus,
    pub page_index: usize,
}

impl TargetPage {
    pub fn new(page_index: usize) -> Self {
        Self {
            path: Input::default(),
            file_size: Input::default(),
            dir_depth: Input::default(),
            dir_width: Input::default(),
            total_files: Input::default(),
            num_files: Input::default(),
            num_dirs: Input::default(),
            layout_manifest: Input::default(),
            export_layout_manifest: Input::default(),
            refill_pattern_idx: 0,
            naming_pattern_idx: 0,
            focus: Focus::Path,
            page_index,
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

    /// RefillPattern is only focusable when refill is on (otherwise the
    /// selector is hidden).
    fn is_focusable(&self, focus: Focus, state: &WizardState) -> bool {
        match focus {
            Focus::RefillPattern => state
                .config
                .targets
                .first()
                .map(|t| t.refill)
                .unwrap_or(false),
            _ => true,
        }
    }

    fn is_text_field(focus: Focus) -> bool {
        matches!(
            focus,
            Focus::Path
                | Focus::FileSize
                | Focus::DirDepth
                | Focus::DirWidth
                | Focus::TotalFiles
                | Focus::NumFiles
                | Focus::NumDirs
                | Focus::LayoutManifest
                | Focus::ExportLayoutManifest
        )
    }

    fn is_toggle(focus: Focus) -> bool {
        matches!(
            focus,
            Focus::Preallocate | Focus::TruncateToSize | Focus::Refill | Focus::NoRefill
        )
    }

    fn toggle(&self, focus: Focus, state: &mut WizardState) {
        if let Some(target) = state.config.targets.first_mut() {
            match focus {
                Focus::Preallocate => target.preallocate ^= true,
                Focus::TruncateToSize => target.truncate_to_size ^= true,
                Focus::Refill => target.refill ^= true,
                Focus::NoRefill => target.no_refill ^= true,
                _ => {}
            }
        }
    }

    fn compute_tree_math(&self) -> Option<TreeMath> {
        let depth: usize = self.dir_depth.value().parse().ok()?;
        let width: usize = self.dir_width.value().parse().ok()?;
        if depth == 0 || width == 0 {
            return None;
        }
        let mut dirs_with_files: usize = 0;
        for level in 1..=depth {
            dirs_with_files = dirs_with_files.checked_add(width.checked_pow(level as u32)?)?;
        }
        let total_files = self.total_files.value().parse::<usize>().ok();
        let files_per_dir = total_files
            .map(|t| t.div_ceil(dirs_with_files.max(1)))
            .unwrap_or(1);
        let actual_total = total_files.unwrap_or(files_per_dir * dirs_with_files);
        Some(TreeMath {
            dirs_with_files,
            files_per_dir,
            total_files: actual_total,
        })
    }

    fn flush_to_state(&self, state: &mut WizardState) {
        if let Some(target) = state.config.targets.first_mut() {
            target.path = PathBuf::from(self.path.value());
            target.file_size = parse_size(self.file_size.value()).ok();

            // Tree fields → LayoutConfig
            let depth = self.dir_depth.value().parse::<usize>().ok();
            let width = self.dir_width.value().parse::<usize>().ok();
            let total = self.total_files.value().parse::<usize>().ok();
            let naming = NAMING_PATTERNS[self.naming_pattern_idx].0;

            if let (Some(d), Some(w)) = (depth, width) {
                if d > 0 && w > 0 {
                    let math = self.compute_tree_math();
                    let files_per_dir = math.as_ref().map(|m| m.files_per_dir).unwrap_or(1);
                    target.layout_config = Some(LayoutConfig {
                        depth: d,
                        width: w,
                        files_per_dir,
                        naming_pattern: naming,
                        num_workers: None,
                        total_files: total,
                    });
                } else {
                    target.layout_config = None;
                }
            } else {
                target.layout_config = None;
            }

            // Flat-tree fields
            target.num_files = self.num_files.value().parse::<usize>().ok();
            target.num_dirs = self.num_dirs.value().parse::<usize>().ok();

            // Layout manifest paths
            let lm = self.layout_manifest.value();
            target.layout_manifest = if lm.is_empty() {
                None
            } else {
                Some(PathBuf::from(lm))
            };
            let elm = self.export_layout_manifest.value();
            target.export_layout_manifest = if elm.is_empty() {
                None
            } else {
                Some(PathBuf::from(elm))
            };

            // Refill pattern
            target.refill_pattern = REFILL_PATTERNS[self.refill_pattern_idx].0;
        }
    }
}

#[derive(Debug)]
struct TreeMath {
    dirs_with_files: usize,
    files_per_dir: usize,
    total_files: usize,
}

impl Page for TargetPage {
    fn title(&self) -> &str {
        "Target"
    }

    fn description(&self) -> &str {
        "Where IOPulse reads/writes. Path + size for a single file. \
        For multi-file workloads, either set depth+width to generate a tree, \
        or set num-files / num-dirs for a flat layout (don't mix both). \
        File prep toggles control how files are created/filled before reads."
    }

    fn keybindings(&self) -> &str {
        "Tab/Enter: next field · Space: toggle checkbox"
    }

    fn render(&self, frame: &mut Frame, area: Rect, state: &WizardState) {
        let target = state.config.targets.first();
        let refill_on = target.map(|t| t.refill).unwrap_or(false);
        let preallocate = target.map(|t| t.preallocate).unwrap_or(false);
        let truncate = target.map(|t| t.truncate_to_size).unwrap_or(false);
        let no_refill = target.map(|t| t.no_refill).unwrap_or(false);

        let mut constraints = vec![
            Constraint::Length(3), // Path
            Constraint::Length(3), // FileSize
            Constraint::Length(1), // separator (tree)
            Constraint::Length(3), // DirDepth
            Constraint::Length(3), // DirWidth
            Constraint::Length(3), // TotalFiles
            Constraint::Length(1), // NamingPattern cycler
            Constraint::Length(2), // tree math
            Constraint::Length(1), // separator (flat)
            Constraint::Length(3), // NumFiles
            Constraint::Length(3), // NumDirs
            Constraint::Length(1), // separator (manifest)
            Constraint::Length(3), // LayoutManifest
            Constraint::Length(3), // ExportLayoutManifest
            Constraint::Length(1), // separator (file prep)
            Constraint::Length(1), // Preallocate
            Constraint::Length(1), // TruncateToSize
            Constraint::Length(1), // Refill
        ];
        if refill_on {
            constraints.push(Constraint::Length(1)); // RefillPattern
        }
        constraints.push(Constraint::Length(1)); // NoRefill
        constraints.push(Constraint::Min(0));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Indices into chunks.
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

        render_text(
            frame,
            chunks[idx],
            Focus::Path,
            "Path (required)",
            &self.path,
            "e.g. /tmp/iopulse_test  or  /dev/nvme0n1",
        );
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::FileSize,
            "File size",
            &self.file_size,
            "e.g. 1G, 512M, 100K",
        );
        idx += 1;

        let separator = Paragraph::new("── Optional: directory tree (depth/width) ──")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(separator, chunks[idx]);
        idx += 1;

        render_text(
            frame,
            chunks[idx],
            Focus::DirDepth,
            "Directory depth",
            &self.dir_depth,
            "(blank = no tree) e.g. 2",
        );
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::DirWidth,
            "Subdirectories per level",
            &self.dir_width,
            "(blank = no tree) e.g. 4",
        );
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::TotalFiles,
            "Total files",
            &self.total_files,
            "(blank = 1 file per leaf dir) e.g. 1000",
        );
        idx += 1;

        // Naming pattern cycler.
        let naming_label = NAMING_PATTERNS[self.naming_pattern_idx].1;
        let naming_style = if self.focus == Focus::NamingPattern {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        let naming_widget = Paragraph::new(format!(
            "  Naming pattern: < {} >  (←/→ to cycle)",
            naming_label
        ))
        .style(naming_style);
        frame.render_widget(naming_widget, chunks[idx]);
        idx += 1;

        let math_text = match self.compute_tree_math() {
            Some(math) => format!(
                "→ Will create {} dirs, {} files/dir, {} files total.",
                math.dirs_with_files, math.files_per_dir, math.total_files
            ),
            None => "→ No tree; falling back to single file or flat layout below.".to_string(),
        };
        let math_widget = Paragraph::new(math_text)
            .style(Style::default().fg(Color::Yellow))
            .wrap(Wrap { trim: true });
        frame.render_widget(math_widget, chunks[idx]);
        idx += 1;

        let sep2 = Paragraph::new("── OR: flat layout (num-files / num-dirs) ──")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(sep2, chunks[idx]);
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::NumFiles,
            "Number of files",
            &self.num_files,
            "(blank = use tree above or single file) e.g. 100",
        );
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::NumDirs,
            "Number of directories",
            &self.num_dirs,
            "(blank = 1) e.g. 10",
        );
        idx += 1;

        let sep_lm =
            Paragraph::new("── Layout manifest (advanced) ──").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(sep_lm, chunks[idx]);
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::LayoutManifest,
            "Layout manifest input (load saved tree)",
            &self.layout_manifest,
            "(blank = generate fresh) e.g. /etc/iopulse/tree.toml",
        );
        idx += 1;
        render_text(
            frame,
            chunks[idx],
            Focus::ExportLayoutManifest,
            "Layout manifest export (save generated tree)",
            &self.export_layout_manifest,
            "(blank = no export) e.g. /etc/iopulse/tree.toml",
        );
        idx += 1;

        let sep3 = Paragraph::new("── File prep ──").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(sep3, chunks[idx]);
        idx += 1;

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

        render_check(
            frame,
            chunks[idx],
            Focus::Preallocate,
            "Preallocate file space (posix_fallocate, no fill)",
            preallocate,
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::TruncateToSize,
            "Truncate to size on creation",
            truncate,
        );
        idx += 1;
        render_check(
            frame,
            chunks[idx],
            Focus::Refill,
            "Fill files with pattern data before reads",
            refill_on,
        );
        idx += 1;

        if refill_on {
            // Refill pattern: a single-line cycler. Shows current value;
            // ↑/↓ or Space cycles through the four patterns.
            let pattern_label = REFILL_PATTERNS[self.refill_pattern_idx].1;
            let style = if self.focus == Focus::RefillPattern {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            let para = Paragraph::new(format!("    Refill pattern: < {} >", pattern_label)).style(style);
            frame.render_widget(para, chunks[idx]);
            idx += 1;
        }

        render_check(
            frame,
            chunks[idx],
            Focus::NoRefill,
            "Disable auto-fill for read tests (advanced)",
            no_refill,
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
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Left | KeyCode::Right if self.focus == Focus::RefillPattern => {
                if key.code == KeyCode::Right {
                    self.refill_pattern_idx =
                        (self.refill_pattern_idx + 1) % REFILL_PATTERNS.len();
                } else {
                    self.refill_pattern_idx = (self.refill_pattern_idx
                        + REFILL_PATTERNS.len()
                        - 1)
                        % REFILL_PATTERNS.len();
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Left | KeyCode::Right if self.focus == Focus::NamingPattern => {
                if key.code == KeyCode::Right {
                    self.naming_pattern_idx =
                        (self.naming_pattern_idx + 1) % NAMING_PATTERNS.len();
                } else {
                    self.naming_pattern_idx = (self.naming_pattern_idx
                        + NAMING_PATTERNS.len()
                        - 1)
                        % NAMING_PATTERNS.len();
                }
                self.flush_to_state(state);
                state.dirty = true;
                PageOutcome::Stay
            }
            KeyCode::Enter => {
                self.flush_to_state(state);
                state.dirty = true;
                if self.focus == Focus::NoRefill {
                    PageOutcome::Next
                } else {
                    self.focus = self.next_focus(state);
                    PageOutcome::Stay
                }
            }
            KeyCode::Esc => PageOutcome::Quit,
            _ if Self::is_text_field(self.focus) => {
                let target = match self.focus {
                    Focus::Path => &mut self.path,
                    Focus::FileSize => &mut self.file_size,
                    Focus::DirDepth => &mut self.dir_depth,
                    Focus::DirWidth => &mut self.dir_width,
                    Focus::TotalFiles => &mut self.total_files,
                    Focus::NumFiles => &mut self.num_files,
                    Focus::NumDirs => &mut self.num_dirs,
                    Focus::LayoutManifest => &mut self.layout_manifest,
                    Focus::ExportLayoutManifest => &mut self.export_layout_manifest,
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
        if let Some(target) = state.config.targets.first() {
            self.path = Input::default().with_value(target.path.to_string_lossy().into_owned());
            if let Some(size) = target.file_size {
                self.file_size = Input::default().with_value(format!("{}", size));
            }
            if let Some(layout) = &target.layout_config {
                self.dir_depth = Input::default().with_value(format!("{}", layout.depth));
                self.dir_width = Input::default().with_value(format!("{}", layout.width));
                if let Some(t) = layout.total_files {
                    self.total_files = Input::default().with_value(format!("{}", t));
                }
                self.naming_pattern_idx = NAMING_PATTERNS
                    .iter()
                    .position(|(p, _)| *p == layout.naming_pattern)
                    .unwrap_or(0);
            }
            if let Some(n) = target.num_files {
                self.num_files = Input::default().with_value(format!("{}", n));
            }
            if let Some(n) = target.num_dirs {
                self.num_dirs = Input::default().with_value(format!("{}", n));
            }
            if let Some(p) = &target.layout_manifest {
                self.layout_manifest =
                    Input::default().with_value(p.to_string_lossy().into_owned());
            }
            if let Some(p) = &target.export_layout_manifest {
                self.export_layout_manifest =
                    Input::default().with_value(p.to_string_lossy().into_owned());
            }
            self.refill_pattern_idx = REFILL_PATTERNS
                .iter()
                .position(|(p, _)| *p == target.refill_pattern)
                .unwrap_or(0);
        }
    }

    fn validate(&self, state: &WizardState) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let target = match state.config.targets.first() {
            Some(t) => t,
            None => return issues,
        };

        if target.path.as_os_str().is_empty() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: "Target path is required".to_string(),
            });
        }
        if !self.file_size.value().is_empty() && parse_size(self.file_size.value()).is_err() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!("Invalid file size: {}", self.file_size.value()),
            });
        }

        let depth_str = self.dir_depth.value();
        let width_str = self.dir_width.value();
        if !depth_str.is_empty() || !width_str.is_empty() {
            if depth_str.is_empty() || width_str.is_empty() {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    page: self.page_index,
                    message: "Set both depth and width, or leave both blank".to_string(),
                });
            }
            if !depth_str.is_empty() && depth_str.parse::<usize>().ok().filter(|n| *n > 0).is_none()
            {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    page: self.page_index,
                    message: format!("Depth must be a positive integer (got '{}')", depth_str),
                });
            }
            if !width_str.is_empty() && width_str.parse::<usize>().ok().filter(|n| *n > 0).is_none()
            {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    page: self.page_index,
                    message: format!("Width must be a positive integer (got '{}')", width_str),
                });
            }
        }
        let total_str = self.total_files.value();
        if !total_str.is_empty() && total_str.parse::<usize>().ok().filter(|n| *n > 0).is_none() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: format!("Total files must be a positive integer (got '{}')", total_str),
            });
        }

        // Don't mix tree + flat layouts.
        let tree_set = !self.dir_depth.value().is_empty() && !self.dir_width.value().is_empty();
        let flat_set =
            !self.num_files.value().is_empty() || !self.num_dirs.value().is_empty();
        if tree_set && flat_set {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                page: self.page_index,
                message: "Both tree (depth/width) and flat (num-files/dirs) are set; tree takes precedence".to_string(),
            });
        }

        // refill + no_refill is a contradiction.
        if target.refill && target.no_refill {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                page: self.page_index,
                message: "Cannot enable both 'fill files' and 'disable auto-fill' — pick one"
                    .to_string(),
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

    fn type_string(page: &mut TargetPage, state: &mut WizardState, s: &str) {
        for ch in s.chars() {
            page.handle_key(key(KeyCode::Char(ch)), state);
        }
    }

    #[test]
    fn test_target_default_focus_is_path() {
        let page = TargetPage::new(1);
        assert_eq!(page.focus, Focus::Path);
    }

    #[test]
    fn test_target_typed_path_lands_in_state() {
        let mut page = TargetPage::new(1);
        let mut state = WizardState::new();
        type_string(&mut page, &mut state, "/tmp/test");
        assert_eq!(state.config.targets[0].path, PathBuf::from("/tmp/test"));
    }

    #[test]
    fn test_target_tab_then_size_typing_parses_to_bytes() {
        let mut page = TargetPage::new(1);
        let mut state = WizardState::new();
        type_string(&mut page, &mut state, "/tmp/test");
        page.handle_key(key(KeyCode::Tab), &mut state);
        assert_eq!(page.focus, Focus::FileSize);
        type_string(&mut page, &mut state, "1G");
        assert_eq!(state.config.targets[0].file_size, Some(1024 * 1024 * 1024));
    }

    #[test]
    fn test_target_validate_errors_on_empty_path() {
        let page = TargetPage::new(1);
        let state = WizardState::new();
        let issues = page.validate(&state);
        assert!(issues
            .iter()
            .any(|i| i.severity == Severity::Error && i.message.contains("path")));
    }

    #[test]
    fn test_target_validate_errors_on_bad_size() {
        let mut page = TargetPage::new(1);
        let mut state = WizardState::new();
        type_string(&mut page, &mut state, "/tmp/test");
        page.handle_key(key(KeyCode::Tab), &mut state);
        type_string(&mut page, &mut state, "not-a-size");
        let issues = page.validate(&state);
        assert!(issues.iter().any(|i| i.message.contains("Invalid file size")));
    }

    /// Helper: Tab until focus reaches the target. Bounded by FOCUS_ORDER.len().
    fn tab_to(page: &mut TargetPage, state: &mut WizardState, target: Focus) {
        for _ in 0..FOCUS_ORDER.len() + 4 {
            if page.focus == target {
                return;
            }
            page.handle_key(key(KeyCode::Tab), state);
        }
        panic!("never reached focus {:?}", target);
    }

    #[test]
    fn test_target_tree_math_simple() {
        let mut page = TargetPage::new(1);
        let mut state = WizardState::new();
        type_string(&mut page, &mut state, "/tmp/test");
        tab_to(&mut page, &mut state, Focus::DirDepth);
        type_string(&mut page, &mut state, "2");
        page.handle_key(key(KeyCode::Tab), &mut state);
        type_string(&mut page, &mut state, "3");
        let math = page.compute_tree_math().expect("math computes");
        assert_eq!(math.dirs_with_files, 3 + 9);
        assert_eq!(math.files_per_dir, 1);
        assert_eq!(math.total_files, 12);
    }

    #[test]
    fn test_target_preallocate_toggle() {
        let mut page = TargetPage::new(1);
        let mut state = WizardState::new();
        tab_to(&mut page, &mut state, Focus::Preallocate);
        assert!(!state.config.targets[0].preallocate);
        page.handle_key(key(KeyCode::Char(' ')), &mut state);
        assert!(state.config.targets[0].preallocate);
    }

    #[test]
    fn test_target_refill_reveals_pattern_field() {
        let mut page = TargetPage::new(1);
        let mut state = WizardState::new();
        tab_to(&mut page, &mut state, Focus::Refill);
        let next = page.next_focus(&state);
        assert_eq!(next, Focus::NoRefill);
        page.handle_key(key(KeyCode::Char(' ')), &mut state);
        assert!(state.config.targets[0].refill);
        let next = page.next_focus(&state);
        assert_eq!(next, Focus::RefillPattern);
    }

    #[test]
    fn test_target_num_files_lands_in_state() {
        let mut page = TargetPage::new(1);
        let mut state = WizardState::new();
        type_string(&mut page, &mut state, "/tmp/test");
        tab_to(&mut page, &mut state, Focus::NumFiles);
        type_string(&mut page, &mut state, "100");
        assert_eq!(state.config.targets[0].num_files, Some(100));
    }

    #[test]
    fn test_target_naming_pattern_cycle() {
        let mut page = TargetPage::new(1);
        let mut state = WizardState::new();
        // Need a tree first so the pattern actually lands in layout_config.
        type_string(&mut page, &mut state, "/tmp/test");
        tab_to(&mut page, &mut state, Focus::DirDepth);
        type_string(&mut page, &mut state, "1");
        page.handle_key(key(KeyCode::Tab), &mut state);
        type_string(&mut page, &mut state, "2");
        // Tab to NamingPattern (after TotalFiles).
        tab_to(&mut page, &mut state, Focus::NamingPattern);
        page.handle_key(key(KeyCode::Right), &mut state);
        let layout = state.config.targets[0]
            .layout_config
            .as_ref()
            .expect("layout exists");
        assert_eq!(layout.naming_pattern, NamingPattern::Random);
    }

    #[test]
    fn test_target_layout_manifest_lands_in_state() {
        let mut page = TargetPage::new(1);
        let mut state = WizardState::new();
        type_string(&mut page, &mut state, "/tmp/test");
        tab_to(&mut page, &mut state, Focus::LayoutManifest);
        type_string(&mut page, &mut state, "/etc/iopulse/tree.toml");
        assert_eq!(
            state.config.targets[0].layout_manifest.as_deref(),
            Some(std::path::Path::new("/etc/iopulse/tree.toml"))
        );
    }
}
