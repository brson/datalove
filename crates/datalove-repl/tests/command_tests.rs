//! How inputs naming REPL commands are read.

use datalove_repl::script::Unit;
use datalove_repl::{Command, Input, InputParse, Place, ReplCommand, parse_input};

fn command(input: &str) -> ReplCommand {
    match parse_input(Input::Multiline(input.to_string())) {
        InputParse::Command(Command::ReplCommand(command)) => command,
        other => panic!("{input:?} is not a REPL command: {other:?}"),
    }
}

fn fragment(source: &str) -> Unit {
    Unit::Fragment(source.to_string())
}

#[test]
fn edit_takes_a_number_and_the_source_after_it() {
    assert!(matches!(command("/edit 3 let a = 2"), ReplCommand::Edit { unit: 3, source } if source == fragment("let a = 2")));
    assert!(matches!(
        command("/edit 12 a + 1"),
        ReplCommand::Edit { unit: 12, source: Unit::Expression(source) } if source == "a + 1",
    ));
}

/// The source goes on over the lines after the number, keeping their
/// indentation, whether it starts on the command's line or the next.
#[test]
fn an_edit_source_spans_lines() {
    let fun = "fun foo()\n  ret\nend fun";
    assert!(matches!(command(&format!("/edit 2 {fun}")), ReplCommand::Edit { unit: 2, source } if source == fragment(fun)));
    assert!(matches!(command(&format!("/edit 2\n{fun}")), ReplCommand::Edit { unit: 2, source } if source == fragment(fun)));
}

#[test]
fn insert_says_which_side() {
    assert!(matches!(
        command("/insert before 1 let x = 1"),
        ReplCommand::Insert { place: Place::Before, unit: 1, source } if source == fragment("let x = 1"),
    ));
    assert!(matches!(
        command("/insert after 4 x"),
        ReplCommand::Insert { place: Place::After, unit: 4, source: Unit::Expression(_) },
    ));
}

#[test]
fn delete_takes_a_number() {
    assert!(matches!(command("/delete 7"), ReplCommand::Delete { unit: 7 }));
}

/// A comment above a command leaves it a command.
#[test]
fn a_comment_before_a_command() {
    assert!(matches!(command("// fix the typo\n/edit 1 let a = 1"), ReplCommand::Edit { unit: 1, .. }));
    assert!(matches!(command("// start again\n/reset"), ReplCommand::Reset));
}

#[test]
fn malformed_commands_say_how_they_are_used() {
    for (input, usage) in [
        ("/edit", "usage: /edit N <source>"),
        ("/edit 3", "usage: /edit N <source>"),
        ("/edit x let a = 1", "usage: /edit N <source>"),
        ("/edit 3 /reset", "usage: /edit N <source>"),
        ("/delete", "usage: /delete N"),
        ("/delete 3 4", "usage: /delete N"),
        ("/insert 2 let a = 1", "usage: /insert before|after N <source>"),
        ("/insert sideways 2 a", "usage: /insert before|after N <source>"),
        ("/insert after 2", "usage: /insert before|after N <source>"),
        ("/reset now", "usage: /reset"),
    ] {
        match command(input) {
            ReplCommand::Usage(said) => assert_eq!(said, usage, "{input}"),
            other => panic!("{input:?} read as {other:?}"),
        }
    }
    assert!(matches!(command("/bogus 1"), ReplCommand::Unknown));
}

fn unit(input: &str) -> Unit {
    match parse_input(Input::Input(input.to_string())) {
        InputParse::Command(Command::Unit(unit)) => unit,
        other => panic!("{input:?} is not a unit: {other:?}"),
    }
}

/// A name that starts with a keyword is a name: underscores go on with it,
/// as the lexer's do.
#[test]
fn a_name_starting_with_a_keyword_is_not_the_keyword() {
    assert!(matches!(unit("for_each(xs)"), Unit::Expression(_)));
    assert!(matches!(unit("if_ready()"), Unit::Expression(_)));
    assert!(matches!(unit("let_x"), Unit::Expression(_)));
    assert!(matches!(unit("let x = 1"), Unit::Fragment(_)));
    assert!(matches!(
        parse_input(Input::Input("for x in xs".to_string())),
        InputParse::ReadMultiline(_),
    ));
}

fn reads_on(input: &str) -> bool {
    matches!(parse_input(Input::Input(input.to_string())), InputParse::ReadMultiline(_))
}

/// A line that leaves a bracket open reads on to where it closes, whatever
/// the line starts with.
#[test]
fn an_open_bracket_reads_on() {
    assert!(reads_on("type Flag: enum {"));
    assert!(reads_on("let xs = ["));
    assert!(reads_on("[1, 2,"));
    assert!(reads_on("f(1,"));
    assert!(reads_on("let t = {| a: u32 |}["));
    assert!(!reads_on("let xs = [1, 2]"));
    assert!(!reads_on("f(1)"));
}

/// Brackets in strings and comments are not code, and do not hold the input
/// open.
#[test]
fn a_bracket_in_a_string_or_comment_does_not_read_on() {
    assert!(!reads_on(r#"let s = "(""#));
    assert!(!reads_on(r#"let s = "\"[""#));
    assert!(!reads_on("let x = 1 // (["));
    assert!(!reads_on("let x = /* /* ( */ [ */ 1"));
    assert!(reads_on(r#"let s = [")""#));
}

/// Submitted as multiline input, an open bracket is the parser's to report.
#[test]
fn an_open_bracket_in_multiline_input_is_a_unit() {
    assert!(matches!(
        parse_input(Input::Multiline("let xs = [".to_string())),
        InputParse::Command(Command::Unit(Unit::Fragment(_))),
    ));
}
