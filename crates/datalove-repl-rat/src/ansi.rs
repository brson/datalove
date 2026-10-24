//! Text with ANSI colour escapes, as styled lines.
//!
//! A diagnostic is rendered by ariadne for a terminal, colour and all, and the
//! Detail tab draws it into a pane, where an escape written as text would be
//! garbage. So the SGR sequences -- `ESC [ ... m`, which are all ariadne
//! writes -- become styles, and any other escape is dropped.

use rmx::prelude::*;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// The lines of `text`, each run between escapes a span in the style the
/// escapes before it set. A style carries over from one line to the next, as
/// it does on a terminal.
pub(crate) fn lines(text: &str) -> Vec<Line<'static>> {
    let mut style = Style::default();
    text.lines().map(|line| {
        let mut spans = Vec::new();
        let mut rest = line;
        while let Some(at) = rest.find('\x1b') {
            if at > 0 {
                spans.push(Span::styled(rest[..at].S(), style));
            }
            let (sequence, after) = escape(&rest[at..]);
            if let Some(params) = sequence {
                style = apply(style, params);
            }
            rest = after;
        }
        if !rest.is_empty() {
            spans.push(Span::styled(rest.S(), style));
        }
        Line::from(spans)
    }).collect()
}

/// The escape at the start of `text`: its SGR parameters, if it is one, and
/// the text after it.
fn escape(text: &str) -> (Option<&str>, &str) {
    let Some(csi) = text.strip_prefix("\x1b[") else {
        // Not a control sequence; only the escape character goes.
        return (None, &text[1..]);
    };
    // A control sequence ends at its first byte from `@` to `~`.
    match csi.find(|c: char| ('@'..='~').contains(&c)) {
        Some(end) => {
            let params = &csi[..end];
            let after = &csi[end + 1..];
            match &csi[end..end + 1] {
                "m" => (Some(params), after),
                _ => (None, after),
            }
        }
        None => (None, ""),
    }
}

/// `style` with an SGR sequence's parameters applied.
fn apply(mut style: Style, params: &str) -> Style {
    let codes: Vec<u16> = params.split(';').map(|code| code.parse().unwrap_or(0)).collect();
    let mut codes = codes.into_iter();
    while let Some(code) = codes.next() {
        style = match code {
            0 => Style::default(),
            1 => style.add_modifier(Modifier::BOLD),
            2 => style.add_modifier(Modifier::DIM),
            3 => style.add_modifier(Modifier::ITALIC),
            4 => style.add_modifier(Modifier::UNDERLINED),
            22 => style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style.remove_modifier(Modifier::ITALIC),
            24 => style.remove_modifier(Modifier::UNDERLINED),
            30..=37 => style.fg(basic(code - 30, false)),
            90..=97 => style.fg(basic(code - 90, true)),
            39 => style.fg(Color::Reset),
            40..=47 => style.bg(basic(code - 40, false)),
            100..=107 => style.bg(basic(code - 100, true)),
            49 => style.bg(Color::Reset),
            38 | 48 => {
                let Some(color) = extended(&mut codes) else {
                    return style;
                };
                match code {
                    38 => style.fg(color),
                    _ => style.bg(color),
                }
            }
            _ => style,
        };
    }
    style
}

/// A 256-colour (`5;n`) or true-colour (`2;r;g;b`) colour, from the codes
/// after a 38 or 48.
fn extended(codes: &mut impl Iterator<Item = u16>) -> Option<Color> {
    let byte = |code: u16| u8::try_from(code).ok();
    match codes.next()? {
        5 => Some(Color::Indexed(byte(codes.next()?)?)),
        2 => Some(Color::Rgb(byte(codes.next()?)?, byte(codes.next()?)?, byte(codes.next()?)?)),
        _ => None,
    }
}

/// One of the eight colours, or its bright form.
fn basic(index: u16, bright: bool) -> Color {
    match (index, bright) {
        (0, false) => Color::Black,
        (1, false) => Color::Red,
        (2, false) => Color::Green,
        (3, false) => Color::Yellow,
        (4, false) => Color::Blue,
        (5, false) => Color::Magenta,
        (6, false) => Color::Cyan,
        (7, false) => Color::Gray,
        (0, true) => Color::DarkGray,
        (1, true) => Color::LightRed,
        (2, true) => Color::LightGreen,
        (3, true) => Color::LightYellow,
        (4, true) => Color::LightBlue,
        (5, true) => Color::LightMagenta,
        (6, true) => Color::LightCyan,
        (7, true) => Color::White,
        _ => unreachable!("an index from a range of eight"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(line: &Line<'_>) -> Vec<(String, Style)> {
        line.spans.iter().map(|span| (span.content.to_string(), span.style)).collect()
    }

    /// Each run is in the style the escapes before it set, a reset goes back
    /// to none, and the escapes themselves are not text.
    #[test]
    fn escapes_become_styles() {
        let lines = lines("\x1b[31m[F001] Error:\x1b[0m cannot find \x1b[38;5;201mx\x1b[0m");
        assert_eq!(runs(&lines[0]), vec![
            ("[F001] Error:".S(), Style::default().fg(Color::Red)),
            (" cannot find ".S(), Style::default()),
            ("x".S(), Style::default().fg(Color::Indexed(201))),
        ]);
    }

    /// A style set on one line holds on the next, and a sequence that is not
    /// a colour leaves only its text.
    #[test]
    fn a_style_carries_over_and_other_escapes_go() {
        let lines = lines("\x1b[1;38;2;1;2;3mone\ntwo\x1b[2K three");
        let style = Style::default().add_modifier(Modifier::BOLD).fg(Color::Rgb(1, 2, 3));
        assert_eq!(runs(&lines[0]), vec![("one".S(), style)]);
        assert_eq!(runs(&lines[1]), vec![("two".S(), style), (" three".S(), style)]);
    }
}
