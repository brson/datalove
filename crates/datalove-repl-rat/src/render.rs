//! Ratatui rendering functions for the REPL UI.

use rmx::prelude::*;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table},
    Frame,
};

use crate::RatatuiApp;
use crate::repl::{self, script};
use crate::repl::app::{Editing, EntryStatus, Panel, ReplApp, ReplExecutor, Tab, UnitStatus};

/// The color of what is selected: the tab shown, and the code unit.
const HIGHLIGHT: Color = Color::Yellow;

/// The rows a panel's borders take, top and bottom.
const BORDERS: u16 = 2;

/// The height of the single-line input: its line and its borders.
const INPUT_HEIGHT: u16 = 1 + BORDERS;

/// Render the UI.
pub fn ui<E: ReplExecutor>(f: &mut Frame, app: &mut RatatuiApp<E>) {
    // The code on top, the input, and the tabs below. A single line of input
    // sits in the middle of the screen; multiline input grows down from
    // there to a third of it, and the tabs give way.
    let constraints = if app.repl.multiline_mode() {
        let top_height = f.area().height.saturating_sub(INPUT_HEIGHT) / 2;
        vec![Constraint::Length(top_height), Constraint::Percentage(33), Constraint::Min(0)]
    } else {
        vec![Constraint::Fill(1), Constraint::Length(INPUT_HEIGHT), Constraint::Fill(1)]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    let (code_view, unit_lines) = render_code(f, &app.repl, chunks[0]);
    app.repl.record_code_view(code_view.pane_lines, code_view.scroll_back_limit, unit_lines);

    render_input(f, app, chunks[1]);

    // Only the tab shown is laid out, so only its bounds are recorded; the
    // other is recorded when it is shown, before a key can scroll it.
    let bottom_block = bottom_block(&app.repl);
    match app.repl.tab() {
        Tab::Environment => {
            let view = render_environment(f, &app.repl, chunks[2], bottom_block);
            app.repl.record_environment_view(view.pane_rows, view.scroll_limit);
        }
        Tab::History => {
            let view = render_history(f, &app.repl, chunks[2], bottom_block);
            app.repl.record_history_view(view.pane_lines, view.scroll_back_limit);
        }
        Tab::Help => {
            let view = render_help(f, &app.repl, chunks[2], bottom_block);
            app.repl.record_help_view(view.pane_rows, view.scroll_limit);
        }
        Tab::Detail => {
            let view = render_detail(f, &app.repl, chunks[2], bottom_block);
            app.repl.record_detail_view(view.pane_rows, view.scroll_limit);
        }
    }

    // Each over the one before: a crash over the exit dialog, a bug in the
    // app's bookkeeping over the reset that may have caused it, and a dead
    // engine over everything, since nothing else can make progress.
    if app.repl.exit_asked() {
        render_exit_dialog(f);
    }
    if let Some(msg) = app.repl.crash_modal_message() {
        render_crash_modal(f, &app.repl, msg);
    }
    if let Some(msg) = app.repl.session_mismatch_message() {
        render_session_mismatch_modal(f, msg);
    }
    if let Some(msg) = app.repl.engine_dead_message() {
        render_engine_dead_modal(f, &app.repl, msg);
    }
}

/// The border style of a panel: white when it has focus, gray when not.
fn panel_border<E: ReplExecutor>(repl: &ReplApp<E>, panel: Panel) -> Style {
    if repl.focus() == panel {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// A line of the history after its input: a mark, and what it says, both in
/// one color.
pub(crate) fn marked<'a>(mark: &str, color: Color, text: impl Into<std::borrow::Cow<'a, str>>) -> Line<'a> {
    let style = Style::default().fg(color);
    Line::from(vec![Span::styled(fmt!("  {mark} "), style), Span::styled(text.into(), style)])
}

/// One binding a fragment defined, with its type.
fn binding_line(binding: &script::EvalBinding) -> Line<'_> {
    let check = Span::styled("  ✓ ", Style::default().fg(Color::Green));
    match binding {
        script::EvalBinding::Function { name } => Line::from(vec![
            check,
            Span::raw("fun "),
            Span::styled(name, Style::default().fg(Color::Cyan)),
        ]),
        script::EvalBinding::Value { name, ty, value }
        | script::EvalBinding::Slot { name, ty, value } => Line::from(vec![
            check,
            Span::styled(name, Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(ty, Style::default().fg(Color::Yellow)),
            match value {
                Some(value) => Span::raw(fmt!(" = {value}")),
                None => Span::styled(" -- given away", given_away()),
            },
        ]),
    }
}

/// How a binding that was given away is shown in place of its value.
pub(crate) fn given_away() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// What a unit came to, in a line's worth: its mark, its color, and what it
/// says.
///
/// The colors are the bars': green for a unit that ran, red for one that did
/// not compile, yellow for one that returned early, and magenta for one that
/// was blocked.
pub(crate) fn outcome(eval: &script::Eval) -> (&'static str, Color, String) {
    match eval {
        script::Eval::Ran(bindings) if bindings.is_empty() => ("✓", Color::Green, "ran".S()),
        script::Eval::Ran(bindings) => {
            let bound: Vec<String> = bindings.iter().map(|binding| match binding {
                script::EvalBinding::Function { name } => fmt!("fun {name}"),
                script::EvalBinding::Value { name, value: Some(value), .. }
                | script::EvalBinding::Slot { name, value: Some(value), .. } => fmt!("{name} = {value}"),
                script::EvalBinding::Value { name, value: None, .. }
                | script::EvalBinding::Slot { name, value: None, .. } => fmt!("{name}, given away"),
            }).collect();
            ("✓", Color::Green, bound.join(", "))
        }
        script::Eval::Value(expr) => ("⇒", Color::Green, fmt!("{} = {}", expr.ty, expr.value)),
        script::Eval::NotCompiled(e) => ("✗", Color::Red, fmt!("error: {e}")),
        script::Eval::ReturnedEarly(returned) => ("✗", Color::Yellow, fmt!("returned early: {returned}")),
        script::Eval::Blocked { on } => ("✗", Color::Magenta, script::blocked_reason(on)),
        script::Eval::RuntimeError(_) | script::Eval::Crashed(_) => {
            unreachable!("a reset takes the entry it would be shown in")
        }
    }
}

/// The lines saying what an answered input came to.
fn eval_lines(eval: &repl::Outcome) -> Vec<Line<'_>> {
    match eval {
        // A fragment's bindings, each with its type.
        repl::Outcome::Unit(script::Eval::Ran(bindings)) => bindings.iter().map(binding_line).collect(),
        repl::Outcome::Unit(eval) => {
            let (mark, color, said) = outcome(eval);
            vec![marked(mark, color, said)]
        }
        repl::Outcome::Edited(step) => {
            let mut lines: Vec<Line> = step.iter().map(|unit| match &unit.eval {
                Some(eval) => {
                    let (mark, color, said) = outcome(eval);
                    Line::from(vec![
                        Span::styled(fmt!("  {mark} "), Style::default().fg(color)),
                        Span::styled(fmt!("unit {}: ", unit.unit), Style::default().fg(Color::DarkGray)),
                        Span::styled(said, Style::default().fg(color)),
                    ])
                }
                None => marked("·", Color::DarkGray, fmt!("unit {}: compiled again", unit.unit)),
            }).collect();
            if step.iter().all(|unit| unit.eval.is_none()) {
                lines.insert(0, marked("✓", Color::Green, "no other unit reran"));
            }
            lines
        }
        repl::Outcome::CommandError(e) => vec![marked("✗", Color::Red, fmt!("error: {e}"))],
        repl::Outcome::Saved { path, inputs } => vec![marked("✓", Color::Green, fmt!("saved {inputs} inputs to {path}"))],
        repl::Outcome::Help => vec![marked("ℹ", Color::Blue, "shown in the Help tab")],
        repl::Outcome::Loaded { inputs } => vec![marked("✓", Color::Green, fmt!("loaded {inputs} inputs"))],
        repl::Outcome::Reset => unreachable!("a reset takes its own entry"),
        repl::Outcome::Detail(_) => unreachable!("a detail answers a question, not an entry"),
    }
}

/// The shape of a pane scrolled up from its bottom, the code or the history,
/// as a frame laid it out.
///
/// The renderer is the only thing that knows either number, and the app needs
/// both to bound scrolling.
struct ScrollBackView {
    /// Lines the pane showed.
    pane_lines: usize,
    /// The furthest up from the bottom the view can scroll.
    scroll_back_limit: usize,
}

/// The shape of a pane scrolled down from its top, the environment or the
/// help, as a frame laid it out.
struct ScrollDownView {
    /// Rows the pane showed: of bindings, not counting the header, or of help.
    pane_rows: usize,
    /// The furthest down from the top the view can scroll.
    scroll_limit: usize,
}

/// One line of a unit's bar.
///
/// The left half, in the status color, is every line. The right half says
/// what the last step did: its top on the unit's first line if the step
/// compiled the unit, and its bottom on the unit's last line if it ran it. A
/// one-line unit has both corners in its one cell.
pub(crate) fn bar_glyph(compiled: bool, ran: bool, line: usize, last_line: usize) -> &'static str {
    let compiled = compiled && line == 0;
    let ran = ran && line == last_line;
    match (compiled, ran) {
        (false, false) => "▌ ",
        (true, false) => "▛ ",
        (false, true) => "▙ ",
        (true, true) => "█ ",
    }
}

/// The color of a unit's bar, for its status.
pub(crate) fn status_color(status: UnitStatus) -> Color {
    match status {
        UnitStatus::Waiting => Color::Cyan,
        UnitStatus::Ran => Color::Green,
        UnitStatus::NotCompiled => Color::Red,
        UnitStatus::ReturnedEarly => Color::Yellow,
        UnitStatus::Blocked => Color::Magenta,
    }
}

/// Render the code panel: every unit's source, in order, each beside a bar
/// colored for its status, and the selected unit highlighted while the panel
/// has focus.
///
/// Returns the lines each unit took, for scrolling the selection into view.
fn render_code<E: ReplExecutor>(
    f: &mut Frame,
    repl: &ReplApp<E>,
    area: Rect,
) -> (ScrollBackView, Vec<(u32, std::ops::Range<usize>)>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(panel_border(repl, Panel::Code))
        .title("Code");

    let mut lines: Vec<Line> = Vec::new();

    // Startup compiles the system library and builds the native riders, which
    // takes seconds at best; say so, or nothing seems to happen.
    if !repl.engine_is_ready() {
        lines.push(marked("…", Color::Yellow, "starting engine"));
        lines.push(Line::from(""));
    }

    let units = repl.code_units();
    if units.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no code yet)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // A unit is always selected, its number white; its text is yellow while
    // the panel has focus, and the keys go to it.
    let selected = repl.selected_unit();
    let focused = repl.focus() == Panel::Code;
    let separator = "─".repeat(area.width.saturating_sub(BORDERS) as usize);
    // Every unit's number in a column as wide as the widest.
    let number_width = units.iter()
        .filter_map(|unit| unit.number)
        .max()
        .map_or(1, |number| number.to_string().len());
    let mut unit_lines = Vec::new();
    for (index, unit) in units.iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(Span::styled(
                separator.C(),
                Style::default().fg(Color::DarkGray),
            )));
        }
        let bar = Style::default().fg(status_color(unit.status));
        let is_selected = selected.is_some() && selected == unit.number;
        let text = match is_selected && focused {
            true => Style::default().fg(HIGHLIGHT),
            false => Style::default(),
        };
        let number_style = match is_selected {
            true => Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            false => Style::default().fg(Color::DarkGray),
        };
        let start = lines.len();
        let last_line = unit.source.lines().count().saturating_sub(1);
        for (line_index, line) in unit.source.lines().enumerate() {
            // The number on the unit's first line; a unit on its way to the
            // engine has none yet.
            let number = match (line_index, unit.number) {
                (0, Some(number)) => fmt!("{number:>number_width$} "),
                _ => " ".repeat(number_width + 1),
            };
            lines.push(Line::from(vec![
                Span::styled(number, number_style),
                Span::styled(bar_glyph(unit.compiled, unit.ran, line_index, last_line), bar),
                Span::styled(line, text),
            ]));
        }
        if let Some(number) = unit.number {
            unit_lines.push((number, start..lines.len()));
        }
    }

    let view = render_scrolled_back(f, lines, block, area, repl.code_scroll_back());
    (view, unit_lines)
}

/// The bottom panel's frame, whose title is its tabs.
fn bottom_block<E: ReplExecutor>(repl: &ReplApp<E>) -> Block<'static> {
    let mut tabs = vec![Span::raw(" ")];
    for (index, tab) in repl.tabs().into_iter().enumerate() {
        if index > 0 {
            tabs.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }
        let style = if tab == repl.tab() {
            Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        tabs.push(Span::styled(tab.title(), style));
    }
    tabs.push(Span::raw(" "));

    Block::default()
        .borders(Borders::ALL)
        .border_style(panel_border(repl, Panel::Bottom))
        .title(Line::from(tabs))
        .title(Line::from("[F2]").right_aligned())
}

/// Render the history tab: every input and what it came to.
fn render_history<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>, area: Rect, block: Block) -> ScrollBackView {
    let mut lines: Vec<Line> = Vec::new();

    for entry in repl.history() {
        // An input over several lines shows its first.
        let input = match entry.input.split_once('\n') {
            Some((first, _)) => fmt!("{first}…"),
            None => entry.input.C(),
        };
        let mut entry_lines = vec![Line::from(vec![Span::styled("> ", Style::default().fg(Color::Cyan)), Span::raw(input)])];

        match &entry.status {
            EntryStatus::Evaluating => entry_lines.push(marked("…", Color::Yellow, "evaluating")),
            EntryStatus::Success | EntryStatus::Error => {
                let eval = entry.eval_result.as_ref().expect("an answered entry has what it came to");
                entry_lines.extend(eval_lines(eval));
            }
        }

        // What was undone is the redo stack, and grayed out.
        if entry.undone {
            for span in entry_lines.iter_mut().flat_map(|line| line.spans.iter_mut()) {
                span.style = Style::default().fg(Color::DarkGray);
            }
        }
        lines.extend(entry_lines);
        lines.push(Line::from(""));
    }

    // The commands waiting to run, after everything that has, the redo stack
    // included.
    let mut queued = repl.queued().peekable();
    if queued.peek().is_some() {
        lines.push(Line::from(Span::styled("── queued ──", Style::default().fg(Color::DarkGray))));
        for input in queued {
            let input = match input.split_once('\n') {
                Some((first, _)) => fmt!("{first}…"),
                None => input.S(),
            };
            lines.push(Line::from(vec![
                Span::styled("  … ", Style::default().fg(Color::DarkGray)),
                Span::styled(input, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
            ]));
        }
    }

    render_scrolled_back(f, lines, block, area, repl.history_scroll_back())
}

/// Render lines that sit at the bottom of their pane unless the user paged up,
/// `scroll_back` lines.
fn render_scrolled_back(
    f: &mut Frame,
    lines: Vec<Line>,
    block: Block,
    area: Rect,
    scroll_back: usize,
) -> ScrollBackView {
    let content_height = lines.len();
    let viewport_height = area.height.saturating_sub(BORDERS) as usize;
    let bottom = content_height.saturating_sub(viewport_height);
    let scroll_offset = (bottom - scroll_back.min(bottom)) as u16;

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((scroll_offset, 0));

    f.render_widget(paragraph, area);

    // Render scrollbar.
    let mut scrollbar_state = ScrollbarState::new(content_height)
        .position(scroll_offset as usize);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None);

    f.render_stateful_widget(
        scrollbar,
        area.inner(ratatui::layout::Margin { vertical: 1, horizontal: 0 }),
        &mut scrollbar_state,
    );

    ScrollBackView { pane_lines: viewport_height, scroll_back_limit: bottom }
}

/// Render the input panel, its title saying what it is writing and the key
/// that submits it.
fn render_input<E: ReplExecutor>(f: &mut Frame, app: &RatatuiApp<E>, area: Rect) {
    let name = match app.repl.editing() {
        Some(Editing::Unit(unit)) => fmt!("Edit unit {unit}"),
        Some(Editing::Before(unit)) => fmt!("Insert before unit {unit}"),
        None => "Input".S(),
    };
    let keys = if app.repl.cancel_armed() {
        "Ctrl+C again to cancel"
    } else if app.repl.multiline_mode() {
        "Alt+Enter"
    } else {
        "Enter"
    };
    let title = fmt!("{name} [{keys}]");

    let focused = app.repl.focus() == Panel::Input;

    // An edit or an insert is magenta wherever the focus is, so the input is
    // not mistaken for a line to append. Otherwise multiline mode keeps its
    // yellow while the panel has focus.
    let border_style = if app.repl.editing().is_some() {
        Style::default().fg(Color::Magenta)
    } else if !focused {
        panel_border(&app.repl, Panel::Input)
    } else if app.repl.multiline_mode() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };

    let mut textarea = app.textarea().C();
    if !focused {
        // A cursor in a panel keys do not go to says they do.
        textarea.set_cursor_style(Style::default());
    }
    textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(border_style)
    );
    textarea.set_cursor_line_style(Style::default());

    f.render_widget(&textarea, area);
}

/// Render the environment tab: a table of the session's bindings.
fn render_environment<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>, area: Rect, block: Block) -> ScrollDownView {
    // The rows below the borders and the header.
    let pane_rows = area.height.saturating_sub(BORDERS + 1) as usize;
    let environment = repl.environment();
    if environment.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled("(no bindings)", Style::default().fg(Color::DarkGray))))
            .block(block);
        f.render_widget(empty, area);
        return ScrollDownView { pane_rows, scroll_limit: 0 };
    }

    let scroll_limit = environment.len().saturating_sub(pane_rows);
    let offset = repl.environment_scroll().min(scroll_limit);

    let heading = Style::default().fg(Color::Yellow);
    let header = Row::new(vec![
        Cell::from("Name").style(heading),
        Cell::from("Type").style(heading),
        Cell::from("Value").style(heading),
    ]);
    let rows = environment.iter().skip(offset).map(|binding| {
        Row::new(vec![
            Cell::from(binding.name.as_str()).style(Style::default().fg(Color::Cyan)),
            Cell::from(binding.ty.as_str()).style(Style::default().fg(Color::Yellow)),
            match &binding.value {
                Some(value) => Cell::from(value.as_str()),
                None => Cell::from("given away").style(given_away()),
            },
        ])
    });
    // A value is usually the longest of the three.
    let widths = [Constraint::Percentage(25), Constraint::Percentage(25), Constraint::Percentage(50)];
    f.render_widget(Table::new(rows, widths).header(header).block(block), area);

    let mut scrollbar_state = ScrollbarState::new(scroll_limit + 1).position(offset);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None);
    // Beside the bindings, below the top border and the header.
    let scrollbar_area = Rect { y: area.y + 2, height: pane_rows as u16, ..area };
    f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);

    ScrollDownView { pane_rows, scroll_limit }
}

/// Render the help tab: the help flowed for the pane's width, scrolled down
/// from its top.
fn render_help<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>, area: Rect, block: Block) -> ScrollDownView {
    // A margin either side of the text, inside the borders.
    let width = area.width.saturating_sub(BORDERS + 2) as usize;
    render_document(f, crate::help::lines(width), repl.help_scroll(), area, block)
}

/// Render the detail tab: what there is to know about the selected unit,
/// scrolled down from its top.
fn render_detail<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>, area: Rect, block: Block) -> ScrollDownView {
    let width = area.width.saturating_sub(BORDERS + 2) as usize;
    render_document(f, crate::detail::lines(repl, width), repl.detail_scroll(), area, block)
}

/// Render lines flowed for the pane, with a margin either side, scrolled
/// `scroll` lines down from the top.
fn render_document(f: &mut Frame, lines: Vec<Line>, scroll: usize, area: Rect, block: Block) -> ScrollDownView {
    let pane_rows = area.height.saturating_sub(BORDERS) as usize;
    let scroll_limit = lines.len().saturating_sub(pane_rows);
    let offset = scroll.min(scroll_limit);

    let paragraph = Paragraph::new(lines)
        .block(block.padding(ratatui::widgets::Padding::horizontal(1)))
        .scroll((offset as u16, 0));
    f.render_widget(paragraph, area);

    let mut scrollbar_state = ScrollbarState::new(scroll_limit + 1).position(offset);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None);
    f.render_stateful_widget(
        scrollbar,
        area.inner(ratatui::layout::Margin { vertical: 1, horizontal: 0 }),
        &mut scrollbar_state,
    );

    ScrollDownView { pane_rows, scroll_limit }
}

/// Render the dialog asking whether to exit.
fn render_exit_dialog(f: &mut Frame) {
    let lines = vec![
        Line::from(""),
        Line::from("Ctrl+C again to exit").style(Style::default().fg(HIGHLIGHT)),
        Line::from(""),
        Line::from("Any other key to go on").style(Style::default().fg(Color::DarkGray)),
    ];
    // Wide enough for its longest line, and its borders.
    let width = 28;
    let height = lines.len() as u16 + BORDERS;
    let screen = f.area();
    let area = Rect {
        x: screen.x + screen.width.saturating_sub(width) / 2,
        y: screen.y + screen.height.saturating_sub(height) / 2,
        width: width.min(screen.width),
        height: height.min(screen.height),
    };

    f.render_widget(Clear, area);
    let dialog = Paragraph::new(lines)
        .alignment(ratatui::layout::Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title("Exit"));
    f.render_widget(dialog, area);
}

/// Render the crash modal popup.
fn render_crash_modal<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>, msg: &str) {
    let area = centered_rect(60, 40, f.area());
    f.render_widget(Clear, area);

    let modal_block = Block::default()
        .borders(Borders::ALL)
        .title("Session Reset - Press Enter to Continue")
        .style(Style::default().fg(Color::Red));

    let mut lines = vec![
        Line::from(""),
        Line::from("The session was reset, forgetting everything entered up to"),
        Line::from("this input. Anything entered after it runs in the new session."),
        Line::from(""),
        Line::from(msg).style(Style::default().fg(Color::Yellow)),
    ];

    if let Some(log_path) = repl.stderr_log_path() {
        lines.push(Line::from(""));
        lines.push(Line::from("Engine output, and a panic's trace, were written to:"));
        lines.push(Line::from(log_path.display().S()).style(Style::default().fg(Color::Cyan)));
    }

    let modal = Paragraph::new(lines)
        .block(modal_block);

    f.render_widget(modal, area);
}

/// Render the modal shown when the history and the engine's session disagree.
fn render_session_mismatch_modal(f: &mut Frame, msg: &str) {
    let area = centered_rect(70, 40, f.area());

    f.render_widget(Clear, area);

    let modal_block = Block::default()
        .borders(Borders::ALL)
        .title("REPL Bug - Press Enter to Continue")
        .style(Style::default().fg(Color::Red));

    let lines = vec![
        Line::from(""),
        Line::from("The REPL lost track of which units the engine has."),
        Line::from("The code shown is the engine's; the history may not match it."),
        Line::from(""),
        Line::from(msg).style(Style::default().fg(Color::Yellow)),
    ];

    let modal = Paragraph::new(lines)
        .block(modal_block)
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(modal, area);
}

/// Render the modal shown when the engine failed to start or died.
fn render_engine_dead_modal<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>, msg: &str) {
    let area = centered_rect(70, 50, f.area());

    f.render_widget(Clear, area);

    let modal_block = Block::default()
        .borders(Borders::ALL)
        .title("Engine Gone - Press Enter to Exit")
        .style(Style::default().fg(Color::Red));

    let mut lines = vec![
        Line::from(""),
        Line::from("The repl engine is not running, so nothing can be evaluated."),
        Line::from(""),
    ];

    for line in msg.lines() {
        lines.push(Line::from(line).style(Style::default().fg(Color::Yellow)));
    }

    if let Some(log_path) = repl.stderr_log_path() {
        lines.push(Line::from(""));
        lines.push(Line::from("Engine output was written to:"));
        lines.push(Line::from(log_path.display().S()).style(Style::default().fg(Color::Cyan)));
    }

    let modal = Paragraph::new(lines)
        .block(modal_block)
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(modal, area);
}

/// A rectangle centered in `r`, these percentages of its width and height.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
