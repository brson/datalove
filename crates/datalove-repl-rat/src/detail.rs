//! The detail tab: what there is to know about the selected unit.
//!
//! What the app keeps -- the unit's status, what the last step did with it,
//! and what it came to -- is shown at once. What only the session knows --
//! its diagnostics, what its bindings hold now, and what it reads and writes
//! -- is shown when the session has said, which it is asked whenever the
//! selection or the session changes.

use rmx::prelude::*;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::help::wrapped;
use crate::render::{bar_glyph, marked, outcome, status_color};
use crate::repl::app::{ReplApp, ReplExecutor, UnitStatus};
use crate::repl::script;

/// The detail of the selected unit, as lines for a pane this wide.
pub(crate) fn lines<E: ReplExecutor>(repl: &ReplApp<E>, width: usize) -> Vec<Line<'static>> {
    let number = repl.selected_unit().expect("the tab is there only while there are units");
    let unit = repl.code_units().into_iter()
        .find(|unit| unit.number == Some(number))
        .expect("the selected unit is shown");

    // The header, with the unit's bar beside it: the left half the status,
    // the corners what the last step did.
    let bar = Style::default().fg(status_color(unit.status));
    let changed = unit.compiled || unit.ran;
    let last_line = usize::from(changed);
    let mut lines = vec![Line::from(vec![
        Span::styled(bar_glyph(unit.compiled, unit.ran, 0, last_line), bar),
        Span::styled(fmt!("unit {number}"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(status_words(unit.status), bar),
    ])];
    if changed {
        let step = match (unit.compiled, unit.ran) {
            (true, true) => "the last step compiled and ran it",
            (true, false) => "the last step compiled it, and did not run it",
            (false, true) => "the last step ran it, and did not compile it",
            (false, false) => unreachable!("it changed"),
        };
        lines.push(Line::from(vec![
            Span::styled(bar_glyph(unit.compiled, unit.ran, 1, last_line), bar),
            Span::styled(step, Style::default().fg(Color::DarkGray)),
        ]));
    }

    let detail = repl.detail();
    let eval = repl.outcome(number);

    // What it came to. A unit that failed is told by its diagnostics where
    // the engine has them, and by its error where it does not.
    let diagnostics = detail.map(|detail| detail.diagnostics.as_str()).filter(|text| !text.is_empty());
    match (eval, diagnostics) {
        (Some(script::Eval::NotCompiled(_)), Some(diagnostics)) => {
            heading(&mut lines, "Diagnostics");
            lines.extend(crate::ansi::lines(diagnostics));
        }
        (Some(script::Eval::Ran(_)), _) | (None, _) => {}
        (Some(eval), _) => {
            lines.push(Line::from(""));
            let (mark, color, said) = outcome(eval);
            lines.extend(wrapped(&said, Style::default().fg(color), &fmt!("{mark} "), "  ", width));
        }
    }

    let Some(detail) = detail else {
        lines.push(Line::from(""));
        lines.push(marked("…", Color::DarkGray, "asking the engine"));
        return lines;
    };

    // Each binding's type is what the unit's run said; what it holds is what
    // it holds now, which a later unit may have changed.
    let defined: &[script::EvalBinding] = match eval {
        Some(script::Eval::Ran(bindings)) => bindings,
        _ => &[],
    };
    if !detail.bindings.is_empty() {
        heading(&mut lines, "Bindings");
        for (name, value) in &detail.bindings {
            let ty = defined.iter().find_map(|binding| match binding {
                script::EvalBinding::Value { name: bound, ty, .. }
                | script::EvalBinding::Slot { name: bound, ty, .. } if bound == name => Some(ty.as_str()),
                _ => None,
            });
            let mut spans = vec![Span::styled(name.C(), Style::default().fg(Color::Cyan))];
            if let Some(ty) = ty {
                spans.push(Span::raw(": "));
                spans.push(Span::styled(ty.S(), Style::default().fg(Color::Yellow)));
            }
            match value {
                Some(value) => {
                    spans.push(Span::raw(" = "));
                    spans.push(Span::raw(value.C()));
                }
                None => spans.push(Span::styled(" -- given away", crate::render::given_away())),
            }
            lines.push(Line::from(spans));
        }
    }
    let functions: Vec<&str> = defined.iter().filter_map(|binding| match binding {
        script::EvalBinding::Function { name } => Some(name.as_str()),
        _ => None,
    }).collect();
    if !functions.is_empty() {
        heading(&mut lines, "Functions");
        lines.extend(functions.iter().map(|name| Line::from(vec![
            Span::raw("fun "),
            Span::styled(name.S(), Style::default().fg(Color::Cyan)),
        ])));
    }

    let accesses = [
        ("provides", &detail.provides),
        ("reads", &detail.reads),
        ("takes", &detail.takes),
        ("overwrites", &detail.overwrites),
        ("updates", &detail.updates),
        ("revives", &detail.revives),
    ];
    if accesses.iter().any(|(_, names)| !names.is_empty()) {
        heading(&mut lines, "Names");
        for (what, names) in accesses {
            if !names.is_empty() {
                let label = fmt!("{what:<11}");
                lines.extend(wrapped(&names.join(", "), Style::default(), &label, &" ".repeat(label.len()), width));
            }
        }
    }
    lines
}

/// A heading, after a blank line.
fn heading(lines: &mut Vec<Line<'static>>, text: &'static str) {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(text, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
}

/// How a unit fared, in words.
fn status_words(status: UnitStatus) -> &'static str {
    match status {
        UnitStatus::Waiting => "waiting on the engine",
        UnitStatus::Ran => "ran",
        UnitStatus::NotCompiled => "did not compile",
        UnitStatus::ReturnedEarly => "returned early",
        UnitStatus::Blocked => "blocked",
    }
}
