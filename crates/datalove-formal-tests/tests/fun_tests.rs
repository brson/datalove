//! Datafun's statement and expression typing held to the typing rules,
//! through the Lean model.
//!
//! The model in `formal/Datalove/Fun.lean` is written from
//! `botdocs/datafun-typing-rules.md` for a slice of datafun: scripts of
//! functions and statements, without generics, modules, consts, patterns,
//! projections, `for` or `match`. A script outside it is skipped and counted.
//!
//! Scripts come from worldgen, made to stay inside the slice, and are then
//! changed a line at a time at random, so that most are wrong somewhere: a
//! type or a literal changed, an operator swapped, `?`, `!` or `@` added, a
//! name made unbound, a `ret` dropped, a stray `break` or `set` added. Before
//! them runs a corpus of cases written by hand. Each script is parsed and
//! typechecked, and the codes of every diagnostic the Rust reports, in order,
//! are compared with the model's.
//!
//! Needs the oracle built, so it runs only when asked, as `oracle_tests` does:
//!
//! ```text
//! FORMAL_ORACLE_TEST=1 cargo test -p datalove-formal-tests --test fun_tests
//! ```
//!
//! Seeds come from `FORMAL_FUN_SEED` and `FORMAL_FUN_COUNT`.

use rmx::prelude::*;
use rmx::serde_json::{Value, json};
use bct::random::SeededRng;
use datalove_formal_tests::ask_oracle;
use datalove_worldgen::WorldGenConfig;
use std::collections::BTreeMap;

const MAX_MUTATIONS: usize = 3;

/// Scripts written by hand, each a case worth keeping.
const CORPUS: &[&str] = &[
    "let a: i8 = -5\n",
    "let a: u8 = true\nlet b: u8 = none\nlet c: ?u8 = 1\n",
    "let a = not 1\nlet b = true + 1\nlet c = 1 and 2\n",
    "let top = 1 + \"s\"\nfun f(): u8\n  ret true\nend fun\n",
    "fun f(a: u8, b: u8): u8\n  ret a\nend fun\nlet x = f(ref 1, \"s\")\n",
    "fun f(): u8\n  let x = 1\nend fun\n",
    "fun f(a: u8): u8\n  let x: u16 = a\n  let y = x\n  ret y\nend fun\nlet z = 1 + \"s\"\nlet w = z\n",
    "var x: u32 = : u32 / 1\nloop while x .< 3\n  break\nend loop\n",
    "fun f(a: u32, b: u32): !u32\n  ret ok (a +! b)\nend fun\nfun g(a: u32): ?u32\n  let b = a +! a\n  ret some b\nend fun\n",
    "fun f(o: ?u8): u8\n  if o |v|\n    ret v\n  else |e|\n    ret 0\n  end if\nend fun\n",
    "fun f(r: !u8): u8\n  if r |v|\n    ret v\n  end if\n  ret 0\nend fun\n",
    "break\ncontinue\nfun f()\n  ret 1\nend fun\nfun g(): u8\n  ret\nend fun\n",
    "let x = 1\nset x = 2\nset y = 3\nvar z: u8 = 1\nset z += 1\nset z +!= 1\n",
    "fun f(a: u8): u16\n  ret a@\nend fun\nfun g(a: u16): u8\n  ret a@\nend fun\n",
    "fun f(): ()\n  ret ()\nend fun\nfun g(): u8\n  ret 1\nend fun\ncall f()\ncall g()\n",
    "fun f(): u8\n  ret 1\nend fun\nfun f(): u8\n  ret 2\nend fun\n",
    "var v0: u32 = []\nlet a: u8 = [1]\n",
    // A call stops at its first argument not of its parameter's type, and
    // fails, so what it was to bind is unbound; a wrong marker is reported
    // and checking goes on.
    "fun f(a: u8, b: u8): u8\n  ret a\nend fun\nlet w = f(true, true)\nlet z = w\nlet y = f(mut 1, true)\nlet x = y\n",
    // A binding `if` that binds nothing leaves its bodies unchecked: a
    // condition with no type, one of neither an option nor a result, and a
    // result with no error bound.
    "fun f(r: !u8, n: u8)\n  if r |v|\n    call nope()\n  end if\n  if n |v|\n    call nope()\n  end if\nend fun\n",
    // An option with an error bound: F094 after the first body, and the
    // error unbound in the second.
    "fun f(o: ?u8)\n  if o |v|\n    call nope()\n  else |e|\n    debuglog e\n  end if\nend fun\n",
    // A `for` over a list, a set and a map, with one name and two, and over
    // what cannot be walked, with something wrong in the body each time.
    "let l: [u8] = [1]\nfor x in ref l\n  call nope()\nend for\nfor i, x in ref l\n  let a: u8 = i\nend for\n",
    "let s: #{u8} = #{1}\nfor i, x in ref s\n  call nope()\nend for\nlet m: %{u8 = bool} = %{1 = true}\nfor k in ref m\n  call nope()\nend for\n",
    "let n: u8 = 1\nfor x in ref n\n  call nope()\nend for\nfor x in ref zz\n  call nope()\nend for\n",
    "let t: [|u8, 2|] = [| 1 2, 3 4 |]\nfor r in ref t\n  let a: u8 = r\nend for\nbreak\n",
    // A negated literal with a hint of its own has the hint's type, and is not
    // range-checked as a bare one is.
    "let a: i8 = -(: string / 8)\nlet b: i8 = -(: i64 / 8)\nlet c: u8 = -(: int / 8)\nlet d: i8 = -128\nlet e: u8 = -1\n",
    // A `set` of an immutable binding still checks its value.
    "let x: u8 = 1\nset x = true\n",
    "if zz |v|\n  call nope()\nelse\n  call nope()\nend if\nif zz\n  call nope()\nelse\n  call nope()\nend if\n",
];

fn config() -> WorldGenConfig {
    let mut config = WorldGenConfig::default();
    config.module_count = (1, 1);
    config.functions_per_module = (1, 4);
    config.generics_per_module = (0, 0);
    config.generic_call_probability = 0;
    config.type_aliases_per_module = (0, 0);
    config.type_alias_usage_probability = 0;
    config.enums_per_module = (0, 0);
    config.consts_per_module = (0, 0);
    config.const_probability = 0;
    config.match_probability = 0;
    config.for_probability = 15;
    config.param_mode_probability = 0;
    config.projection_probability = 40;
    config.function_call_probability = 50;
    config.arithmetic_probability = 50;
    config
}

const TYPES: &[&str] = &["u8", "i8", "u32", "i64", "int", "f32", "f64", "bool", "string", "index", "?u8", "!u8", "[u8]", "()"];
const VALUES: &[&str] = &["0", "1", "-1", "300", "true", "\"s\"", "1.5", "none", "some 1", "ok 1", "()", "[]"];
const OPS: &[&str] = &["+", "-", "*", "/", "+!", "-!", "*?", "/?", "==", ".<", "and"];

/// One random change to one line of a script.
fn mutate(rng: &mut SeededRng, text: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(|l| l.S()).collect();
    if lines.is_empty() {
        return text.S();
    }
    let i = rng.random_range(0..lines.len());
    let line = lines[i].C();
    let indent: String = line.chars().take_while(|c| *c == ' ').collect();
    let pick = |rng: &mut SeededRng, from: &[&str]| from[rng.random_range(0..from.len())].S();
    let name = |rng: &mut SeededRng| pick(rng, &["p0", "p1", "p2", "v0", "v1", "v2", "1", "0x10", "1.5", "true"]);
    let changed = match rng.random_range(0..16) {
        // A type named in the line changed.
        0 => {
            let ty = pick(rng, TYPES);
            match TYPES.iter().find(|t| line.contains(&fmt!(": {t}"))) {
                Some(t) => line.replacen(&fmt!(": {t}"), &fmt!(": {ty}"), 1),
                None => line,
            }
        }
        // What the line binds, sets or returns, replaced by a value.
        1 => match line.find(" = ").or_else(|| line.find("ret ").map(|p| p + 1)) {
            Some(p) => fmt!("{} {}", &line[..p + 2].trim_end(), pick(rng, VALUES)),
            None => line,
        },
        // An operator swapped.
        2 => {
            let to = pick(rng, OPS);
            match [" + ", " - ", " * ", " / ", " +! ", " -! ", " *! ", " /! ", " +? ", " -? ", " *? ", " /? ", " == ", " .< "]
                .iter().find(|op| line.contains(*op)) {
                Some(op) => line.replacen(op, &fmt!(" {to} "), 1),
                None => line,
            }
        }
        // `?`, `!` or `@` added to the end of a value.
        3 => if line.contains(" = ") || line.trim_start().starts_with("ret ") {
            fmt!("{line}{}", pick(rng, &["?", "!", "@"]))
        } else {
            line
        },
        // A name made one nothing binds.
        4 => match line.find(" v").or_else(|| line.find(" p")) {
            Some(p) => fmt!("{} zz{}", &line[..p], &line[p + 1..]),
            None => line,
        },
        // A `ret` dropped.
        5 => if line.trim_start().starts_with("ret") { fmt!("{indent}debuglog 0") } else { line },
        // A stray `break` or `continue`.
        6 => fmt!("{line}\n{indent}{}", pick(rng, &["break", "continue"])),
        // A `set` of something.
        7 => fmt!("{line}\n{indent}set {} = {}", pick(rng, &["v0", "v1", "p0", "x"]), pick(rng, VALUES)),
        // A call statement of something.
        8 => fmt!("{line}\n{indent}call {}()", pick(rng, &["fn0", "fn1", "fn2", "nope"])),
        // A binding computed from what is in scope: an operator over two
        // names, or one adapted, unwrapped or negated, with or without a type.
        9 => {
            let (a, b, op) = (name(rng), name(rng), pick(rng, OPS));
            fmt!("{line}\n{indent}let w = {a} {op} {b}")
        }
        10 => {
            let (a, ty) = (name(rng), pick(rng, TYPES));
            let form = pick(rng, &["@", "?", "!", ""]);
            let neg = if rng.random_bool(0.3) { "-" } else { "" };
            fmt!("{line}\n{indent}let w: {ty} = {neg}{a}{form}")
        }
        // A binding `if` on something in scope, with an `else` that binds or
        // not, or none.
        11 => {
            let a = name(rng);
            let tail = pick(rng, &["", "else\n  debuglog 0\n", "else |e|\n  debuglog e\n"]);
            let tail = tail.replace('\n', &fmt!("\n{indent}")).replace(&fmt!("{indent}  "), &fmt!("{indent}  "));
            fmt!("{line}\n{indent}if {a} |w|\n{indent}  debuglog w\n{indent}{tail}end if")
        }
        // A `ret` of something in scope, or of nothing.
        12 => fmt!("{line}\n{indent}ret {}", pick(rng, &["", "p0", "p1", "v0", "()", "ok ()", "none"])),
        // A call with arguments from scope, some with a mode marker.
        13 => {
            let (a, b) = (name(rng), name(rng));
            let marker = pick(rng, &["", "", "ref ", "mut "]);
            fmt!("{line}\n{indent}let w = {}({marker}{a}, {b})", pick(rng, &["fn0", "fn1", "fn2"]))
        }
        // A `for` over something in scope, with one name or two, and a body
        // that uses them.
        14 => {
            let names = pick(rng, &["w", "i, w", "k, w"]);
            fmt!("{line}\n{indent}for {names} in ref {}\n{indent}  debuglog w\n{indent}  let z: u8 = w\n{indent}end for", name(rng))
        }
        // A literal at a range's edge, negated.
        _ => fmt!("{line}\n{indent}let w: {} = -{}", pick(rng, &["i8", "u8", "i16", "f32", "int"]), pick(rng, &["5", "128", "129", "300", "1.5"])),
    };
    lines[i] = changed;
    lines.join("\n") + "\n"
}

/// The codes of a script's type diagnostics, in order, and its serde AST.
fn rust_side(text: &str) -> (Vec<String>, Value) {
    let db = &datalove_datafun_compiler::Database::default();
    let source = bct::input::Source::new(db, text.S());
    let parsed = datalove_datafun_parser::parse_for_diagnostics(db, source);
    let script = datalove_datafun_ast::ast_serde::Script::from_ast(db, parsed.C());
    let (unit, env) = datalove_datafun_tycheck::single_fragment_script(db, source);
    let _ = datalove_datafun_tycheck::type_check_script_units(db, unit, env);
    let diagnostics = datalove_datafun_tycheck::type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(db, unit, env);
    let codes = diagnostics.iter()
        .map(|d| d.to_diagnostic(db).code.map(|c| c.as_str(db).S()).unwrap_or_else(|| "none".S()))
        .collect();
    (codes, rmx::serde_json::to_value(&script).X())
}

#[test]
fn statement_typing_agrees() {
    if std::env::var("FORMAL_ORACLE_TEST").is_err() {
        println!("Skipping the statement typing tests (set FORMAL_ORACLE_TEST=1 to run)");
        return;
    }
    let seeds = bct::random::env_seeds("FORMAL_FUN", 0, 300);
    let config = config();
    let mut cases: Vec<(String, String)> = CORPUS.iter().map(|t| ("corpus".S(), t.S())).collect();
    for seed in seeds.iter() {
        let session = datalove_worldgen::gen_session_seeded(seed, config.C());
        let mut text = session.modules.first().map(|m| m.1.C()).unwrap_or_default();
        let mut rng = SeededRng::derive(seed, "mutate");
        for _ in 0..rng.random_range(0..=MAX_MUTATIONS) {
            text = mutate(&mut rng, &text);
        }
        cases.push((fmt!("seed {seed}"), text));
    }

    let mut queries = Vec::new();
    let mut rust = Vec::new();
    for (_, text) in &cases {
        let (codes, script) = rust_side(text);
        queries.push(json!({ "fun": script }));
        rust.push(codes);
    }
    let answers = ask_oracle(&queries);

    let mut skipped = BTreeMap::<String, usize>::new();
    let mut agreed = 0;
    let mut outcomes = BTreeMap::<String, usize>::new();
    let mut mismatches = Vec::new();
    for (((what, text), rust), oracle) in cases.iter().zip(&rust).zip(&answers) {
        if let Some(why) = oracle.get("unsupported") {
            // What the Rust says of a script the model does not take, to see
            // what widening the slice will have to agree with.
            if what == "corpus" && std::env::var("FORMAL_FUN_SHOW_SKIPPED").is_ok() {
                println!("skipped ({why}):\n{text}    rust: {rust:?}");
            }
            *skipped.entry(why.as_str().X().S()).or_default() += 1;
            continue;
        }
        let model: Vec<String> = oracle["codes"].as_array().X().iter().map(|c| c.as_str().X().S()).collect();
        for c in rust {
            *outcomes.entry(c.C()).or_default() += 1;
        }
        if rust.is_empty() {
            *outcomes.entry("clean".S()).or_default() += 1;
        }
        if *rust == model {
            agreed += 1;
        } else {
            mismatches.push(fmt!("{what}:\n{text}    rust:  {rust:?}\n    model: {model:?}"));
        }
    }
    println!(
        "{} scripts over seeds {}..{}: {agreed} agree, {} differ; skipped {skipped:?}; codes {outcomes:?}",
        cases.len(), seeds.start, seeds.start.wrapping_add(seeds.count), mismatches.len(),
    );
    for m in mismatches.iter().take(15) {
        println!("{m}");
    }
    assert!(mismatches.is_empty(), "the Rust and the typing rules disagree on {} scripts", mismatches.len());
}

/// Print the serde AST of `FORMAL_FUN_AST_OF`, a file.
#[test]
fn ast_of() {
    let Ok(path) = std::env::var("FORMAL_FUN_AST_OF") else { return };
    let (codes, script) = rust_side(&std::fs::read_to_string(path).X());
    println!("{codes:?}\n{}", rmx::serde_json::to_string_pretty(&script).X());
}

/// Print the script a seed makes, to look at a failure:
/// `FORMAL_FUN_DUMP=<seed> cargo test -p datalove-formal-tests --test fun_tests dump -- --nocapture`.
#[test]
fn dump() {
    let Ok(seed) = std::env::var("FORMAL_FUN_DUMP") else { return };
    let seed: u64 = seed.parse().X();
    let session = datalove_worldgen::gen_session_seeded(seed, config());
    let mut text = session.modules.first().map(|m| m.1.C()).unwrap_or_default();
    let mut rng = SeededRng::derive(seed, "mutate");
    for _ in 0..rng.random_range(0..=MAX_MUTATIONS) {
        text = mutate(&mut rng, &text);
    }
    println!("{text}");
}
