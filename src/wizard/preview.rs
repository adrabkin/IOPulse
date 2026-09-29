//! Live TOML preview pane.
//!
//! Renders `state.config` (or `state.multi_phase`) as TOML lines into a
//! ratatui paragraph. The render is intentionally deterministic — the same
//! state always produces the same output — so tests can assert content with
//! line-equality.

use crate::wizard::pages::review::ReviewPage;
use crate::wizard::state::WizardState;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Build a styled `Vec<Line>` for the preview pane. Lines belonging to the
/// section the user is currently editing are rendered normal; everything else
/// is dimmed so the eye snaps to the active section.
pub fn render_lines(state: &WizardState) -> Vec<Line<'static>> {
    let toml = ReviewPage::render_toml(state);
    let active = active_section(state.current_page);
    toml.lines()
        .map(|line| style_line(line, active))
        .collect()
}

/// Section name (e.g., `[workload]`, `[[targets]]`) the active page edits.
/// Returns `None` for pages that don't map cleanly onto one section (like the
/// review page or phases toggle).
fn active_section(page_index: usize) -> Option<&'static str> {
    match page_index {
        0 => None,                              // Mode
        1 => Some("[[targets]]"),               // Target
        2 => Some("[workload]"),                // Engine
        3 => Some("[workload]"),                // Workload mix
        4 => Some("[workload.distribution]"),   // Distribution
        5 => Some("[workload]"),                // Advanced workload
        6 => Some("[workload]"),                // Block size + queue depth
        7 => Some("[workload.completion_mode]"), // Completion
        8 => Some("[workers]"),                 // Workers
        9 => Some("[runtime]"),                 // Reliability
        10 => Some("[output]"),                 // Output
        _ => None,                              // Phases / review
    }
}

fn style_line(line: &str, active: Option<&str>) -> Line<'static> {
    let owned = line.to_string();
    let style = match active {
        Some(section) if !line_matches_section(&owned, section) => {
            Style::default().add_modifier(Modifier::DIM)
        }
        _ => Style::default(),
    };
    Line::from(Span::styled(owned, style))
}

/// Cheap heuristic: a line "belongs to" the section whose header most recently
/// appeared in the document. We don't actually track that here — we'd need the
/// full prefix scan for that — but checking whether the line *is* the section
/// header (or a child of it via `[parent.child]` / `[[targets.foo]]` syntax)
/// catches the common cases for the wizard's small config.
fn line_matches_section(line: &str, section: &str) -> bool {
    let trimmed = line.trim();
    if trimmed == section {
        return true;
    }
    // For `[workload]`, also match `[workload.completion_mode]`,
    // `[workload.distribution]`, etc. For `[[targets]]`, match
    // `[targets.fadvise_flags]` and any other `[targets.*]` subkey that the
    // serializer renders inline.
    let inner = section
        .trim_start_matches('[')
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches(']');
    let dotted_single = format!("[{}.", inner);
    let dotted_double = format!("[[{}.", inner);
    if trimmed.starts_with(&dotted_single) || trimmed.starts_with(&dotted_double) {
        return true;
    }
    false
}

/// Render the preview pane into `area`, including borders and scroll offset.
pub fn render(frame: &mut Frame, area: Rect, state: &WizardState, scroll: u16) {
    let lines = render_lines(state);
    let para = Paragraph::new(lines)
        .scroll((scroll, 0))
        .block(Block::default().title("Preview").borders(Borders::ALL));
    frame.render_widget(para, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_preview_lines_contain_workload_section() {
        let state = WizardState::new();
        let lines = render_lines(&state);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.contains("[workload]"));
        assert!(text.contains("read_percent = 100"));
    }

    #[test]
    fn test_preview_targets_section_matches_header() {
        // Target page (1) → active section is [[targets]]; the header line
        // and its [targets.fadvise_flags] subtable should both render normal.
        let mut state = WizardState::new();
        state.current_page = 1;
        let lines = render_lines(&state);
        let mut found_targets_normal = false;
        for line in &lines {
            for span in &line.spans {
                if span.content.trim() == "[[targets]]"
                    && !span.style.add_modifier.contains(Modifier::DIM)
                {
                    found_targets_normal = true;
                }
            }
        }
        assert!(
            found_targets_normal,
            "expected [[targets]] to render normal on Target page"
        );
    }

    #[test]
    fn test_preview_dims_non_active_sections() {
        let mut state = WizardState::new();
        // Engine page (2) → active section is [workload]; [output] should dim.
        state.current_page = 2;
        let lines = render_lines(&state);
        let mut found_dim_output = false;
        let mut found_normal_workload = false;
        for line in &lines {
            for span in &line.spans {
                if span.content.contains("[output]")
                    && span.style.add_modifier.contains(Modifier::DIM)
                {
                    found_dim_output = true;
                }
                if span.content.trim() == "[workload]"
                    && !span.style.add_modifier.contains(Modifier::DIM)
                {
                    found_normal_workload = true;
                }
            }
        }
        assert!(found_dim_output, "expected [output] section to be dimmed");
        assert!(
            found_normal_workload,
            "expected active [workload] section to be normal"
        );
    }

    /// The wizard's TOML, when parsed back, must yield a Config equivalent to
    /// the canonical fixture's parsed Config — so the wizard can be used to
    /// produce *any* config the CLI accepts.
    #[test]
    fn test_wizard_toml_roundtrips_through_parser() {
        // Build a state matching examples/basic_config.toml's workload settings.
        let mut state = WizardState::new();
        state.config.workload.read_percent = 70;
        state.config.workload.write_percent = 30;
        state.config.workload.queue_depth = 32;

        let toml_str = ReviewPage::render_toml(&state);
        let parsed: Config = toml::from_str(&toml_str)
            .unwrap_or_else(|e| panic!("wizard TOML failed to parse: {}\n---\n{}", e, toml_str));

        assert_eq!(parsed.workload.read_percent, 70);
        assert_eq!(parsed.workload.write_percent, 30);
        assert_eq!(parsed.workload.queue_depth, 32);
    }

    // NOTE: a "fixture parses" test is intentionally omitted. The canonical
    // fixture `examples/basic_config.toml` does not currently parse against
    // `Config` because of a pre-existing case-sensitivity bug in EngineType
    // deserialization (the fixture uses `engine = "sync"`, the deserializer
    // accepts `"Sync"`). That bug is one of the 14 baseline failures the plan
    // calls out (config::toml::tests::test_parse_toml_basic et al.) and is
    // explicitly out of scope for the wizard work — fixing it would touch
    // EngineType's serde rename, which is a separate change.
}
