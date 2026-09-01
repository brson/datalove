//! Ratatui rendering functions for the REPL UI.

use rmx::prelude::*;
use datalove_repl as repl;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table},
    Frame,
};

use crate::{RatatuiApp, ReplApp, ReplExecutor, EntryStatus};

/// Render the UI.
pub fn ui<E: ReplExecutor>(f: &mut Frame, app: &mut RatatuiApp<E>) {
    // Three-panel layout: history (top), input (middle), debug (bottom).
    // Single-line mode: Input centered like a Cylon visor.
    // Multi-line mode: Input grows downward from center to 1/3 screen.

    let screen_height = f.area().height;

    let constraints = if app.multiline_mode() {
        // Multi-line mode: Keep history at same height, input grows to 1/3, debug compressed.
        let top_height = (screen_height.saturating_sub(3)) / 2;
        vec![
            Constraint::Length(top_height),  // History (same as single-line center point)
            Constraint::Percentage(33),      // Input (1/3 of screen)
            Constraint::Min(0),              // Debug (fills remaining space)
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

    // History panel - scrollable display of interactive cards.
    let history_view = render_history(f, &app.repl, chunks[0]);
    app.repl.record_history_view(history_view.pane_lines, history_view.scroll_back_limit);

    // Input panel - current text input with multiline indicators.
    render_input(f, app, chunks[1]);

    // Debug panel - table of variables and values.
    render_debug_pane(f, &app.repl, chunks[2]);

    // Render menu if open.
    if app.menu_is_open() {
        render_menu(f, &app.repl);
    }

    // Render crash modal if present (takes priority over menu).
    if let Some(msg) = app.repl.crash_modal_message() {
        render_crash_modal(f, &app.repl, msg);
    }

    // A dead engine outranks everything: nothing else can make progress.
    if let Some(msg) = app.repl.engine_dead_message() {
        render_engine_dead_modal(f, &app.repl, msg);
    }
}

/// Render one binding a fragment defined.
fn binding_line(binding: &repl::EvalBinding) -> Line<'_> {
    let check = ratatui::text::Span::styled("  ✓ ", Style::default().fg(Color::Green));

    match binding {
        repl::EvalBinding::Function { name } => Line::from(vec![
            check,
            ratatui::text::Span::raw("fun "),
            ratatui::text::Span::styled(name, Style::default().fg(Color::Cyan)),
        ]),
        repl::EvalBinding::Value { name, ty, value }
        | repl::EvalBinding::Slot { name, ty, value } => Line::from(vec![
            check,
            ratatui::text::Span::styled(name, Style::default().fg(Color::Cyan)),
            ratatui::text::Span::raw(": "),
            ratatui::text::Span::styled(ty, Style::default().fg(Color::Yellow)),
            ratatui::text::Span::raw(" = "),
            ratatui::text::Span::raw(value),
        ]),
    }
}

/// The shape of the history pane a frame laid out.
///
/// The renderer is the only thing that knows either number, and the app needs
/// both to bound scrolling.
struct HistoryView {
    /// Lines the pane showed.
    pane_lines: usize,
    /// The furthest up from the bottom the view can scroll.
    scroll_back_limit: usize,
}

/// Render the history panel with interactive cards.
fn render_history<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>, area: Rect) -> HistoryView {
    let history_block = Block::default()
        .borders(Borders::ALL)
        .title("History");

    let mut lines: Vec<Line> = Vec::new();

    // Startup compiles the system library and builds the native riders, which
    // takes seconds at best; say so, or every entry just reads "parsing...".
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
                        repl::Eval::Nothing => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  ✓ ",
                                    Style::default().fg(Color::Green),
                                ),
                                ratatui::text::Span::raw("nothing"),
                            ]));
                        }
                        repl::Eval::Success(bindings) => {
                            for binding in bindings {
                                lines.push(binding_line(binding));
                            }
                        }
                        repl::Eval::SuccessExpr(eval_expr) => {
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
                        repl::Eval::Error(e) => {
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
                        repl::Eval::CrashReset(_msg) => {
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
                let error_msg = if let Some(repl::Eval::Error(e)) = &entry.eval_result {
                    format!("error: {}", e)
                } else {
                    "error".S()
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

    // The pane sits at the bottom of the history unless the user paged up.
    let content_height = lines.len();
    let viewport_height = area.height.saturating_sub(2) as usize; // Subtract borders.
    let bottom = content_height.saturating_sub(viewport_height);
    let scroll_offset = (bottom - repl.history_scroll_back().min(bottom)) as u16;

    let history = Paragraph::new(lines)
        .block(history_block)
        .scroll((scroll_offset, 0));

    f.render_widget(history, area);

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

    HistoryView { pane_lines: viewport_height, scroll_back_limit: bottom }
}

/// Render the input panel with multiline indicators.
fn render_input<E: ReplExecutor>(f: &mut Frame, app: &RatatuiApp<E>, area: Rect) {
    let title = if app.multiline_mode() {
        "Input [Alt+Enter]"
    } else {
        "Input [Enter]"
    };

    let border_style = if app.multiline_mode() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };

    let mut textarea = app.textarea().C();
    textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(border_style)
    );
    textarea.set_cursor_line_style(Style::default());

    f.render_widget(&textarea, area);
}

fn render_debug_pane<E: ReplExecutor>(f: &mut Frame, repl: &ReplApp<E>, area: Rect) {
    let debug_block = Block::default()
        .borders(Borders::ALL)
        .title("Environment");

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
    } else {
        // Create header row.
        let header = Row::new(vec![
            Cell::from("Name").style(Style::default().fg(Color::Yellow)),
            Cell::from("Type").style(Style::default().fg(Color::Yellow)),
            Cell::from("Value").style(Style::default().fg(Color::Yellow)),
        ])
        .height(1);

        // Create data rows.
        let rows = repl.environment().iter().map(|(name, ty, value)| {
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
        .title("💥 Engine Crash - Press Enter to Continue")
        .style(Style::default().fg(Color::Red));

    let mut lines = vec![
        Line::from(""),
        Line::from("The REPL engine encountered a panic and has been reset."),
        Line::from("All history and environment has been cleared."),
        Line::from(""),
        Line::from("Crash details:"),
        Line::from(""),
        Line::from(msg).style(Style::default().fg(Color::Yellow)),
    ];

    // Add stderr log path if available.
    if let Some(log_path) = repl.stderr_log_path() {
        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from("Full panic trace written to:"));
        lines.push(Line::from(log_path.display().S()).style(Style::default().fg(Color::Cyan)));
    }

    let modal = Paragraph::new(lines)
        .block(modal_block);

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
