//! The help tab: a markdown document, flowed for the width of its pane.
//!
//! The document is `help.md`, at the root of the crate so that it is packaged
//! with it, read into the binary as it is built. It is parsed once, into
//! blocks of styled text, and each frame wraps the blocks to the pane's
//! width.

use rmx::prelude::*;
use rmx::comrak::{self, Arena, Node, Options};
use rmx::comrak::nodes::{ListType, NodeValue};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const HELP: &str = include_str!("../help.md");

/// A run of text in one style.
#[derive(Clone)]
struct Run {
    text: String,
    style: Style,
}

/// One block of the document.
enum Block {
    /// Text to wrap. The first line begins with `first`, and the lines after
    /// it with `rest`, which line the text up under itself.
    Text { first: String, rest: String, runs: Vec<Run> },
    /// Lines that are not wrapped, each after `indent`.
    Code { indent: String, lines: Vec<String> },
    /// The space between two blocks.
    Gap,
}

/// The help, as lines for a pane this wide.
pub fn lines(width: usize) -> Vec<Line<'static>> {
    static BLOCKS: std::sync::OnceLock<Vec<Block>> = std::sync::OnceLock::new();
    let blocks = BLOCKS.get_or_init(|| {
        let arena = Arena::new();
        let document = comrak::parse_document(&arena, HELP, &Options::default());
        let mut blocks = Vec::new();
        read_blocks(document, "", &mut blocks);
        blocks
    });

    let mut lines = Vec::new();
    for block in blocks {
        match block {
            Block::Text { first, rest, runs } => wrap(runs, first, rest, width, &mut lines),
            Block::Code { indent, lines: code } => {
                let style = Style::default().fg(Color::Green);
                lines.extend(code.iter().map(|line| Line::from(Span::styled(fmt!("{indent}{line}"), style))));
            }
            Block::Gap => lines.push(Line::from("")),
        }
    }
    lines
}

/// Read the blocks under `node`, each line of them after `indent`, with a
/// gap between each.
fn read_blocks<'a>(node: Node<'a>, indent: &str, blocks: &mut Vec<Block>) {
    for (index, child) in node.children().enumerate() {
        if index > 0 {
            blocks.push(Block::Gap);
        }
        read_block(child, indent, blocks);
    }
}

fn read_block<'a>(node: Node<'a>, indent: &str, blocks: &mut Vec<Block>) {
    let value = node.data.borrow().value.clone();
    match value {
        NodeValue::Heading(heading) => {
            let style = match heading.level {
                1 => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                _ => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            };
            blocks.push(Block::Text { first: indent.S(), rest: indent.S(), runs: runs(node, style) });
        }
        NodeValue::Paragraph => {
            blocks.push(Block::Text { first: indent.S(), rest: indent.S(), runs: runs(node, Style::default()) });
        }
        NodeValue::List(list) => {
            for (index, item) in node.children().enumerate() {
                if index > 0 && !list.tight {
                    blocks.push(Block::Gap);
                }
                let marker = match list.list_type {
                    ListType::Bullet => "• ".S(),
                    ListType::Ordered => fmt!("{}. ", list.start + index),
                };
                // The item's blocks line up after its marker, and the first
                // begins with it.
                let inner = fmt!("{indent}{}", " ".repeat(marker.chars().count()));
                let start = blocks.len();
                for (index, child) in item.children().enumerate() {
                    if index > 0 && !list.tight {
                        blocks.push(Block::Gap);
                    }
                    read_block(child, &inner, blocks);
                }
                if let Some(Block::Text { first, .. }) = blocks.get_mut(start) {
                    *first = fmt!("{indent}{marker}");
                }
            }
        }
        NodeValue::CodeBlock(code) => {
            let lines = code.literal.lines().map(S).collect();
            blocks.push(Block::Code { indent: fmt!("{indent}  "), lines });
        }
        NodeValue::BlockQuote => read_blocks(node, &fmt!("{indent}│ "), blocks),
        NodeValue::ThematicBreak => {
            let rule = Run { text: "────────".S(), style: Style::default().fg(Color::DarkGray) };
            blocks.push(Block::Text { first: indent.S(), rest: indent.S(), runs: vec![rule] });
        }
        other => todo!("help.md has a {other:?}, which the help does not lay out"),
    }
}

/// The runs of text under an inline node, in `style` and what it adds.
fn runs<'a>(node: Node<'a>, style: Style) -> Vec<Run> {
    let mut runs = Vec::new();
    for child in node.children() {
        let value = child.data.borrow().value.clone();
        match value {
            NodeValue::Text(text) => runs.push(Run { text: text.to_string(), style }),
            NodeValue::Code(code) => runs.push(Run { text: code.literal, style: style.fg(Color::Green) }),
            NodeValue::SoftBreak => runs.push(Run { text: " ".S(), style }),
            NodeValue::LineBreak => runs.push(Run { text: "\n".S(), style }),
            NodeValue::Emph => runs.extend(self::runs(child, style.add_modifier(Modifier::ITALIC))),
            NodeValue::Strong => runs.extend(self::runs(child, style.add_modifier(Modifier::BOLD))),
            NodeValue::Link(_) => runs.extend(self::runs(child, style.add_modifier(Modifier::UNDERLINED))),
            other => todo!("help.md has a {other:?} inline, which the help does not lay out"),
        }
    }
    runs
}

/// Text in one style wrapped at spaces to `width`, its first line after
/// `first` and the rest after `rest`.
pub(crate) fn wrapped(text: &str, style: Style, first: &str, rest: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    wrap(&[Run { text: text.S(), style }], first, rest, width, &mut lines);
    lines
}

/// Wrap runs at spaces to `width`, each line after its prefix.
///
/// A word longer than a whole line is broken where the line ends.
fn wrap(runs: &[Run], first: &str, rest: &str, width: usize, lines: &mut Vec<Line<'static>>) {
    let mut line: Vec<Span<'static>> = vec![Span::raw(first.S())];
    let mut used = Span::raw(first).width();
    // A space waiting to be written, if a word follows it on the same line.
    let mut space: Option<Style> = None;

    let end_line = |line: &mut Vec<Span<'static>>, used: &mut usize, lines: &mut Vec<Line<'static>>| {
        lines.push(Line::from(std::mem::take(line)));
        line.push(Span::raw(rest.S()));
        *used = Span::raw(rest).width();
    };

    for run in runs {
        for (index, piece) in run.text.split('\n').enumerate() {
            if index > 0 {
                end_line(&mut line, &mut used, lines);
                space = None;
            }
            for (index, word) in piece.split(' ').enumerate() {
                if index > 0 {
                    space = Some(run.style);
                }
                if word.is_empty() {
                    continue;
                }
                let mut word = word.S();
                loop {
                    let gap = usize::from(space.is_some());
                    let word_width = Span::raw(word.as_str()).width();
                    if used + gap + word_width <= width {
                        if let Some(style) = space.take() {
                            line.push(Span::styled(" ", style));
                        }
                        used += gap + word_width;
                        line.push(Span::styled(word, run.style));
                        break;
                    }
                    let start = Span::raw(rest).width();
                    if used > start {
                        // Not on this line, so on the next.
                        end_line(&mut line, &mut used, lines);
                        space = None;
                        continue;
                    }
                    // Longer than a line: as much as fits, and the rest on.
                    let fits = width.saturating_sub(used).max(1);
                    let split = word.char_indices().nth(fits).map_or(word.len(), |(at, _)| at);
                    let tail = word.split_off(split);
                    line.push(Span::styled(word, run.style));
                    end_line(&mut line, &mut used, lines);
                    space = None;
                    word = tail;
                    if word.is_empty() {
                        break;
                    }
                }
            }
        }
    }
    lines.push(Line::from(line));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The help lays out at any width without meeting markdown it does not
    /// know, and no line is wider than the pane.
    #[test]
    fn the_help_lays_out_at_any_width() {
        for width in [20, 40, 80, 200] {
            let lines = lines(width);
            assert!(lines.len() > 10, "{width}: {} lines", lines.len());
            for line in &lines {
                let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
                // A code block is not wrapped, so it is the one thing that may
                // run over.
                let code = line.spans.iter().any(|span| span.style.fg == Some(Color::Green));
                assert!(code || line.width() <= width, "{width}: {text:?} is {} wide", line.width());
            }
        }
    }

    /// A long paragraph wraps under itself, and a list item's lines after the
    /// first line up after its marker.
    #[test]
    fn text_wraps_under_itself() {
        let runs = vec![Run { text: "one two three four five six".S(), style: Style::default() }];
        let mut lines = Vec::new();
        wrap(&runs, "• ", "  ", 12, &mut lines);
        let text: Vec<String> = lines.iter()
            .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
            .collect();
        assert_eq!(text, vec!["• one two", "  three four", "  five six"]);
    }
}
