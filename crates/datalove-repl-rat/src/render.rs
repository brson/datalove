//! Ratatui rendering functions for the REPL UI.

use rmx::prelude::*;
use datalove_repl as repl;
use repl::script;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table},
    Frame,
};

use crate::{RatatuiApp, ReplApp, ReplExecutor, EntryStatus, Panel, Tab, UnitStep};

/// The color of what is selected: the tab shown, and the code unit.
const HIGHLIGHT: Color = Color::Yellow;

/// Render the UI.
pub fn ui<E: ReplExecutor>(f: &mut Frame, app: &mut RatatuiApp<E>) {
    // Three-panel layout: code (top), input (middle), tabs (bottom).
    // Single-line mode: Input centered like a Cylon visor.
    // Multi-line mode: Input grows downward from center to 1/3 screen.

    let screen_height = f.area().height;

    let constraints = if app.multiline_mode() {
        // Multi-line mode: Keep code at same height, input grows to 1/3, tabs compressed.
        let top_height = (screen_height.saturating_sub(3)) / 2;
        vec![
            Constraint::Length(top_height),  // Code (same as single-line center point)
            Constraint::Percentage(33),      // Input (1/3 of screen)
            Constraint::Min(0),              // Tabs (fills remaining space)
        ]
    } else {
        // Single-line mode: Input centered vertically.
        vec![
            Constraint::Fill(1),      // Top half (centers input)
            Constraint::Length(3),    // Input (3 lines: 1 text + 2 borders)
            Constraint::Fill(1),      // Bottom half (centers input)
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    let (code_view, unit_lines) = render_code(f, &app.repl, chunks[0]);
    app.repl.record_code_view(code_view.pane_lines, code_view.scroll_back_limit, unit_lines);

    // Input panel - current text input with multiline indicators.
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
    }

    // Render menu if open.
    if app.menu_is_open() {
        render_menu(f, &app.repl);
    }

    // Render crash modal if present (takes priority over menu).
    if let Some(msg) = app.repl.crash_modal_message() {
        render_crash_modal(f, &app.repl, msg);
    }

    // A bug in the app's bookkeeping, over the reset that may have caused it.
    if let Some(msg) = app.repl.session_mismatch_message() {
        render_session_mismatch_modal(f, msg);
    }

    // A dead engine outranks everything: nothing else can make progress.
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

/// Render one binding a fragment defined.
fn binding_line(binding: &script::EvalBinding) -> Line<'_> {
    let check = ratatui::text::Span::styled("  ✓ ", Style::default().fg(Color::Green));

    match binding {
        script::EvalBinding::Function { name } => Line::from(vec![
            check,
            ratatui::text::Span::raw("fun "),
            ratatui::text::Span::styled(name, Style::default().fg(Color::Cyan)),
        ]),
        script::EvalBinding::Value { name, ty, value }
        | script::EvalBinding::Slot { name, ty, value } => Line::from(vec![
            check,
            ratatui::text::Span::styled(name, Style::default().fg(Color::Cyan)),
            ratatui::text::Span::raw(": "),
            ratatui::text::Span::styled(ty, Style::default().fg(Color::Yellow)),
            ratatui::text::Span::raw(" = "),
            ratatui::text::Span::raw(value),
        ]),
    }
}

/// One line saying what a unit an edit reached came to.
fn report_line(report: &repl::UnitReport) -> Line<'_> {
    let (mark, color, said) = match &report.eval {
        script::Eval::Nothing => ("✓", Color::Green, "ran".S()),
        script::Eval::Success(bindings) => {
            let bound: Vec<String> = bindings.iter().map(|binding| match binding {
                script::EvalBinding::Function { name } => fmt!("fun {name}"),
                script::EvalBinding::Value { name, value, .. }
                | script::EvalBinding::Slot { name, value, .. } => fmt!("{name} = {value}"),
            }).collect();
            ("✓", Color::Green, bound.join(", "))
        }
        script::Eval::SuccessExpr(expr) => ("⇒", Color::Green, fmt!("{} = {}", expr.ty, expr.value)),
        script::Eval::Error(e) => ("✗", Color::Red, fmt!("error: {e}")),
        script::Eval::ReturnedEarly(returned) => ("✗", Color::Yellow, fmt!("returned early: {returned}")),
        script::Eval::Blocked { on } => ("✗", Color::Magenta, fmt!("not run: it reads from units {on:?}")),
        script::Eval::ErrorReset(_) | script::Eval::CrashReset(_) => {
            unreachable!("a reset takes the entry with it")
        }
    };
    Line::from(vec![
        ratatui::text::Span::styled(fmt!("  {mark} "), Style::default().fg(color)),
        ratatui::text::Span::styled(fmt!("unit {}: ", report.unit), Style::default().fg(Color::DarkGray)),
        ratatui::text::Span::styled(said, Style::default().fg(color)),
    ])
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

/// The shape of the environment pane a frame laid out.
struct EnvironmentView {
    /// Rows of bindings the pane showed, not counting the header.
    pane_rows: usize,
    /// The furthest down from the top the view can scroll.
    scroll_limit: usize,
}

/// The color of a unit's bar, for what it did in the last step.
fn step_color(step: UnitStep) -> Color {
    match step {
        UnitStep::Pending => Color::Cyan,
        UnitStep::Untouched => Color::DarkGray,
        UnitStep::Ran => Color::Green,
        UnitStep::Failed => Color::Red,
        UnitStep::ReturnedEarly => Color::Yellow,
        UnitStep::Blocked => Color::Magenta,
    }
}

/// Render the code panel: every unit's source, in order, each beside a bar
/// colored for what the last step did with it, and the selected unit
/// highlighted while the panel has focus.
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
        lines.push(Line::from(vec![
            ratatui::text::Span::styled(
                "  ⏱ ",
                Style::default().fg(Color::Yellow),
            ),
            ratatui::text::Span::styled(
                "starting engine...",
                Style::default().fg(Color::Yellow),
            ),
        ]));
        lines.push(Line::from(""));
    }

    let units = repl.code_units();
    if units.is_empty() {
        lines.push(Line::from(ratatui::text::Span::styled(
            "(no code yet)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let selected = match repl.focus() {
        Panel::Code => repl.selected_unit(),
        _ => None,
    };
    let separator = "─".repeat(area.width.saturating_sub(2) as usize);
    // Every unit's number in a column as wide as the widest.
    let number_width = units.iter()
        .filter_map(|unit| unit.number)
        .max()
        .map_or(1, |number| number.to_string().len());
    let mut unit_lines = Vec::new();
    for (index, unit) in units.iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(ratatui::text::Span::styled(
                separator.C(),
                Style::default().fg(Color::DarkGray),
            )));
        }
        let bar = Style::default().fg(step_color(unit.step));
        let text = if selected.is_some() && selected == unit.number {
            Style::default().fg(HIGHLIGHT)
        } else {
            Style::default()
        };
        let start = lines.len();
        for (line_index, line) in unit.source.lines().enumerate() {
            // The number on the unit's first line; a unit on its way to the
            // engine has none yet.
            let number = match (line_index, unit.number) {
                (0, Some(number)) => fmt!("{number:>number_width$} "),
                _ => " ".repeat(number_width + 1),
            };
            lines.push(Line::from(vec![
                ratatui::text::Span::styled(number, Style::default().fg(Color::DarkGray)),
                ratatui::text::Span::styled("▌ ", bar),
                ratatui::text::Span::styled(line, text),
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
    let mut tabs = vec![ratatui::text::Span::raw(" ")];
    for (index, tab) in Tab::ALL.into_iter().enumerate() {
        if index > 0 {
            tabs.push(ratatui::text::Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }
        let style = if tab == repl.tab() {
            Style::default().fg(HIGHLIGHT).add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        tabs.push(ratatui::text::Span::styled(tab.title(), style));
    }
    tabs.push(ratatui::text::Span::raw(" "));

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
        // Input line with prompt.
        // Truncate multiline input to first line with ellipsis.
        let display_input = if entry.input.contains('\n') {
            let first_line = entry.input.lines().next().unwrap_or("");
            format!("{}…", first_line)
        } else {
            entry.input.C()
        };

        lines.push(Line::from(vec![
            ratatui::text::Span::styled(
                "> ",
                Style::default().fg(Color::Cyan),
            ),
            ratatui::text::Span::raw(display_input),
        ]));

        // Output/status line.
        match &entry.status {
            EntryStatus::Parsing => {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  ⏱ ",
                        Style::default().fg(Color::Yellow),
                    ),
                    ratatui::text::Span::styled(
                        "parsing...",
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
            }
            EntryStatus::Evaluating { .. } => {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  ⏱ ",
                        Style::default().fg(Color::Yellow),
                    ),
                    ratatui::text::Span::styled(
                        "evaluating...",
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
            }
            EntryStatus::Success => {
                if let Some(eval) = &entry.eval_result {
                    match eval {
                        repl::Eval::Unit(script::Eval::Nothing) => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  ✓ ",
                                    Style::default().fg(Color::Green),
                                ),
                                ratatui::text::Span::raw("nothing"),
                            ]));
                        }
                        repl::Eval::Unit(script::Eval::Success(bindings)) => {
                            for binding in bindings {
                                lines.push(binding_line(binding));
                            }
                        }
                        repl::Eval::Unit(script::Eval::SuccessExpr(eval_expr)) => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  ⇒ ",
                                    Style::default().fg(Color::Green),
                                ),
                                ratatui::text::Span::styled(
                                    &eval_expr.ty,
                                    Style::default().fg(Color::Yellow),
                                ),
                                ratatui::text::Span::raw(" = "),
                                ratatui::text::Span::raw(&eval_expr.value),
                            ]));
                        }
                        repl::Eval::Edited(reports) => {
                            if reports.is_empty() {
                                lines.push(Line::from(vec![
                                    ratatui::text::Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                                    ratatui::text::Span::raw("no other unit reran"),
                                ]));
                            }
                            lines.extend(reports.iter().map(report_line));
                        }
                        repl::Eval::Saved { path, inputs } => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  ✓ ",
                                    Style::default().fg(Color::Green),
                                ),
                                ratatui::text::Span::raw(format!("saved {inputs} inputs to {path}")),
                            ]));
                        }
                        repl::Eval::Unit(script::Eval::Error(e)) | repl::Eval::CommandError(e) => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  ✗ ",
                                    Style::default().fg(Color::Red),
                                ),
                                ratatui::text::Span::styled(
                                    format!("error: {e}"),
                                    Style::default().fg(Color::Red),
                                ),
                            ]));
                        }
                        repl::Eval::CallerInterpret(command) => {
                            match command {
                                repl::ReplCommand::Help => {
                                    lines.push(Line::from(vec![
                                        ratatui::text::Span::styled(
                                            "  ℹ ",
                                            Style::default().fg(Color::Blue),
                                        ),
                                        ratatui::text::Span::raw("help"),
                                    ]));
                                }
                                _ => {
                                    lines.push(Line::from(vec![
                                        ratatui::text::Span::styled(
                                            "  ℹ ",
                                            Style::default().fg(Color::Blue),
                                        ),
                                        ratatui::text::Span::raw("(repl command)"),
                                    ]));
                                }
                            }
                        }
                        repl::Eval::Unit(script::Eval::ErrorReset(e)) => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  ✗ ",
                                    Style::default().fg(Color::Red),
                                ),
                                ratatui::text::Span::styled(
                                    format!("error: {e} (session reset)"),
                                    Style::default().fg(Color::Red),
                                ),
                            ]));
                        }
                        repl::Eval::Unit(script::Eval::ReturnedEarly(_) | script::Eval::Blocked { .. }) => {
                            unreachable!("the app reports a unit that did not run as an error")
                        }
                        repl::Eval::Unit(script::Eval::CrashReset(_msg)) => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  💥 ",
                                    Style::default().fg(Color::Red),
                                ),
                                ratatui::text::Span::styled(
                                    "crash reset",
                                    Style::default().fg(Color::Red),
                                ),
                            ]));
                        }
                    }
                }
            }
            EntryStatus::Error => {
                // Try to extract error message from eval_result.
                let error_msg = match &entry.eval_result {
                    Some(repl::Eval::Unit(script::Eval::Error(e)) | repl::Eval::CommandError(e)) => format!("error: {}", e),
                    Some(repl::Eval::Unit(script::Eval::ReturnedEarly(returned))) => {
                        format!("returned early: {returned}")
                    }
                    Some(repl::Eval::Unit(script::Eval::Blocked { on })) => {
                        format!("not run: it reads from units {on:?}, which did not finish")
                    }
                    _ => "error".S(),
                };

                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  ✗ ",
                        Style::default().fg(Color::Red),
                    ),
                    ratatui::text::Span::styled(
                        error_msg,
                        Style::default().fg(Color::Red),
                    ),
                ]));
            }
            EntryStatus::Empty => {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  · ",
                        Style::default().fg(Color::DarkGray),
                    ),
                    ratatui::text::Span::styled(
                        "(empty)",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
            EntryStatus::ReadMultiline => {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  → ",
                        Style::default().fg(Color::DarkGray),
                    ),
                    ratatui::text::Span::styled(
                        "read-multiline",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        // Separator between entries.
        lines.push(Line::from(""));
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
    let viewport_height = area.height.saturating_sub(2) as usize; // Subtract borders.
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

/// Render the input panel with multiline indicators.
fn render_input<E: ReplExecutor>(f: &mut Frame, app: &RatatuiApp<E>, area: Rect) {
    let title = if app.multiline_mode() {
        "Input [Alt+Enter]"
    } else {
        "Input [Enter]"
    };

    let focused = app.focus() == Panel::Input;

    // Multiline mode keeps its yellow while the panel has focus.
    let border_style = if !focused {
        panel_border(&app.repl, Panel::Input)
    } else if app.multiline_mode() {
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
fn render_environment<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>, area: Rect, debug_block: Block) -> EnvironmentView {
    if repl.environment().is_empty() {
        // Display empty state message as a paragraph.
        let lines = vec![
            Line::from(vec![
                ratatui::text::Span::styled(
                    "(no variables defined)",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ];
        let debug = Paragraph::new(lines)
            .block(debug_block);
        f.render_widget(debug, area);

        EnvironmentView { pane_rows: area.height.saturating_sub(3) as usize, scroll_limit: 0 }
    } else {
        // Less the borders and the header.
        let pane_rows = area.height.saturating_sub(3) as usize;
        let row_count = repl.environment().len();
        let scroll_limit = row_count.saturating_sub(pane_rows);
        let offset = repl.environment_scroll().min(scroll_limit);

        // Create header row.
        let header = Row::new(vec![
            Cell::from("Name").style(Style::default().fg(Color::Yellow)),
            Cell::from("Type").style(Style::default().fg(Color::Yellow)),
            Cell::from("Value").style(Style::default().fg(Color::Yellow)),
        ])
        .height(1);

        // Create data rows.
        let rows = repl.environment().iter().skip(offset).map(|(name, ty, value)| {
            Row::new(vec![
                Cell::from(name.as_str()).style(Style::default().fg(Color::Cyan)),
                Cell::from(ty.as_str()).style(Style::default().fg(Color::Yellow)),
                Cell::from(value.as_str()),
            ])
            .height(1)
        });

        // Create table with column constraints.
        let widths = [
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(50),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .block(debug_block);

        f.render_widget(table, area);

        let mut scrollbar_state = ScrollbarState::new(scroll_limit + 1).position(offset);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);

        // Beside the bindings, below the top border and the header.
        let scrollbar_area = Rect {
            y: area.y + 2,
            height: pane_rows as u16,
            ..area
        };
        f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);

        EnvironmentView { pane_rows, scroll_limit }
    }
}

/// Render the ESC menu popup.
fn render_menu<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>) {
    let area = centered_rect(20, 20, f.area());

    // Clear the background.
    f.render_widget(Clear, area);

    // Menu block.
    let menu_block = Block::default()
        .borders(Borders::ALL)
        .title("Menu");

    let menu_items = vec![
        if repl.menu_selection() == 0 {
            Line::from("> Resume").style(Style::default().fg(Color::Yellow))
        } else {
            Line::from("  Resume")
        },
        if repl.menu_selection() == 1 {
            Line::from("> Exit").style(Style::default().fg(Color::Yellow))
        } else {
            Line::from("  Exit")
        },
    ];

    let menu = Paragraph::new(menu_items)
        .block(menu_block);

    f.render_widget(menu, area);
}

/// Render the crash modal popup.
fn render_crash_modal<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>, msg: &str) {
    let area = centered_rect(60, 40, f.area());

    // Clear the background.
    f.render_widget(Clear, area);

    // Crash modal block.
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

    // Add stderr log path if available.
    if let Some(log_path) = repl.stderr_log_path() {
        lines.push(Line::from(""));
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
        Line::from("The REPL lost track of which code units the engine has."),
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

/// Create a centered rectangle.
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
