//! What a large collection literal costs to compile, by kind, shape and size,
//! for tuning when one is worth splitting into chunks.
//!
//! Each case is one collection, generated: a list, map, set, table or tensor
//! of scalars, small records or small lists, at sizes doubling from 4KB. In
//! datalit it is a data file, timed through the queries a `require data` goes
//! through -- lexing and bracket matching, `datalit::parser::parse`,
//! `data_file_type` against the declared type, and `data_file_value` -- and in
//! datafun it is a module-level const, timed through lexing and
//! `parse_module_full`. Every stage of every iteration runs on a fresh
//! database, so nothing is a memo hit. See `botdocs/plan-parallel-collections.md`.
//!
//! `cargo run --release -p datalove-bench --example collection_cutoffs -- [options]`:
//! `--lang datalit|datafun`, `--kind list|map|set|table|tensor`,
//! `--min-bytes N`, `--max-bytes N`, `--target-ms N` (how long to sample each
//! case for; default 300).

use rmx::prelude::*;
use std::time::{Duration, Instant};

use bct::input::Source;
use bct::module_graph::{Module, ModuleId};
use datalove_datafun as datafun;
use datalove_datafun_ir::IrType;
use datalove_datafun_tycheck::{DataFileQuery, data_file_type};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lang {
    Datalit,
    Datafun,
}

/// One kind of collection with one shape of element.
struct Shape {
    kind: &'static str,
    shape: &'static str,
    /// The collection's type, as both languages write it.
    ty: &'static str,
    open: &'static str,
    close: &'static str,
    /// What goes before every element but the first.
    separator: &'static str,
    /// Written once after the open, as a table's header is.
    header: &'static str,
    element: fn(usize) -> String,
}

const SHAPES: &[Shape] = &[
    Shape { kind: "list", shape: "scalar", ty: "[u32]",
        open: "[", close: "]", separator: ",\n", header: "",
        element: |i| fmt!("{}", i * 7919 % 1_000_000) },
    Shape { kind: "list", shape: "record", ty: "[{id: u32, name: string, qty: u32}]",
        open: "[", close: "]", separator: ",\n", header: "",
        element: |i| fmt!("{{id = {i}, name = \"item{i}\", qty = {}}}", i % 9) },
    Shape { kind: "list", shape: "nested", ty: "[[u32]]",
        open: "[", close: "]", separator: ",\n", header: "",
        element: |i| fmt!("[{i}, {}, {}]", i + 1, i + 2) },
    Shape { kind: "map", shape: "scalar", ty: "%{u32 = u32}",
        open: "%{", close: "}", separator: ",\n", header: "",
        element: |i| fmt!("{i} = {}", i * 7919 % 1_000_000) },
    Shape { kind: "map", shape: "record", ty: "%{u32 = {name: string, qty: u32}}",
        open: "%{", close: "}", separator: ",\n", header: "",
        element: |i| fmt!("{i} = {{name = \"item{i}\", qty = {}}}", i % 9) },
    Shape { kind: "map", shape: "nested", ty: "%{u32 = [u32]}",
        open: "%{", close: "}", separator: ",\n", header: "",
        element: |i| fmt!("{i} = [{i}, {}, {}]", i + 1, i + 2) },
    Shape { kind: "set", shape: "scalar", ty: "#{u32}",
        open: "#{", close: "}", separator: ",\n", header: "",
        element: |i| fmt!("{i}") },
    Shape { kind: "set", shape: "string", ty: "#{string}",
        open: "#{", close: "}", separator: ",\n", header: "",
        element: |i| fmt!("\"item{i}\"") },
    Shape { kind: "set", shape: "nested", ty: "#{[u32]}",
        open: "#{", close: "}", separator: ",\n", header: "",
        element: |i| fmt!("[{i}, {}, {}]", i + 1, i + 2) },
    Shape { kind: "table", shape: "scalar", ty: "{| id: u32 |}",
        open: "{|", close: "|}", separator: ";\n", header: "id;\n",
        element: |i| fmt!("{i}") },
    Shape { kind: "table", shape: "record", ty: "{| id: u32, name: string, qty: u32 |}",
        open: "{|", close: "|}", separator: ";\n", header: "id, name, qty;\n",
        element: |i| fmt!("{i}, \"item{i}\", {}", i % 9) },
    Shape { kind: "tensor", shape: "scalar", ty: "[|u32, 2|]",
        open: "[|", close: "|]", separator: ",\n", header: "",
        element: |i| (0..8).map(|j| fmt!("{}", (i * 8 + j) % 1000)).collect::<Vec<_>>().join(" ") },
];

/// The collection written out to at least `bytes`, and how many elements it has.
fn generate(shape: &Shape, bytes: usize) -> (String, usize) {
    let mut text = String::with_capacity(bytes + 256);
    text.push_str(shape.open);
    text.push('\n');
    text.push_str(shape.header);
    let mut count = 0;
    while text.len() < bytes {
        if count > 0 {
            text.push_str(shape.separator);
        }
        text.push_str(&(shape.element)(count));
        count += 1;
    }
    text.push('\n');
    text.push_str(shape.close);
    text.push('\n');
    (text, count)
}

/// The medians of one case's stages, in the order the case reports them.
type Stages = Vec<(&'static str, Duration)>;

/// One iteration of a datalit case, each stage on what the one before left.
fn datalit_once(shape: &Shape, text: &str) -> Stages {
    let db = datafun::Database::default();
    // The declared type, as `require data` would hand it over: the type of a
    // two-element collection that names it, an empty table being no table and
    // one row of a tensor no tensor of rank 2.
    let declared = Source::new(&db, fmt!(": {} / {}{}{}{}{}{}",
        shape.ty, shape.open, shape.header, (shape.element)(0), shape.separator, (shape.element)(1), shape.close));
    let expected = match data_file_type(&db, DataFileQuery::new(&db, declared, None)) {
        Ok(ty) => ty.clone(),
        Err(errors) => panic!("`{}` is not a type: {errors:?}", shape.ty),
    };
    let ir_type = IrType::from_datalit(&db, &expected);
    let source = Source::new(&db, text.S());

    let mut stages = Vec::new();
    let t = Instant::now();
    let lexed = bct::lexer::lex(&db, source);
    let _ = bct::bracer::bracer(&db, lexed);
    stages.push(("lex", t.elapsed()));

    let t = Instant::now();
    let _ = datalove_datalit::parser::parse(&db, source);
    stages.push(("parse", t.elapsed()));

    let t = Instant::now();
    if let Err(errors) = data_file_type(&db, DataFileQuery::new(&db, source, Some(expected))) {
        panic!("{} of {} does not check: {errors:?}", shape.kind, shape.shape);
    }
    stages.push(("type", t.elapsed()));

    let t = Instant::now();
    let _ = datalove_datafun_lower::data_file_value(&db, source, &ir_type);
    stages.push(("read", t.elapsed()));

    drop(db);
    stages
}

/// One iteration of a datafun case: the collection as a module's const.
fn datafun_once(shape: &Shape, text: &str) -> Stages {
    let db = datafun::Database::default();
    let source = Source::new(&db, fmt!("const XS: {} = {}", shape.ty, text));
    let module = Module::new(&db, ModuleId::new(&db, S("local/bench/m")), source);

    let mut stages = Vec::new();
    let t = Instant::now();
    let lexed = bct::lexer::lex(&db, source);
    let _ = bct::bracer::bracer(&db, lexed);
    stages.push(("lex", t.elapsed()));

    let t = Instant::now();
    let _ = datalove_datafun_parser::parse_module_full(&db, module);
    stages.push(("parse", t.elapsed()));

    drop(db);
    stages
}

/// Iterate a case until `target` has been spent on it, at least 3 times and
/// at most 51, and take each stage's median.
fn measure(once: impl Fn() -> Stages, target: Duration) -> Stages {
    let start = Instant::now();
    let mut runs: Vec<Stages> = Vec::new();
    while runs.len() < 3 || (runs.len() < 51 && start.elapsed() < target) {
        runs.push(once());
    }
    let names: Vec<&'static str> = runs[0].iter().map(|(name, _)| *name).collect();
    names.iter().enumerate()
        .map(|(i, name)| {
            let mut times: Vec<Duration> = runs.iter().map(|run| run[i].1).collect();
            times.sort();
            (*name, times[times.len() / 2])
        })
        .collect()
}

struct Options {
    langs: Vec<Lang>,
    kind: Option<String>,
    min_bytes: usize,
    max_bytes: usize,
    target: Duration,
}

fn options() -> Options {
    let mut options = Options {
        langs: vec![Lang::Datalit, Lang::Datafun],
        kind: None,
        min_bytes: 4 << 10,
        max_bytes: 4 << 20,
        target: Duration::from_millis(300),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{arg} wants a value")).C();
        match arg.as_str() {
            "--lang" => options.langs = match value().as_str() {
                "datalit" => vec![Lang::Datalit],
                "datafun" => vec![Lang::Datafun],
                other => panic!("no language `{other}`"),
            },
            "--kind" => options.kind = Some(value()),
            "--min-bytes" => options.min_bytes = value().parse().expect("a byte count"),
            "--max-bytes" => options.max_bytes = value().parse().expect("a byte count"),
            "--target-ms" => options.target = Duration::from_millis(value().parse().expect("milliseconds")),
            other => panic!("no option `{other}`"),
        }
    }
    options
}

fn main() {
    let options = options();
    println!("{:<8} {:<7} {:<7} {:>9} {:>8}  {}", "lang", "kind", "shape", "bytes", "elements",
        "stage: median ms (ns/byte)");
    for lang in &options.langs {
        for shape in SHAPES.iter().filter(|s| options.kind.as_deref().is_none_or(|k| k == s.kind)) {
            let mut bytes = options.min_bytes;
            while bytes <= options.max_bytes {
                let (text, elements) = generate(shape, bytes);
                let stages = match lang {
                    Lang::Datalit => measure(|| datalit_once(shape, &text), options.target),
                    Lang::Datafun => measure(|| datafun_once(shape, &text), options.target),
                };
                let report: Vec<String> = stages.iter()
                    .map(|(name, time)| fmt!("{name} {:.3} ({:.0})",
                        time.as_secs_f64() * 1e3, time.as_secs_f64() * 1e9 / text.len() as f64))
                    .collect();
                println!("{:<8} {:<7} {:<7} {:>9} {:>8}  {}",
                    fmt!("{lang:?}").to_lowercase(), shape.kind, shape.shape, text.len(), elements,
                    report.join("  "));
                bytes *= 2;
            }
        }
    }
}
