//! Everything both parsers return, over every source in the tree, written to
//! a directory or checked against one.
//!
//! For changing how a parser works without changing what it does: write the
//! snapshots with the parser as it is, change it, and check. Each source is
//! parsed whole and cut off at a few of its lines, since a source cut off
//! mid-block or mid-bracket is where recovery runs and where two parsers that
//! agree on correct programs come apart. Each is given to both languages'
//! parsers, which for the wrong one is recovery all the way through.
//!
//! Does nothing unless `PARSE_SNAPSHOT` is set, to `write:DIR` or `check:DIR`:
//!
//! ```text
//! PARSE_SNAPSHOT=write:/tmp/parse-snapshots cargo test --release -p datalove-datafun-compiler --test parse_snapshot_tests
//! ```

use rmx::prelude::*;
use rmx::serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// How many places, besides its end, a source is cut off at.
const CUTS: usize = 6;

#[test]
fn parse_snapshots() {
    let Ok(setting) = std::env::var("PARSE_SNAPSHOT") else { return };
    let (mode, dir) = setting.split_once(':').expect("PARSE_SNAPSHOT is write:DIR or check:DIR");
    let dir = PathBuf::from(dir);
    let writing = match mode {
        "write" => true,
        "check" => false,
        _ => panic!("PARSE_SNAPSHOT is write:DIR or check:DIR"),
    };
    if writing {
        std::fs::create_dir_all(&dir).X();
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().X();
    let mut inputs = corpus(&root);
    inputs.sort();

    let mut compared = 0;
    let mut differing = vec![];
    for (name, text) in inputs.iter().flat_map(|(name, text)| cuts(name, text)) {
        for (language, output) in [("datafun", datafun(&text)), ("datalit", datalit(&text))] {
            let file = dir.join(fmt!("{}.{language}.json", sanitize(&name)));
            if writing {
                std::fs::write(&file, &output).X();
            } else {
                compared += 1;
                let expected = std::fs::read_to_string(&file)
                    .unwrap_or_else(|_| panic!("no snapshot for {name}: {}", file.display()));
                if expected != output {
                    std::fs::write(file.with_extension("actual"), &output).X();
                    differing.push(file);
                }
            }
        }
    }

    if !writing {
        eprintln!("parse snapshots: {compared} compared, {} differ", differing.len());
        for file in differing.iter().take(40) {
            eprintln!("  {}", file.display());
        }
        assert!(differing.is_empty(), "{} parse snapshots differ; each has a .actual beside it", differing.len());
    }
}

/// Every source in the tree, by its path from the root; a world file's
/// sections each on their own, after the file as a whole.
fn corpus(root: &Path) -> Vec<(String, String)> {
    let mut inputs = vec![];
    for dir in ["crates", "sys", "benches", "botdocs", "demo", "examples"] {
        let dir = root.join(dir);
        if dir.exists() {
            walk(&dir, root, &mut inputs);
        }
    }
    inputs
}

fn walk(dir: &Path, root: &Path, inputs: &mut Vec<(String, String)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).X().map(|e| e.X().path()).collect();
    entries.sort();
    for path in entries {
        let file_name = path.file_name().X().to_string_lossy().S();
        if path.is_dir() {
            if file_name != "target" && !file_name.starts_with('.') {
                walk(&path, root, inputs);
            }
            continue;
        }
        let Some(ext) = path.extension().map(|e| e.to_string_lossy().S()) else { continue };
        if !["dfs", "dfm", "dli", "dlt", "world"].contains(&ext.as_str()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let name = path.strip_prefix(root).X().to_string_lossy().S();
        if ext == "world" {
            for (i, section) in world_sections(&text).into_iter().enumerate() {
                inputs.push((fmt!("{name}#{i}"), section));
            }
        }
        inputs.push((name, text));
    }
}

/// A world file's sections: what lies between its `----------` headers, the
/// header's own name line left out.
fn world_sections(text: &str) -> Vec<String> {
    let mut sections = vec![];
    let mut current = String::new();
    let mut in_header = false;
    for line in text.lines() {
        if line.starts_with("----------") {
            if !in_header && !current.trim().is_empty() {
                sections.push(std::mem::take(&mut current));
            }
            current.clear();
            in_header = !in_header;
            continue;
        }
        if !in_header {
            current.push_str(line);
            current.push('\n');
        }
    }
    if !current.trim().is_empty() {
        sections.push(current);
    }
    sections
}

/// A source whole, and cut off after a few of its lines, evenly spaced.
fn cuts(name: &str, text: &str) -> Vec<(String, String)> {
    let ends: Vec<usize> = text.match_indices('\n').map(|(i, _)| i + 1).collect();
    let mut out = vec![(name.S(), text.S())];
    if ends.len() < 2 {
        return out;
    }
    let step = (ends.len() / (CUTS + 1)).max(1);
    let mut seen = std::collections::BTreeSet::new();
    for k in 1..=CUTS {
        let Some(&end) = ends.get(k * step - 1) else { break };
        if end < text.len() && seen.insert(end) {
            out.push((fmt!("{name}@{end}"), text[..end].S()));
        }
    }
    out
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '@' || c == '#' || c == '-' { c } else { '_' }).collect()
}

fn diagnostics_json<'db>(
    db: &'db dyn bct::Db,
    text: &str,
    diagnostics: impl Iterator<Item = &'db datalove_diagnostic::ParseDiagnostic>,
) -> Value {
    diagnostics.map(|d| {
        let diag = d.to_diagnostic(db);
        let labels: Vec<_> = diag.labels.iter().map(|label| json!({
            "span": [label.span.start, label.span.end],
            "text": text.get(label.span.clone()).unwrap_or("<not a boundary>"),
            "message": label.message.map(|m| m.as_str(db).S()),
        })).collect();
        json!({
            "code": diag.code.map(|c| c.as_str(db).S()),
            "message": diag.message.as_str(db),
            "labels": labels,
        })
    }).collect()
}

fn spans_json(spans: &[bct::diagnostic::SpanEntry]) -> Value {
    spans.iter().map(|s| json!([s.span.start, s.span.end])).collect()
}

fn datafun(text: &str) -> String {
    let db = datalove_datafun_compiler::Database::default();
    let source = bct::input::Source::new(&db, text.S());
    let result = datalove_datafun_parser::parse(&db, source);
    let diagnostics = datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(&db, source);

    let expr_spans: Vec<_> = result.expr_spans.iter().map(|e| json!([
        e.expr_key.fn_name.map(|n| n.as_str(&db).S()),
        e.expr_key.local_index,
        e.span.start,
        e.span.end,
    ])).collect();
    let qualified: Vec<_> = result.parsed.qualified_calls.iter()
        .map(|(alias, name)| json!([alias.as_str(&db), name.as_str(&db)]))
        .collect();

    let expr = datalove_datafun_parser::parse_expr(&db, source);
    let expr_diagnostics = datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(&db, source);

    let output = json!({
        "ast": datalove_datafun_ast::ast_serde::Script::from_ast(&db, result.parsed.clone()),
        "qualified_calls": qualified,
        "expr_spans": expr_spans,
        "break_spans": spans_json(&result.break_spans),
        "continue_spans": spans_json(&result.continue_spans),
        "ret_spans": spans_json(&result.ret_spans),
        "set_spans": spans_json(&result.set_spans),
        "fun_spans": spans_json(&result.fun_spans),
        "type_alias_spans": spans_json(&result.type_alias_spans),
        "import_spans": spans_json(&result.import_spans),
        "require_spans": spans_json(&result.require_spans),
        "alias_spans": spans_json(&result.alias_spans),
        "diagnostics": diagnostics_json(&db, text, diagnostics.into_iter()),
        "expr": datalove_datafun_ast::ast_serde::ExprFun::from_ast(&db, expr.expr),
        "expr_qualified_calls": expr.qualified_calls.len(),
        "expr_diagnostics": diagnostics_json(&db, text, expr_diagnostics.into_iter()),
    });
    rmx::serde_json::to_string_pretty(&output).X()
}

fn datalit(text: &str) -> String {
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, text.S());
    let ast = datalove_datalit::parser::parse_integration_test(&db, source);
    let diagnostics = datalove_datalit::parser::parse_integration_test::accumulated::<datalove_diagnostic::ParseDiagnostic>(&db, source);
    let output = json!({
        "ast": datalove_datalit::ast_serde::ExprFull::from_ast(&db, ast),
        "diagnostics": diagnostics_json(&db, text, diagnostics.into_iter()),
    });
    rmx::serde_json::to_string_pretty(&output).X()
}
