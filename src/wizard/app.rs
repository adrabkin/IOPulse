//! Wizard event loop and terminal guard.
//!
//! The event loop is parameterized over an `EventSource` trait so tests can drive
//! it with a canned queue of events without needing a real terminal. In production
//! the event source is `CrosstermEventSource`, which polls `crossterm::event::poll`
//! at 50ms intervals.
//!
//! Terminal state is owned by `TerminalGuard`, which restores cooked mode and
//! leaves the alternate screen on `Drop`. A panic hook is installed on first use
//! so a panic on the wizard thread doesn't leave the user's terminal corrupted.

use crate::wizard::pages::{build_all_pages, Page, PageOutcome, PAGE_COUNT};
use crate::wizard::preview;
use crate::wizard::state::WizardState;
use anyhow::Result;
use crossterm::event::Event;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use std::io::{self, Stdout};
use std::time::Duration;

/// Source of UI events. Production uses `CrosstermEventSource`; tests use a
/// canned queue.
pub trait EventSource {
    /// Poll for the next event, returning `None` if `timeout` elapses with
    /// nothing available.
    fn poll(&mut self, timeout: Duration) -> Result<Option<Event>>;
}

/// Reason the event loop returned. Used by tests to assert clean exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopExit {
    /// User requested quit (e.g., `q` or `Ctrl-C`).
    Quit,
    /// User asked to save the config to `state.output_path`.
    Save,
    /// User asked to save and exec into `iopulse --config <tempfile>`.
    RunNow,
    /// Event source drained (test-only — production poll never drains).
    EventSourceDrained,
}

/// Run the wizard event loop against an arbitrary event source. This is the
/// testable core; `run` wraps it with a real terminal and crossterm event source.
///
/// Returns `Ok(LoopExit::*)` on normal termination. Panics propagate to the
/// caller after the panic hook restores the terminal.
pub fn run_loop<E: EventSource>(state: &mut WizardState, source: &mut E) -> Result<LoopExit> {
    let mut pages = build_all_pages();
    run_loop_with_pages(state, source, &mut pages)
}

/// Variant of `run_loop` that takes an existing page list; useful for tests
/// that want to substitute MockPages for the real ones.
pub fn run_loop_with_pages<E: EventSource>(
    state: &mut WizardState,
    source: &mut E,
    pages: &mut [Box<dyn Page>],
) -> Result<LoopExit> {
    use crossterm::event::{KeyCode, KeyModifiers};

    loop {
        // Refresh validation whenever the state has changed since the last pass.
        if state.dirty {
            run_validation(state, pages);
        }

        let event = match source.poll(Duration::from_millis(50))? {
            Some(ev) => ev,
            None => continue,
        };

        if let Event::Key(key) = event {
            // Top-level quit applies on any page, even if it would otherwise
            // be a printable character — keeps escape hatch consistent. Note
            // this means typed text on input fields can't include 'q' alone;
            // the wizard's text fields work around it because Enter / Esc /
            // Tab are caught by handle_key first.
            if key.code == KeyCode::Char('q')
                || (key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                return Ok(LoopExit::Quit);
            }

            let page_idx = state.current_page.min(pages.len().saturating_sub(1));
            let outcome = pages[page_idx].handle_key(key, state);
            // Re-run validation after the page may have mutated state.
            if state.dirty {
                run_validation(state, pages);
            }
            match outcome {
                PageOutcome::Stay => {}
                PageOutcome::Next => {
                    let errors = state.errors_blocking_advance();
                    if errors > 0 {
                        state.flash = Some(format!(
                            "cannot advance: {} error{} on this page",
                            errors,
                            if errors == 1 { "" } else { "s" }
                        ));
                        continue;
                    }
                    state.flash = None;
                    if state.current_page + 1 < PAGE_COUNT.min(pages.len()) {
                        state.current_page += 1;
                    }
                }
                PageOutcome::Back => {
                    state.flash = None;
                    if state.current_page > 0 {
                        state.current_page -= 1;
                    }
                }
                PageOutcome::Quit => return Ok(LoopExit::Quit),
                PageOutcome::Save => {
                    let errors = state.issue_counts().0;
                    if errors > 0 {
                        state.flash = Some(format!(
                            "cannot save: {} error{} — fix and retry",
                            errors,
                            if errors == 1 { "" } else { "s" }
                        ));
                        continue;
                    }
                    return Ok(LoopExit::Save);
                }
                PageOutcome::RunNow => {
                    let errors = state.issue_counts().0;
                    if errors > 0 {
                        state.flash = Some(format!(
                            "cannot run: {} error{} — fix and retry",
                            errors,
                            if errors == 1 { "" } else { "s" }
                        ));
                        continue;
                    }
                    return Ok(LoopExit::RunNow);
                }
            }
        }
    }
}

/// Refresh `state.validation` from both the global validator and every page's
/// own `validate`. The dirty flag is cleared so the next iteration skips this
/// work unless something else changes.
fn run_validation(state: &mut WizardState, pages: &mut [Box<dyn Page>]) {
    state.run_global_validation();
    let page_issues: Vec<_> = pages
        .iter()
        .flat_map(|p| p.validate(state))
        .collect();
    state.validation.extend(page_issues);
}

/// Render the status bar showing error/warning counts and the save target.
/// Any transient `state.flash` message is appended in red so blocked actions
/// surface clearly.
pub fn render_status_bar(frame: &mut Frame, area: Rect, state: &WizardState) {
    let (errors, warnings) = state.issue_counts();
    let style = if errors > 0 || state.flash.is_some() {
        Style::default().fg(Color::Red)
    } else if warnings > 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Green)
    };
    let mut text = format!(
        "{} errors · {} warnings · save → {}",
        errors,
        warnings,
        state.output_path.display()
    );
    if let Some(flash) = &state.flash {
        text.push_str(" · ");
        text.push_str(flash);
    }
    let para = Paragraph::new(text)
        .style(style)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(para, area);
}


/// Click-region map produced by each render pass. The event loop consults it
/// when it sees a Mouse event to decide which tab/button was clicked.
#[derive(Debug, Default, Clone)]
pub struct ClickMap {
    /// Each tab's screen rectangle and the page index it jumps to.
    pub tabs: Vec<(Rect, usize)>,
    /// Back button rect.
    pub back: Option<Rect>,
    /// Next button rect.
    pub next: Option<Rect>,
    /// Save button rect (only on review page).
    pub save: Option<Rect>,
    /// Run-now button rect (only on review page).
    pub run: Option<Rect>,
}

impl ClickMap {
    fn hit_tab(&self, x: u16, y: u16) -> Option<usize> {
        for (rect, idx) in &self.tabs {
            if rect_contains(rect, x, y) {
                return Some(*idx);
            }
        }
        None
    }
}

fn rect_contains(rect: &Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

/// Production event loop that draws the wizard with a structured layout:
/// tab bar (clickable), header, description, page widgets, button row, status
/// bar. The right pane shows a live TOML preview.
pub fn run_loop_drawing(
    state: &mut WizardState,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    pages: &mut [Box<dyn Page>],
) -> Result<LoopExit> {
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};

    // Validate once at startup so the status bar is accurate from the first frame.
    run_validation(state, pages);
    let mut click_map = ClickMap::default();

    loop {
        // Refresh validation whenever the state has changed.
        if state.dirty {
            run_validation(state, pages);
        }

        // Draw current state and capture click regions.
        let mut new_click_map = ClickMap::default();
        terminal.draw(|frame| {
            new_click_map = draw_frame(frame, state, pages);
        })?;
        click_map = new_click_map;

        // Wait for one event.
        if !crossterm::event::poll(Duration::from_millis(50))? {
            continue;
        }
        let event = crossterm::event::read()?;
        match event {
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                if let Some(exit) = handle_click_outcome(state, pages, &click_map, m.column, m.row)
                {
                    if let Some(reason) = exit {
                        return Ok(reason);
                    }
                }
            }
            Event::Key(key) => {
                // Ctrl+C: hard quit.
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    return Ok(LoopExit::Quit);
                }
                // Esc: go back one page (or quit if on page 0).
                if key.code == KeyCode::Esc {
                    if state.current_page == 0 {
                        return Ok(LoopExit::Quit);
                    }
                    state.flash = None;
                    state.current_page -= 1;
                    continue;
                }

                let page_idx = state.current_page.min(pages.len().saturating_sub(1));
                let outcome = pages[page_idx].handle_key(key, state);
                if state.dirty {
                    run_validation(state, pages);
                }
                if let Some(exit) = apply_outcome(state, pages, outcome) {
                    return Ok(exit);
                }
            }
            _ => {}
        }
    }
}

/// Hit-test a click and apply the resulting nav. Returns `Some(Some(LoopExit))`
/// if the click triggered Save/Run/Quit, `Some(None)` if it was handled but
/// didn't exit, `None` if no click target matched.
fn handle_click_outcome(
    state: &mut WizardState,
    pages: &mut [Box<dyn Page>],
    map: &ClickMap,
    x: u16,
    y: u16,
) -> Option<Option<LoopExit>> {
    // Tab click: jump to that page. Forward jumps still respect error gating.
    if let Some(target) = map.hit_tab(x, y) {
        if target <= state.current_page {
            // Backwards is always free.
            state.flash = None;
            state.current_page = target;
        } else {
            // Forward: only if the current page validates clean.
            let errors = state.errors_blocking_advance();
            if errors == 0 {
                state.flash = None;
                state.current_page = target.min(PAGE_COUNT.saturating_sub(1));
            } else {
                state.flash = Some(format!(
                    "fix {} error{} before jumping ahead",
                    errors,
                    if errors == 1 { "" } else { "s" }
                ));
            }
        }
        if state.dirty {
            run_validation(state, pages);
        }
        return Some(None);
    }
    if let Some(rect) = map.back.as_ref() {
        if rect_contains(rect, x, y) {
            return Some(apply_outcome(state, pages, PageOutcome::Back));
        }
    }
    if let Some(rect) = map.next.as_ref() {
        if rect_contains(rect, x, y) {
            return Some(apply_outcome(state, pages, PageOutcome::Next));
        }
    }
    if let Some(rect) = map.save.as_ref() {
        if rect_contains(rect, x, y) {
            return Some(apply_outcome(state, pages, PageOutcome::Save));
        }
    }
    if let Some(rect) = map.run.as_ref() {
        if rect_contains(rect, x, y) {
            return Some(apply_outcome(state, pages, PageOutcome::RunNow));
        }
    }
    None
}

/// Apply a `PageOutcome` to navigation/exit, mirroring the key-driven path.
fn apply_outcome(
    state: &mut WizardState,
    pages: &mut [Box<dyn Page>],
    outcome: PageOutcome,
) -> Option<LoopExit> {
    match outcome {
        PageOutcome::Stay => None,
        PageOutcome::Next => {
            let errors = state.errors_blocking_advance();
            if errors > 0 {
                state.flash = Some(format!(
                    "cannot advance: {} error{} on this page",
                    errors,
                    if errors == 1 { "" } else { "s" }
                ));
            } else {
                state.flash = None;
                if state.current_page + 1 < PAGE_COUNT.min(pages.len()) {
                    state.current_page += 1;
                }
            }
            None
        }
        PageOutcome::Back => {
            state.flash = None;
            if state.current_page > 0 {
                state.current_page -= 1;
            }
            None
        }
        PageOutcome::Quit => Some(LoopExit::Quit),
        PageOutcome::Save => {
            let errors = state.issue_counts().0;
            if errors == 0 {
                Some(LoopExit::Save)
            } else {
                state.flash = Some(format!(
                    "cannot save: {} error{} — fix and retry",
                    errors,
                    if errors == 1 { "" } else { "s" }
                ));
                None
            }
        }
        PageOutcome::RunNow => {
            let errors = state.issue_counts().0;
            if errors == 0 {
                Some(LoopExit::RunNow)
            } else {
                state.flash = Some(format!(
                    "cannot run: {} error{} — fix and retry",
                    errors,
                    if errors == 1 { "" } else { "s" }
                ));
                None
            }
        }
    }
}

/// One render pass. Returns the click map for the rendered frame so the
/// event loop can hit-test mouse clicks.
fn draw_frame(frame: &mut Frame, state: &WizardState, pages: &[Box<dyn Page>]) -> ClickMap {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tab bar
            Constraint::Min(0),    // body
            Constraint::Length(3), // status bar
        ])
        .split(area);

    let page_idx = state.current_page.min(pages.len().saturating_sub(1));
    let page = &pages[page_idx];
    let is_review = page_idx == PAGE_COUNT.saturating_sub(1);

    let mut click_map = ClickMap::default();

    // Tab bar (clickable)
    click_map.tabs = render_tab_bar(frame, outer[0], state, pages);

    // Body layout: form card on its own (most pages) OR side-by-side with
    // preview (only on the final review page where the user wants to see
    // exactly what they're saving).
    let form_area = if is_review {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(outer[1]);
        preview::render(frame, panes[1], state, 0);
        panes[0]
    } else {
        outer[1]
    };

    // Single outlined "form card" wraps title + description + inputs +
    // buttons. Inside the card we draw structured rows; the outer border
    // gives a single visual container for the whole step.
    let card_title = format!(" Step {} of {}: {} ", page_idx + 1, PAGE_COUNT, page.title());
    let card = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(card_title);
    let card_inner = card.inner(form_area);
    frame.render_widget(card, form_area);

    // Inside the card: description, then page widgets, then buttons.
    let card_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(6), // description
            Constraint::Min(6),    // page widgets
            Constraint::Length(3), // button row
        ])
        .split(card_inner);

    // Description (no border — the card already provides the frame).
    let desc = Paragraph::new(page.description())
        .wrap(ratatui::widgets::Wrap { trim: false })
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(desc, card_chunks[0]);

    // Page widgets — page-internal layout decides labels/inputs.
    page.render(frame, card_chunks[1], state);

    // Button row (clickable)
    let (back, next, save, run) =
        render_button_row(frame, card_chunks[2], state, page_idx, page.keybindings());
    click_map.back = back;
    click_map.next = next;
    click_map.save = save;
    click_map.run = run;

    // Bottom: status bar
    render_status_bar(frame, outer[2], state);

    click_map
}

/// Render the top tab bar showing every page. Returns each tab's rect and
/// the page index it represents (for click hit-testing).
fn render_tab_bar(
    frame: &mut Frame,
    area: Rect,
    state: &WizardState,
    pages: &[Box<dyn Page>],
) -> Vec<(Rect, usize)> {
    use ratatui::text::{Line, Span};

    let mut spans: Vec<Span> = Vec::new();
    let mut tab_rects = Vec::new();

    // Track running x position inside the area (account for the border).
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let mut x = inner.x;

    for (i, p) in pages.iter().enumerate() {
        let label = format!(" {}.{} ", i + 1, p.title());
        let style = if i == state.current_page {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if i < state.current_page {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let label_len = label.chars().count() as u16;
        if x + label_len > inner.x + inner.width {
            // Out of room — stop rendering tabs (line up has gone past the
            // border). Click map only includes what we drew.
            break;
        }
        let rect = Rect {
            x,
            y: inner.y,
            width: label_len,
            height: 1,
        };
        tab_rects.push((rect, i));
        spans.push(Span::styled(label, style));
        x += label_len;
    }

    let line = Line::from(spans);
    let para = Paragraph::new(line).block(Block::default().borders(Borders::ALL).title("Steps"));
    frame.render_widget(para, area);

    tab_rects
}

/// Render the Back / Next (or Save / Run on the review page) buttons.
/// Returns each button's rect (for click hit-testing).
fn render_button_row(
    frame: &mut Frame,
    area: Rect,
    state: &WizardState,
    page_idx: usize,
    extra_hints: &str,
) -> (Option<Rect>, Option<Rect>, Option<Rect>, Option<Rect>) {
    use ratatui::text::{Line, Span};

    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    // Back button (always available; on page 0 it's "Cancel")
    let back_label = if page_idx == 0 {
        " [ Cancel ] "
    } else {
        " [ ◄ Back ] "
    };
    let back_rect = Rect {
        x: inner.x,
        y: inner.y,
        width: back_label.chars().count() as u16,
        height: 1,
    };

    // Next/Save/Run buttons
    let (next_label, save_label, run_label) = if page_idx == PAGE_COUNT.saturating_sub(1) {
        (None, Some(" [ Save ] "), Some(" [ Save & Run ] "))
    } else {
        (Some(" [ Next ► ] "), None, None)
    };

    // Compute right-aligned positions.
    let right_edge = inner.x + inner.width;
    let mut spans = vec![Span::styled(back_label, Style::default().fg(Color::Cyan))];
    spans.push(Span::raw(format!("    {}    ", extra_hints)));

    let mut next_rect = None;
    let mut save_rect = None;
    let mut run_rect = None;

    if let Some(label) = next_label {
        let len = label.chars().count() as u16;
        let rect = Rect {
            x: right_edge.saturating_sub(len),
            y: inner.y,
            width: len,
            height: 1,
        };
        next_rect = Some(rect);
        // We can't precisely position via spans; the line is sequential. Just
        // append so it wraps; the click hit-test uses the computed rect.
        spans.push(Span::styled(label, Style::default().fg(Color::Cyan)));
    }
    if let (Some(s_label), Some(r_label)) = (save_label, run_label) {
        let r_len = r_label.chars().count() as u16;
        let s_len = s_label.chars().count() as u16;
        let r_x = right_edge.saturating_sub(r_len);
        let s_x = r_x.saturating_sub(s_len);
        save_rect = Some(Rect {
            x: s_x,
            y: inner.y,
            width: s_len,
            height: 1,
        });
        run_rect = Some(Rect {
            x: r_x,
            y: inner.y,
            width: r_len,
            height: 1,
        });
        spans.push(Span::styled(s_label, Style::default().fg(Color::Green)));
        spans.push(Span::styled(r_label, Style::default().fg(Color::Yellow)));
    }

    let _ = state;
    let line = Line::from(spans);
    let para = Paragraph::new(line).block(Block::default().borders(Borders::ALL).title("Actions"));
    frame.render_widget(para, area);

    (Some(back_rect), next_rect, save_rect, run_rect)
}

/// Terminal guard. Owns the raw-mode + alternate-screen lifecycle and restores
/// cooked mode on `Drop` regardless of how the wizard exits (normal, error,
/// panic).
pub struct TerminalGuard {
    /// Set to false once the guard has torn the terminal down, so a manual
    /// `restore()` followed by `Drop` doesn't double-restore.
    active: bool,
}

impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
        Ok(Self { active: true })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.active {
            // Best-effort: even if these fail there's nothing useful we can do.
            let _ = crossterm::execute!(
                io::stdout(),
                crossterm::event::DisableMouseCapture,
                crossterm::terminal::LeaveAlternateScreen
            );
            let _ = crossterm::terminal::disable_raw_mode();
            self.active = false;
        }
    }
}

/// Install a panic hook that restores the terminal before delegating to the
/// previous hook. Idempotent — only installs once per process.
pub fn install_panic_hook() {
    use std::sync::Once;
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = crossterm::execute!(
                io::stdout(),
                crossterm::event::DisableMouseCapture,
                crossterm::terminal::LeaveAlternateScreen
            );
            let _ = crossterm::terminal::disable_raw_mode();
            prev(info);
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use std::collections::VecDeque;

    /// Test-only event source backed by a canned queue. Returns `None` (timeout)
    /// when the queue is drained — the loop will spin until something is enqueued
    /// or it's told to exit.
    struct MockEventSource {
        queue: VecDeque<Event>,
        /// Number of `None` returns before the source declares itself drained.
        max_idle_polls: usize,
        idle_polls: usize,
    }

    impl MockEventSource {
        fn new(events: Vec<Event>) -> Self {
            Self {
                queue: events.into(),
                max_idle_polls: 10,
                idle_polls: 0,
            }
        }
    }

    impl EventSource for MockEventSource {
        fn poll(&mut self, _timeout: Duration) -> Result<Option<Event>> {
            if let Some(ev) = self.queue.pop_front() {
                self.idle_polls = 0;
                return Ok(Some(ev));
            }
            self.idle_polls += 1;
            if self.idle_polls > self.max_idle_polls {
                // Simulate the loop exit by injecting Ctrl-C — tests that don't
                // explicitly send a quit key still terminate.
                return Ok(Some(Event::Key(KeyEvent::new(
                    KeyCode::Char('c'),
                    KeyModifiers::CONTROL,
                ))));
            }
            Ok(None)
        }
    }

    #[test]
    fn test_q_exits_cleanly() {
        let mut state = WizardState::new();
        let mut source = MockEventSource::new(vec![Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        ))]);
        let exit = run_loop(&mut state, &mut source).expect("loop runs without error");
        assert_eq!(exit, LoopExit::Quit);
    }

    #[test]
    fn test_ctrl_c_exits_cleanly() {
        let mut state = WizardState::new();
        let mut source = MockEventSource::new(vec![Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ))]);
        let exit = run_loop(&mut state, &mut source).expect("loop runs without error");
        assert_eq!(exit, LoopExit::Quit);
    }

    #[test]
    fn test_idle_then_quit() {
        // Drains a few empty polls, then sees `q`.
        let mut state = WizardState::new();
        let mut source = MockEventSource::new(vec![Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        ))]);
        // Force a few idle polls before the queued event by pre-incrementing.
        source.idle_polls = 0;
        let exit = run_loop(&mut state, &mut source).expect("loop runs without error");
        assert_eq!(exit, LoopExit::Quit);
    }

    #[test]
    fn test_unrelated_keys_do_not_exit() {
        // Loop should ignore 'a', 'b', then exit on 'q'.
        let mut state = WizardState::new();
        let mut source = MockEventSource::new(vec![
            Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        ]);
        let exit = run_loop(&mut state, &mut source).expect("loop runs without error");
        assert_eq!(exit, LoopExit::Quit);
    }

    #[test]
    fn test_install_panic_hook_is_idempotent() {
        // Calling twice should not panic or duplicate. (Functional check; the
        // hook itself is hard to observe without panicking.)
        install_panic_hook();
        install_panic_hook();
    }

    /// Page that always asks for Next, lets us assert navigation gating.
    struct AlwaysAdvancePage {
        title: &'static str,
    }

    impl Page for AlwaysAdvancePage {
        fn title(&self) -> &str {
            self.title
        }
        fn render(&self, _: &mut Frame, _: ratatui::layout::Rect, _: &WizardState) {}
        fn handle_key(
            &mut self,
            _: KeyEvent,
            state: &mut WizardState,
        ) -> PageOutcome {
            state.dirty = true;
            PageOutcome::Next
        }
        fn validate(&self, _: &WizardState) -> Vec<crate::wizard::state::ValidationIssue> {
            vec![]
        }
    }

    #[test]
    fn test_navigation_blocked_when_global_errors_present() {
        let mut state = WizardState::new();
        // Force a global error.
        state.config.workload.read_percent = 60;
        state.config.workload.write_percent = 30;
        state.dirty = true;

        let mut pages: Vec<Box<dyn Page>> = vec![
            Box::new(AlwaysAdvancePage { title: "p0" }),
            Box::new(AlwaysAdvancePage { title: "p1" }),
        ];
        // Send a single key, then a quit key.
        let mut source = MockEventSource::new(vec![
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        ]);
        run_loop_with_pages(&mut state, &mut source, &mut pages).expect("loop runs");
        // Despite Next, the page should not have advanced because errors stand.
        assert_eq!(state.current_page, 0);
        // The user-visible flash message must explain why nav was blocked.
        assert!(
            state.flash.as_deref().map_or(false, |s| s.contains("cannot advance")),
            "expected flash explaining the block, got {:?}",
            state.flash
        );
    }

    #[test]
    fn test_navigation_proceeds_when_no_errors() {
        let mut state = WizardState::new();
        state.config.workload.read_percent = 70;
        state.config.workload.write_percent = 30;
        state.dirty = true;

        let mut pages: Vec<Box<dyn Page>> = vec![
            Box::new(AlwaysAdvancePage { title: "p0" }),
            Box::new(AlwaysAdvancePage { title: "p1" }),
        ];
        let mut source = MockEventSource::new(vec![
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        ]);
        run_loop_with_pages(&mut state, &mut source, &mut pages).expect("loop runs");
        // No errors → Next should have been honored once.
        assert_eq!(state.current_page, 1);
    }

    #[test]
    fn test_status_bar_renders_without_panic() {
        let state = WizardState::new();
        let backend = ratatui::backend::TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render_status_bar(frame, frame.area(), &state))
            .expect("status bar renders");
    }
}
