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
//!
//! With `--cutoffs`, it times instead the parse of each datalit case that can
//! be cut, whole and cut into chunks of 8KB to 512KB, the chunks parsed one
//! after another and on rayon pools of 1, 2 and 4 threads, and says for each
//! shape the smallest size from which cutting is the faster by 5% at every
//! size measured, and with what chunks at that size. Those are what
//! `bct::chunk::DEFAULT_THRESHOLDS` should say.
//!
//! With `--defaults`, it times the same cases whole and as
//! `DEFAULT_THRESHOLDS` cuts them, in turn and on four threads: the check
//! that the defaults are what the cutoffs said.

use rmx::prelude::*;
use std::time::{Duration, Instant};

use bct::chunk::{ChunkPolicy, CollectionKind, Thresholds};
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

/// A database whose collections are parsed whole.
fn whole_db() -> datafun::Database {
    let db = datafun::Database::default();
    ChunkPolicy::new(&db, Vec::new());
    db
}

/// One iteration of a datalit case, each stage on what the one before left.
fn datalit_once(shape: &Shape, text: &str) -> Stages {
    let db = whole_db();
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
    let db = whole_db();
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

/// How a case's collection is parsed.
#[derive(Clone, Copy, Debug)]
enum Config {
    Whole,
    /// Cut into chunks of about this many bytes, parsed one after another by
    /// the parse that reaches them.
    Cut(u32),
    /// The same, the chunks parsed first on a pool of this many threads.
    CutParallel(u32, usize),
    /// Cut as `DEFAULT_THRESHOLDS` says, the chunks parsed first on a pool
    /// of this many threads, or in turn for none.
    Default(Option<usize>),
}

/// The parse of a datalit case in `config`, from the plan on, on a fresh
/// database; lexing is done first, untimed.
fn parse_once(shape: &Shape, text: &str, config: Config, pools: &[(usize, rmx::rayon::ThreadPool)]) -> Duration {
    use rmx::rayon::prelude::*;

    let db = datafun::Database::default();
    let thresholds = match config {
        Config::Whole => Vec::new(),
        Config::Default(_) => bct::chunk::DEFAULT_THRESHOLDS.to_vec(),
        Config::Cut(chunk_bytes) | Config::CutParallel(chunk_bytes, _) => {
            let kind = match shape.kind {
                "list" => CollectionKind::List,
                "map" => CollectionKind::Map,
                "set" => CollectionKind::Set,
                other => panic!("a {other} is not cut"),
            };
            vec![(kind, Thresholds { min_bytes: 0, min_elements: 2, chunk_bytes, target_chunks: u32::MAX })]
        }
    };
    ChunkPolicy::new(&db, thresholds);
    let source = Source::new(&db, text.S());
    let lexed = bct::lexer::lex(&db, source);
    let _ = bct::bracer::bracer(&db, lexed);

    let t = Instant::now();
    let plan = bct::chunk::collection_plan(&db, lexed);
    let threads = match config {
        Config::CutParallel(_, threads) | Config::Default(Some(threads)) => Some(threads),
        _ => None,
    };
    if let Some(threads) = threads.filter(|_| !plan.is_empty()) {
        let pool = &pools.iter().find(|(n, _)| *n == threads).expect("a pool of that size").1;
        let work: Vec<_> = plan.iter().enumerate()
            .flat_map(|(collection, planned)| (0..planned.chunks.len()).map(move |chunk| (collection as u32, chunk as u32)))
            .map(|(collection, chunk)| (db.clone(), collection, chunk))
            .collect();
        pool.install(|| work.into_par_iter().for_each(|(db, collection, chunk)| {
            let _ = datalove_datalit::parser::parse_chunk(&db, source, collection, chunk);
        }));
    }
    let _ = datalove_datalit::parser::parse(&db, source);
    let elapsed = t.elapsed();
    drop(db);
    elapsed
}

/// The median of `once` over `target`, at least 3 times and at most 51.
fn median(once: impl Fn() -> Duration, target: Duration) -> Duration {
    let start = Instant::now();
    let mut times = Vec::new();
    while times.len() < 3 || (times.len() < 51 && start.elapsed() < target) {
        times.push(once());
    }
    times.sort();
    times[times.len() / 2]
}

/// The `--defaults` report.
fn defaults(options: &Options) {
    let pools = vec![(4, rmx::rayon::ThreadPoolBuilder::new().num_threads(4).build().expect("a pool"))];
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    println!("{:<7} {:<7} {:>9} {:>9}  {:>14}  {:>14}", "kind", "shape", "bytes", "whole ms",
        "in turn", "on 4 threads");
    let cuttable = SHAPES.iter()
        .filter(|s| matches!(s.kind, "list" | "map" | "set"))
        .filter(|s| options.kind.as_deref().is_none_or(|k| k == s.kind));
    for shape in cuttable {
        let mut bytes = options.min_bytes;
        while bytes <= options.max_bytes {
            let (text, _) = generate(shape, bytes);
            let whole = median(|| parse_once(shape, &text, Config::Whole, &pools), options.target);
            let in_turn = median(|| parse_once(shape, &text, Config::Default(None), &pools), options.target);
            let parallel = median(|| parse_once(shape, &text, Config::Default(Some(4)), &pools), options.target);
            let ratio = |d: Duration| d.as_secs_f64() / whole.as_secs_f64();
            println!("{:<7} {:<7} {:>9} {:>9.2}  {:>8.2} {:>5.2}x  {:>8.2} {:>5.2}x", shape.kind, shape.shape,
                text.len(), ms(whole), ms(in_turn), ratio(in_turn), ms(parallel), ratio(parallel));
            bytes *= 2;
        }
    }
}

/// The `--cutoffs` report.
fn cutoffs(options: &Options) {
    const CHUNK_BYTES: [u32; 4] = [8 << 10, 32 << 10, 128 << 10, 512 << 10];
    const THREADS: [usize; 3] = [1, 2, 4];
    let pools: Vec<(usize, rmx::rayon::ThreadPool)> = THREADS.iter()
        .map(|&n| (n, rmx::rayon::ThreadPoolBuilder::new().num_threads(n).build().expect("a pool")))
        .collect();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;

    println!("{:<7} {:<7} {:>9} {:>9}  {:>16}  {}", "kind", "shape", "bytes", "whole ms",
        "cut, in turn", "cut, on 1/2/4 threads: best ms (chunk KB)");
    let mut summary = Vec::new();
    let cuttable = SHAPES.iter()
        .filter(|s| matches!(s.kind, "list" | "map" | "set"))
        .filter(|s| options.kind.as_deref().is_none_or(|k| k == s.kind));
    for shape in cuttable {
        // Per thread count, per size: whether cutting won, and with what chunks.
        let mut wins: Vec<Vec<(usize, Option<u32>)>> = vec![Vec::new(); THREADS.len()];
        let mut bytes = options.min_bytes;
        while bytes <= options.max_bytes {
            let (text, _) = generate(shape, bytes);
            let whole = median(|| parse_once(shape, &text, Config::Whole, &pools), options.target);
            let best = |config: &dyn Fn(u32) -> Config| CHUNK_BYTES.iter()
                .filter(|&&c| (c as usize) < text.len())
                .map(|&c| (median(|| parse_once(shape, &text, config(c), &pools), options.target), c))
                .min();
            let in_turn = best(&Config::Cut);
            let mut cells = Vec::new();
            for (i, &threads) in THREADS.iter().enumerate() {
                let found = best(&|c| Config::CutParallel(c, threads));
                let won = found.filter(|(time, _)| time.as_secs_f64() < whole.as_secs_f64() * 0.95);
                wins[i].push((text.len(), won.map(|(_, chunk)| chunk)));
                match found {
                    Some((time, chunk)) => cells.push(fmt!("{:.2} ({})", ms(time), chunk >> 10)),
                    None => cells.push(S("-")),
                }
            }
            let in_turn = in_turn.map_or(S("-"), |(time, chunk)| fmt!("{:.2} ({})", ms(time), chunk >> 10));
            println!("{:<7} {:<7} {:>9} {:>9.2}  {:>16}  {}", shape.kind, shape.shape, text.len(), ms(whole),
                in_turn, cells.join("  "));
            bytes *= 2;
        }
        // The smallest size from which every larger one won too.
        let from: Vec<Option<(usize, u32)>> = wins.iter()
            .map(|sizes| {
                let losing = sizes.iter().rposition(|(_, won)| won.is_none());
                let start = losing.map_or(0, |at| at + 1);
                sizes.get(start).map(|(bytes, won)| (*bytes, won.expect("a win")))
            })
            .collect();
        summary.push((shape.kind, shape.shape, from));
    }

    println!("\nsmallest size from which cutting wins by 5%, and with what chunks:");
    for (kind, shape, wins) in summary {
        let cells: Vec<String> = THREADS.iter().zip(&wins)
            .map(|(threads, win)| match win {
                Some((bytes, chunk)) => fmt!("{threads} threads: {}KB ({}KB chunks)", bytes >> 10, chunk >> 10),
                None => fmt!("{threads} threads: never"),
            })
            .collect();
        println!("  {kind:<7} {shape:<7} {}", cells.join(", "));
    }
}

struct Options {
    cutoffs: bool,
    defaults: bool,
    langs: Vec<Lang>,
    kind: Option<String>,
    min_bytes: usize,
    max_bytes: usize,
    target: Duration,
}

fn options() -> Options {
    let mut options = Options {
        cutoffs: false,
        defaults: false,
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
            "--cutoffs" => options.cutoffs = true,
            "--defaults" => options.defaults = true,
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
    if options.cutoffs {
        return cutoffs(&options);
    }
    if options.defaults {
        return defaults(&options);
    }
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
