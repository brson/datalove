//! Generated datalit expressions, parsed and typechecked by datalit and by
//! datafun, which must agree.
//!
//! A datalit expression is a datafun expression too, and the two languages
//! check it with two parsers and two type checkers, which have drifted apart
//! before. A seed makes an expression with datalit's generator -- every kind
//! it writes, recursive types among them -- and checks it three ways:
//!
//! - **inferred**: `expr` in datalit against `let _x = expr` in datafun. The
//!   two parse to the same tree, and the expression comes to the same type
//!   with the same diagnostics.
//! - **checked**: a type and a value of it without a hint at its head, as
//!   `: T / expr` in datalit against `let _x: T = expr` in datafun, which
//!   reach the expected type by different routes.
//! - **mutated**: the expression broken by one of datalit's mutations -- a
//!   bracket or a comma taken out, a number out of range, a field doubled --
//!   which both must refuse alike: both fail to parse, or both come to the
//!   same type with the same diagnostic codes.
//!
//! A difference is shrunk, an element of a list, a set, a map or a table
//! dropped at a time while it differs the same way, and written out under the
//! target directory with what each side said. A mutated case is written as
//! it is: shrinking what was mutated would change what the mutation does.
//!
//! Environment:
//!
//! - `LITERAL_EQUIV_SEED`: the first seed, by default [`DEFAULT_SEED`], or
//!   `random`.
//! - `LITERAL_EQUIV_COUNT`: how many seeds, by default [`DEFAULT_COUNT`].
//!   `just test-literal-equiv` runs more.
//! - `LITERAL_EQUIV_TEXT`: a datalit expression to compare as it is written,
//!   rather than any seed's.

use rmx::prelude::*;
use std::path::PathBuf;

use bct::random::{SeededRng, env_seeds};
use datalove_datafun_compiler::funlit_equiv::{
    TypeSerde, datafun_expr_to_datalit_serde, datafun_unit_typecheck_to_serde, datalit_typecheck_to_serde,
};
use datalove_datalit::ast::{Expr, ExprFull, ExprList, ExprMap, ExprSet, ExprTable, TypeHint};
use datalove_datalit::ast_gen::{self, AstGenConfig, TypeWeights};
use datalove_datalit::mutation_gen::Mutation;
use datalove_diagnostic::{ParseDiagnostic, TypeDiagnostic};

type Db = datalove_datafun_compiler::Database;

/// The first seed of a default run.
const DEFAULT_SEED: u64 = 0;

/// How many seeds a default run checks.
const DEFAULT_COUNT: u64 = 500;

/// The generator's configuration: datalit's own, recursive types weighted in.
fn config() -> AstGenConfig {
    AstGenConfig {
        type_weights: TypeWeights { rec_type: 3, ..TypeWeights::default() },
        ..AstGenConfig::default()
    }
}

/// Which way an expression is put to the two languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Inferred,
    Checked,
    Mutated,
}

/// What one language made of an expression.
#[derive(Debug, PartialEq)]
struct Side {
    /// Whether it failed to parse; the two parsers' codes are of different
    /// families, so only that is compared.
    parse_failed: bool,
    /// The expression's type, where it parsed.
    root_type: Option<TypeSerde>,
    /// The codes of its type diagnostics, sorted.
    codes: Vec<String>,
}

/// How the two languages differed over an expression, the first way they did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Differs {
    /// One parsed it and the other did not.
    Parse,
    /// Both parsed it, to different trees.
    Tree,
    /// It came to different types.
    Type,
    /// It came to different diagnostics.
    Codes,
}

/// A difference, and what the two sides said.
struct Difference {
    differs: Differs,
    datalit_text: String,
    datafun_text: String,
    detail: String,
}

// ============================================================================
// The two sides
// ============================================================================

/// The codes of `diagnostics`, sorted, a failed subsumption written alike.
///
/// That is the one place the two report one error under two codes, by
/// design: datalit's failed `Check-Subsume` is T022 for `true`, `false` and
/// a string and T032 for anything else (`datalit-typing-rules.md`), where
/// datafun's is F016 for all of them, a plain mismatch
/// (`datafun-typing-rules.md`, Inherited rules).
fn codes<'db>(db: &'db Db, diagnostics: impl Iterator<Item = bct::diagnostic::Diagnostic<'db>>) -> Vec<String> {
    let mut codes: Vec<String> = diagnostics
        .map(|d| d.code.map_or_else(|| "no code".S(), |c| c.as_str(db).S()))
        .map(|code| match code.as_str() {
            "T022" | "T032" | "F016" => "mismatch".S(),
            _ => code,
        })
        .collect();
    codes.sort();
    codes
}

/// What datalit makes of `text`, and the tree it parsed it to.
fn datalit_side(db: &Db, text: &str) -> (Side, Option<datalove_datalit::ast_serde::ExprFull>) {
    let source = bct::input::Source::new(db, text.S());
    let parse_failed = !datalove_datalit::parser::parse_integration_test::accumulated::<ParseDiagnostic>(db, source).is_empty();
    if parse_failed {
        return (Side { parse_failed, root_type: None, codes: Vec::new() }, None);
    }
    let parsed = datalove_datalit::parser::parse_integration_test(db, source);
    let tree = datalove_datalit::ast_serde::ExprFull::from_ast(db, parsed.C());
    let resolved = datalove_datalit::resolve::resolve_names(db, source, parsed.C());
    let result = datalove_datalit::tycheck::type_check(db, parsed.C(), resolved);
    let root_type = datalit_typecheck_to_serde(db, result).root_type;
    let diagnostics = datalove_datalit::tycheck::type_check::accumulated::<TypeDiagnostic>(db, parsed, resolved);
    let codes = codes(db, diagnostics.iter().map(|d| d.to_diagnostic(db)));
    (Side { parse_failed, root_type, codes }, Some(tree))
}

/// What datafun makes of `text`, a `let` binding the expression, and the
/// tree it parsed the expression to, as datalit's.
fn datafun_side(db: &Db, text: &str) -> (Side, Option<Result<datalove_datalit::ast_serde::ExprFull, String>>) {
    let source = bct::input::Source::new(db, text.S());
    let parse_failed = !datalove_datafun_parser::parse_integration_test::accumulated::<ParseDiagnostic>(db, source).is_empty();
    if parse_failed {
        return (Side { parse_failed, root_type: None, codes: Vec::new() }, None);
    }
    let parsed = datalove_datafun_parser::parse_integration_test(db, source);
    let statements = parsed.statements(db);
    let [datalove_datafun_ast::ast::Statement::Let(stmt)] = statements else {
        panic!("`{text}` parsed to something other than one `let`");
    };
    let expr = parsed.body(db).arena.get(stmt.value);
    let tree = datafun_expr_to_datalit_serde(db, expr).map_err(|e| e.S());
    let result = datalove_datafun_tycheck::type_check_single_script(db, source);
    let root_type = datafun_unit_typecheck_to_serde(db, result, expr).root_type;
    let (script, env) = datalove_datafun_tycheck::single_fragment_script(db, source);
    let diagnostics = datalove_datafun_tycheck::type_check_script_units::accumulated::<TypeDiagnostic>(db, script, env);
    let codes = codes(db, diagnostics.iter().map(|d| d.to_diagnostic(db)));
    (Side { parse_failed, root_type, codes }, Some(tree))
}

/// How datalit's `datalit_text` and datafun's `datafun_text` differ, if they
/// do. Their trees are compared only `inferred`, where the two texts write
/// the same expression the same way.
fn compare(db: &Db, datalit_text: &str, datafun_text: &str, inferred: bool) -> Option<Difference> {
    let (datalit, datalit_tree) = datalit_side(db, datalit_text);
    let (datafun, datafun_tree) = datafun_side(db, datafun_text);
    let differs = |differs: Differs, detail: String| Some(Difference {
        differs,
        datalit_text: datalit_text.S(),
        datafun_text: datafun_text.S(),
        detail,
    });
    // Every datalit expression is a datafun one, so what datalit parses
    // datafun must. Datafun's grammar is wider -- names, operators, calls --
    // so what datalit does not parse datafun may, and must then refuse as it
    // checks: datalit refuses `er e` and `: f32 / 0.>5` as it parses them,
    // where datafun reads a name `e` that is no binding (F001), and a
    // comparison `.>` of `: f32 / 0`, an integer where a float is wanted.
    let refused_wider = datalit.parse_failed && !datafun.parse_failed && !datafun.codes.is_empty();
    if datalit.parse_failed != datafun.parse_failed && !refused_wider {
        return differs(Differs::Parse, fmt!(
            "datalit {} it, datafun {} it",
            if datalit.parse_failed { "refused" } else { "parsed" },
            if datafun.parse_failed { "refused" } else { "parsed" },
        ));
    }
    if datalit.parse_failed || datafun.parse_failed {
        return None;
    }
    if inferred && let (Some(datalit_tree), Some(datafun_tree)) = (&datalit_tree, &datafun_tree) {
        match datafun_tree {
            Ok(datafun_tree) if datafun_tree == datalit_tree => {}
            Ok(datafun_tree) => return differs(Differs::Tree, fmt!("datalit: {datalit_tree:?}\ndatafun: {datafun_tree:?}")),
            Err(e) => return differs(Differs::Tree, fmt!("datafun's tree is not datalit's: {e}")),
        }
    }
    if datalit.codes != datafun.codes {
        return differs(Differs::Codes, fmt!("datalit: {:?}\ndatafun: {:?}", datalit.codes, datafun.codes));
    }
    if datalit.root_type != datafun.root_type {
        return differs(Differs::Type, fmt!("datalit: {:?}\ndatafun: {:?}", datalit.root_type, datafun.root_type));
    }
    None
}

// ============================================================================
// Cases
// ============================================================================

/// A case: an expression, and the type it is checked against when it is.
#[derive(Clone)]
struct Case<'db> {
    expr: ExprFull<'db>,
    checked_as: Option<TypeHint<'db>>,
}

impl<'db> Case<'db> {
    /// The two texts, datalit's and datafun's.
    fn texts(&self, db: &'db Db) -> (String, String) {
        let expr = datalove_datalit::pretty::pretty_print(db, &self.expr);
        match &self.checked_as {
            None => (expr.C(), fmt!("let _x = {expr}")),
            Some(ty) => {
                let ty = ty.syntax(db);
                (fmt!(": {ty} / {expr}"), fmt!("let _x: {ty} = {expr}"))
            }
        }
    }

    fn compare(&self, db: &'db Db) -> Option<Difference> {
        let (datalit, datafun) = self.texts(db);
        compare(db, &datalit, &datafun, self.checked_as.is_none())
    }
}

/// The inferred case seed `seed` makes.
fn inferred<'db>(db: &'db Db, seed: u64) -> Case<'db> {
    Case { expr: ast_gen::gen_expr_full_seeded(db, seed, config()), checked_as: None }
}

/// The checked case seed `seed` makes: a type, and a value of it with no
/// hint at its head, which the type checks it against instead.
fn checked<'db>(db: &'db Db, seed: u64) -> Case<'db> {
    let config = config();
    let mut rng = SeededRng::derive(seed, "checked");
    let ty = ast_gen::gen_type_hint(db, &mut rng, &config, 0);
    let expr = ast_gen::gen_expr_matching_type(db, &mut rng, ty.C(), &config, 0);
    Case { expr: ExprFull::new(None, None, expr), checked_as: Some(ty) }
}

/// The mutated text seed `seed` makes, and what was done to it, if any
/// mutation applies to its expression.
fn mutated(db: &Db, seed: u64) -> Option<(String, String)> {
    let mut rng = SeededRng::derive(seed, "mutated");
    let mutations = Mutation::all();
    let first = rng.random_range(0..mutations.len());
    // The first that applies, from one picked at random.
    (0..mutations.len()).find_map(|k| {
        let mutation = mutations[(first + k) % mutations.len()];
        let result = mutation.apply(db, inferred(db, seed).expr, &mut rng)?;
        Some((result.source, result.description))
    })
}

// ============================================================================
// Shrinking
// ============================================================================

/// `expr` with one element of one list, set, map or table in it dropped,
/// every way that can be.
fn smaller<'db>(expr: &ExprFull<'db>) -> Vec<ExprFull<'db>> {
    let rebuild = |e: Expr<'db>| ExprFull::new(expr.local_index(), expr.type_hint(), e);
    // Each child replaced by each of its smaller forms, through `with`.
    fn within<'db, T: Clone>(
        items: &[T],
        child: impl Fn(&T) -> ExprFull<'db>,
        replace: impl Fn(&T, ExprFull<'db>) -> T,
        with: impl Fn(Vec<T>) -> ExprFull<'db>,
    ) -> Vec<ExprFull<'db>> {
        let mut out = Vec::new();
        for (i, item) in items.iter().enumerate() {
            for small in smaller(&child(item)) {
                let mut items = items.to_vec();
                items[i] = replace(item, small);
                out.push(with(items));
            }
        }
        out
    }
    fn dropped<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
        (0..items.len()).map(|i| {
            let mut items = items.to_vec();
            items.remove(i);
            items
        }).collect()
    }
    let one = |payload: &ExprFull<'db>, with: &dyn Fn(ExprFull<'db>) -> Expr<'db>| -> Vec<ExprFull<'db>> {
        smaller(payload).into_iter().map(|small| rebuild(with(small))).collect()
    };
    match expr.expr() {
        Expr::List(l) => {
            let with = |elements| rebuild(Expr::List(ExprList { elements }));
            let mut out: Vec<_> = dropped(&l.elements).into_iter().map(with).collect();
            out.extend(within(&l.elements, |e| e.C(), |_, s| s, with));
            out
        }
        Expr::Set(l) => {
            let with = |elements| rebuild(Expr::Set(ExprSet { elements }));
            let mut out: Vec<_> = dropped(&l.elements).into_iter().map(with).collect();
            out.extend(within(&l.elements, |e| e.C(), |_, s| s, with));
            out
        }
        Expr::Map(m) => {
            let with = |entries| rebuild(Expr::Map(ExprMap { entries }));
            let mut out: Vec<_> = dropped(&m.entries).into_iter().map(with).collect();
            out.extend(within(&m.entries, |e| e.value.C(), |e, value| datalove_datalit::ast::ExprMapEntry { key: e.key.C(), value }, with));
            out
        }
        Expr::Table(t) => {
            let with = |rows| rebuild(Expr::Table(ExprTable { header: t.header.C(), rows }));
            dropped(&t.rows).into_iter().map(with).collect()
        }
        Expr::Some(s) => one(&s.payload, &|payload| Expr::Some(datalove_datalit::ast::ExprSome { payload })),
        Expr::Ok(s) => one(&s.payload, &|payload| Expr::Ok(datalove_datalit::ast::ExprOk { payload })),
        Expr::Er(s) => one(&s.payload, &|payload| Expr::Er(datalove_datalit::ast::ExprEr { payload })),
        Expr::Data(s) => one(&s.value, &|value| Expr::Data(datalove_datalit::ast::ExprData { value })),
        Expr::Error(s) => one(&s.value, &|value| Expr::Error(datalove_datalit::ast::ExprError { value })),
        Expr::Term(t) => one(&t.payload, &|payload| Expr::Term(datalove_datalit::ast::ExprTerm { name: t.name, payload })),
        Expr::AnonTuple(t) => within(&t.elements, |e| e.C(), |_, s| s,
            |elements| rebuild(Expr::AnonTuple(datalove_datalit::ast::ExprAnonTuple { elements }))),
        Expr::AnonStruct(t) => within(&t.fields, |f| f.value.C(),
            |f, value| datalove_datalit::ast::ExprStructField { name: f.name, value },
            |fields| rebuild(Expr::AnonStruct(datalove_datalit::ast::ExprAnonStruct { fields }))),
        _ => Vec::new(),
    }
}

/// The smallest case found that differs as `case` does, the same way.
fn shrink<'db>(db: &'db Db, case: Case<'db>, differs: Differs) -> Case<'db> {
    let mut case = case;
    'smaller: loop {
        for expr in smaller(&case.expr) {
            let candidate = Case { expr, ..case.C() };
            if candidate.compare(db).is_some_and(|d| d.differs == differs) {
                case = candidate;
                continue 'smaller;
            }
        }
        return case;
    }
}

// ============================================================================
// Running
// ============================================================================

/// Write a difference out where it can be read, and say where.
fn write_out(label: &str, difference: &Difference) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("literal-equiv");
    std::fs::create_dir_all(&dir).X();
    let path = dir.join(fmt!("{}.txt", label.replace(' ', "-")));
    std::fs::write(&path, fmt!(
        "{label}: {:?}\n\ndatalit:\n{}\n\ndatafun:\n{}\n\n{}\n",
        difference.differs, difference.datalit_text, difference.datafun_text, difference.detail,
    )).X();
    path
}

fn main() {
    let db = Db::default();
    if let Ok(text) = std::env::var("LITERAL_EQUIV_TEXT") {
        match compare(&db, &text, &fmt!("let _x = {text}"), true) {
            None => println!("\x1b[32msame\x1b[0m {text}"),
            Some(difference) => {
                println!("\x1b[31mFAIL\x1b[0m {text}: {:?}\n{}", difference.differs, difference.detail);
                std::process::exit(1);
            }
        }
        return;
    }
    let seeds = env_seeds("LITERAL_EQUIV", DEFAULT_SEED, DEFAULT_COUNT);
    let mut failed = Vec::new();
    let mut counts = [0usize; 3];
    for seed in seeds.iter() {
        for mode in [Mode::Inferred, Mode::Checked, Mode::Mutated] {
            let label = fmt!("seed {seed} {}", fmt!("{mode:?}").to_lowercase());
            let difference = match mode {
                Mode::Inferred | Mode::Checked => {
                    let case = if mode == Mode::Inferred { inferred(&db, seed) } else { checked(&db, seed) };
                    counts[mode as usize] += 1;
                    case.compare(&db).map(|difference| {
                        let small = shrink(&db, case, difference.differs);
                        small.compare(&db).X()
                    })
                }
                Mode::Mutated => {
                    let Some((text, description)) = mutated(&db, seed) else { continue };
                    counts[2] += 1;
                    compare(&db, &text, &fmt!("let _x = {text}"), false).map(|mut difference| {
                        difference.detail = fmt!("{description}\n{}", difference.detail);
                        difference
                    })
                }
            };
            if let Some(difference) = difference {
                let path = write_out(&label, &difference);
                println!(
                    "\x1b[31mFAIL\x1b[0m {label}: {:?}\n  datalit: {}\n  datafun: {}\n  {}",
                    difference.differs, difference.datalit_text, difference.datafun_text, path.display(),
                );
                failed.push(label);
            }
        }
    }
    println!(
        "literal equivalence: {} inferred, {} checked and {} mutated cases from {} seeds from {}; {} differ",
        counts[0], counts[1], counts[2], seeds.count, seeds.start, failed.len(),
    );
    if !failed.is_empty() {
        std::process::exit(1);
    }
}
