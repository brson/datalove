//! Generated datalit expressions, parsed and typechecked by datalit and by
//! datafun, which must agree.
//!
//! A datalit expression is a datafun expression too, and the two languages
//! check it with two parsers and two type checkers, which have drifted apart
//! before. A seed makes an expression with datalit's generator -- every kind
//! it writes, recursive types among them -- and puts it these ways:
//!
//! - **inferred**: `expr` in datalit against `let _x = expr` in datafun. The
//!   two parse to the same tree, and the expression comes to the same type
//!   with the same diagnostics.
//! - **valued**: an inferred expression both accept, read as a data file by
//!   datalit and compiled as a literal by datafun, in one program that shows
//!   both: the two values are shown alike.
//! - **checked**: a type and a value of it without a hint at its head, as
//!   `: T / expr` in datalit against `let _x: T = expr` in datafun, which
//!   reach the expected type by different routes.
//! - **placed**: the checked expression put where datafun checks it by
//!   another route still -- a function's argument, what a function returns,
//!   a `set`, a `var` -- against datalit's `: T / expr`: the same
//!   diagnostics.
//! - **mutated**: the expression broken by one of datalit's mutations -- a
//!   bracket or a comma taken out, a number out of range, a field doubled, a
//!   type's name made no type's -- which both must refuse alike: both fail
//!   to parse, or both come to the same type with the same diagnostic codes.
//!
//! Seeds are checked in parallel, each with a database of its own.
//!
//! A difference is shrunk, an element of a list, a set, a map or a table
//! dropped at a time while it differs the same way, and written out under the
//! target directory as a case, with what each side said in comments. A
//! mutated case is written as it is: shrinking what was mutated would change
//! what the mutation does.
//!
//! The cases in `tests/literal_equiv/` run before any seed: differences
//! found and fixed, copied in from where they were written out, and cases
//! written by hand for the rules the two languages state differently.
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
use rmx::rayon::prelude::*;
use std::collections::BTreeMap;
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

/// How many cases were put each way.
#[derive(Default)]
struct Counts {
    inferred: usize,
    valued: usize,
    checked: usize,
    placed: usize,
    mutated: usize,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.inferred += other.inferred;
        self.valued += other.valued;
        self.checked += other.checked;
        self.placed += other.placed;
        self.mutated += other.mutated;
    }
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
    /// The expression's type as datalit writes it, where datalit typed it.
    spelled: Option<String>,
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
    /// Read as data and compiled as a literal, it came to different values.
    Value,
}

/// A difference, the case that showed it, and what the two sides said.
struct Difference {
    differs: Differs,
    written: Written,
    datalit_text: String,
    datafun_text: String,
    detail: String,
}

// ============================================================================
// The two sides
// ============================================================================

/// The codes of `diagnostics`, sorted, with the two errors the languages
/// report under different codes by design written alike:
///
/// - A failed `Check-Subsume`: T022 in datalit for `true`, `false` and a
///   string, and T032 for anything else (`datalit-typing-rules.md`), where
///   datafun's is F016 for all of them, a plain mismatch
///   (`datafun-typing-rules.md`, Inherited rules).
/// - A type named that is no type: T059 in datalit, which has no names for
///   types at all, and F064 in datafun, which has aliases and no such one.
fn codes<'db>(db: &'db Db, diagnostics: impl Iterator<Item = bct::diagnostic::Diagnostic<'db>>) -> Vec<String> {
    let mut codes: Vec<String> = diagnostics
        .map(|d| d.code.map_or_else(|| "no code".S(), |c| c.as_str(db).S()))
        .map(|code| match code.as_str() {
            "T022" | "T032" | "F016" => "mismatch".S(),
            "T059" | "F064" => "unknown type".S(),
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
        return (Side { parse_failed, root_type: None, codes: Vec::new(), spelled: None }, None);
    }
    let parsed = datalove_datalit::parser::parse_integration_test(db, source);
    let tree = datalove_datalit::ast_serde::ExprFull::from_ast(db, parsed.C());
    let resolved = datalove_datalit::resolve::resolve_names(db, source, parsed.C());
    let result = datalove_datalit::tycheck::type_check(db, parsed.C(), resolved);
    let spelled = result.root_type(db).as_ref().map(|ty| datalove_datalit::tycheck::type_to_string(db, ty));
    let root_type = datalit_typecheck_to_serde(db, result).root_type;
    let diagnostics = datalove_datalit::tycheck::type_check::accumulated::<TypeDiagnostic>(db, parsed, resolved);
    let codes = codes(db, diagnostics.iter().map(|d| d.to_diagnostic(db)));
    (Side { parse_failed, root_type, codes, spelled }, Some(tree))
}

/// What datafun makes of `text`, a `let` binding the expression, and the
/// tree it parsed the expression to, as datalit's.
fn datafun_side(db: &Db, text: &str) -> (Side, Option<Result<datalove_datalit::ast_serde::ExprFull, String>>) {
    let source = bct::input::Source::new(db, text.S());
    let parse_failed = !datalove_datafun_parser::parse_integration_test::accumulated::<ParseDiagnostic>(db, source).is_empty();
    if parse_failed {
        return (Side { parse_failed, root_type: None, codes: Vec::new(), spelled: None }, None);
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
    (Side { parse_failed, root_type, codes, spelled: None }, Some(tree))
}

/// Whether datafun's `text`, a script, fails to parse, and the codes of its
/// type diagnostics.
fn datafun_codes(db: &Db, text: &str) -> (bool, Vec<String>) {
    let source = bct::input::Source::new(db, text.S());
    if !datalove_datafun_parser::parse_integration_test::accumulated::<ParseDiagnostic>(db, source).is_empty() {
        return (true, Vec::new());
    }
    let (script, env) = datalove_datafun_tycheck::single_fragment_script(db, source);
    let diagnostics = datalove_datafun_tycheck::type_check_script_units::accumulated::<TypeDiagnostic>(db, script, env);
    (false, codes(db, diagnostics.iter().map(|d| d.to_diagnostic(db))))
}

/// How datalit and datafun differ over `written`'s two texts, if they do.
/// Their trees are compared only where it is inferred, where the two texts
/// write the same expression the same way.
fn compare(db: &Db, written: &Written) -> Option<Difference> {
    let (datalit_text, datafun_text) = written.texts();
    let (datalit, datalit_tree) = datalit_side(db, &datalit_text);
    let (datafun, datafun_tree) = datafun_side(db, &datafun_text);
    let differs = |differs: Differs, detail: String| Some(Difference {
        differs,
        written: written.C(),
        datalit_text: datalit_text.C(),
        datafun_text: datafun_text.C(),
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
    if written.way == Way::Inferred && let (Some(datalit_tree), Some(datafun_tree)) = (&datalit_tree, &datafun_tree) {
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
// Written cases
// ============================================================================

/// Which way an expression is put to the two languages; see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    Inferred,
    Valued,
    Checked,
    Placed(Place),
    Mutated,
}

impl Way {
    const NAMES: [(&str, Way); 8] = [
        ("inferred", Way::Inferred),
        ("valued", Way::Valued),
        ("checked", Way::Checked),
        ("argument", Way::Placed(Place::Argument)),
        ("return", Way::Placed(Place::Return)),
        ("set", Way::Placed(Place::Set)),
        ("var", Way::Placed(Place::Var)),
        ("mutated", Way::Mutated),
    ];

    fn name(self) -> &'static str {
        Way::NAMES.iter().find(|(_, way)| *way == self).X().0
    }
}

/// A case as text: a way, the type an expression is checked against where
/// the way checks it against one, and the expression. What the kept cases
/// are written as, and what a difference is written out as.
#[derive(Clone, Debug)]
struct Written {
    way: Way,
    ty: Option<String>,
    expr: String,
}

impl Written {
    /// The two texts, datalit's and datafun's, for the ways that compare two
    /// texts' typing.
    fn texts(&self) -> (String, String) {
        let expr = &self.expr;
        match (&self.ty, self.way) {
            (None, _) => (expr.C(), fmt!("let _x = {expr}")),
            (Some(ty), Way::Placed(place)) => (fmt!(": {ty} / {expr}"), place.script(ty, expr)),
            (Some(ty), _) => (fmt!(": {ty} / {expr}"), fmt!("let _x: {ty} = {expr}")),
        }
    }

    /// How the two languages differ over it, put its way.
    fn check(&self, db: &Db) -> Option<Difference> {
        match self.way {
            Way::Inferred | Way::Checked | Way::Mutated => compare(db, self),
            Way::Placed(_) => self.placed(db),
            Way::Valued => self.valued(db),
        }
    }

    /// How datafun, checking the expression where it is put, differs from
    /// datalit checking it against its type.
    fn placed(&self, db: &Db) -> Option<Difference> {
        let (datalit_text, datafun_text) = self.texts();
        let (datalit, _) = datalit_side(db, &datalit_text);
        let (parse_failed, codes) = datafun_codes(db, &datafun_text);
        let differs = match () {
            _ if datalit.parse_failed != parse_failed => Differs::Parse,
            _ if datalit.codes != codes => Differs::Codes,
            _ => return None,
        };
        Some(Difference {
            differs,
            written: self.C(),
            datalit_text,
            datafun_text,
            detail: fmt!("datalit: {:?}\ndatafun: {codes:?}", datalit.codes),
        })
    }

    /// How the expression differs read as a data file, by datalit, from the
    /// same text compiled as a datafun literal, run and shown, where both
    /// languages accept it.
    ///
    /// The two are bound at the type datalit finds for it, and shown by one
    /// printer, so what can differ is what each built.
    fn valued(&self, db: &Db) -> Option<Difference> {
        let text = &self.expr;
        let (datalit, _) = datalit_side(db, text);
        let ty = match datalit.spelled {
            Some(ty) if datalit.codes.is_empty() && !datalit.parse_failed => ty,
            _ => return None,
        };
        let world = fmt!(
            "----------\ndata local/gen/d\n----------\n{text}\n\
             ----------\nscriptunit-fragment\n----------\n\
             require data local/gen/d: {ty}\nlet v: {ty} = {text}\ndebuglog d\ndebuglog v\n",
        );
        let parsed = datalove_datafun::package_load_worldfile::parse_worldfile_sections(world.as_bytes()).X();
        let analysis = datalove_datafun::worldfile_analysis::analyze_worldfile(&datalove_datafun::Database::default(), parsed);
        let difference = |detail: String| Some(Difference {
            differs: Differs::Value,
            written: self.C(),
            datalit_text: fmt!("data local/gen/d: {ty} = {text}"),
            datafun_text: fmt!("let v: {ty} = {text}"),
            detail,
        });
        let analysis = match analysis {
            Ok(analysis) => analysis,
            Err(e) => return difference(fmt!("the program did not analyze: {e}")),
        };
        let fragment = analysis.sections.iter().find(|s| s.section_type == "scriptunit-fragment").X();
        if fragment.output != "(fragment executed)" {
            return difference(fmt!(
                "the program did not run: {}\n{:?} {:?} {:?}",
                fragment.output, fragment.typecheck, fragment.ownership, fragment.lowering,
            ));
        }
        let shown = fragment.debug_output.as_deref().unwrap_or("");
        match shown.lines().collect::<Vec<_>>().as_slice() {
            [data, literal] if data == literal => None,
            _ => difference(fmt!("shown, the data file and then the literal:\n{shown}")),
        }
    }

    /// The case `text` writes: `mode:`, `type:` where the way checks the
    /// expression against one, and `expr:`, a line each, among `//`
    /// comments.
    fn read(text: &str) -> Result<Written, String> {
        let mut fields = BTreeMap::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")) {
            let (key, value) = line.split_once(':').ok_or_else(|| fmt!("`{line}` is no `key: value`"))?;
            fields.insert(key.trim(), value.trim().S());
        }
        let mode = fields.remove("mode").ok_or("no `mode:`")?;
        let way = Way::NAMES.iter().find(|(name, _)| *name == mode).ok_or_else(|| fmt!("no mode `{mode}`"))?.1;
        let ty = fields.remove("type");
        let expr = fields.remove("expr").ok_or("no `expr:`")?;
        if let Some(key) = fields.keys().next() {
            return Err(fmt!("`{key}:` is not a field"));
        }
        let checks = matches!(way, Way::Checked | Way::Placed(_));
        if checks != ty.is_some() {
            return Err(fmt!("mode `{mode}` {} a `type:`", if checks { "needs" } else { "takes no" }));
        }
        Ok(Written { way, ty, expr })
    }

    /// The case as [`Self::read`] reads it.
    fn write(&self) -> String {
        let mut text = fmt!("mode: {}\n", self.way.name());
        if let Some(ty) = &self.ty {
            text.push_str(&fmt!("type: {ty}\n"));
        }
        text.push_str(&fmt!("expr: {}\n", self.expr));
        text
    }
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
    /// The case as text, put `way`.
    fn written(&self, db: &'db Db, way: Way) -> Written {
        Written {
            way,
            ty: self.checked_as.as_ref().map(|ty| ty.syntax(db).S()),
            expr: datalove_datalit::pretty::pretty_print(db, &self.expr),
        }
    }

    fn check(&self, db: &'db Db, way: Way) -> Option<Difference> {
        self.written(db, way).check(db)
    }
}

/// Where in a datafun script a checked expression is put, each checking it
/// against its type by its own route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// A function's argument, against its parameter.
    Argument,
    /// What a function returns, against its return type.
    Return,
    /// A `set` of a `var`, against the variable's type.
    Set,
    /// A `var`'s value, against its type.
    Var,
}

impl Place {
    const ALL: [Place; 4] = [Place::Argument, Place::Return, Place::Set, Place::Var];

    /// A script putting `expr` here, checked against `ty`.
    fn script(self, ty: &str, expr: &str) -> String {
        match self {
            Place::Argument => fmt!("fun _f(_p: {ty})\nend fun\ncall _f({expr})"),
            Place::Return => fmt!("fun _f(): {ty}\n  ret {expr}\nend fun"),
            Place::Set => fmt!("var _x: {ty} = {expr}\nset _x = {expr}"),
            Place::Var => fmt!("var _x: {ty} = {expr}"),
        }
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

/// The smallest case found that `still` finds differs as `case` does, the
/// same way.
fn shrink<'db>(case: Case<'db>, still: impl Fn(&Case<'db>) -> Option<Difference>, differs: Differs) -> Case<'db> {
    let mut case = case;
    'smaller: loop {
        for expr in smaller(&case.expr) {
            let candidate = Case { expr, ..case.C() };
            if still(&candidate).is_some_and(|d| d.differs == differs) {
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

/// Write a difference out as a case the kept cases can take, with what the
/// two sides said in comments, and say where.
fn write_out(label: &str, difference: &Difference) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("literal-equiv");
    std::fs::create_dir_all(&dir).X();
    let path = dir.join(fmt!("{}.case", label.replace(' ', "-")));
    let said = fmt!(
        "{label}: {:?}\n\ndatalit:\n{}\n\ndatafun:\n{}\n\n{}",
        difference.differs, difference.datalit_text, difference.datafun_text, difference.detail,
    );
    let comments: String = said.lines().map(|line| fmt!("// {line}\n")).collect();
    std::fs::write(&path, fmt!("{comments}\n{}", difference.written.write())).X();
    path
}

/// Print a difference, written out, and give its label back.
fn report(label: String, difference: &Difference) -> String {
    let path = write_out(&label, difference);
    println!(
        "\x1b[31mFAIL\x1b[0m {label}: {:?}\n  datalit: {}\n  datafun: {}\n  {}",
        difference.differs, difference.datalit_text, difference.datafun_text, path.display(),
    );
    label
}

/// The kept cases, by file name, in name order: differences found once,
/// and fixed, and some cases written by hand. See `literal_equiv/README.md`.
fn kept() -> Vec<(String, Written)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/literal_equiv");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir).X()
        .map(|entry| entry.X().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "case"))
        .collect();
    paths.sort();
    paths.into_iter()
        .map(|path| {
            let name = path.file_name().X().to_string_lossy().S();
            let written = Written::read(&std::fs::read_to_string(&path).X())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, written)
        })
        .collect()
}

/// Put seed `seed`'s expressions every way, saying how many and which
/// differ, each shrunk by the check that found it.
fn check_seed(seed: u64) -> (Counts, Vec<(String, Difference)>) {
    let db = Db::default();
    let mut counts = Counts::default();
    let mut differences = Vec::new();
    let shrunk = |case: Case<'_>, check: &dyn Fn(&Case<'_>) -> Option<Difference>| {
        check(&case).map(|found| check(&shrink(case, check, found.differs)).X())
    };
    let mut report = |label: String, difference: Option<Difference>| differences.extend(difference.map(|d| (label, d)));

    let case = inferred(&db, seed);
    counts.inferred += 1;
    match shrunk(case.C(), &|c| c.check(&db, Way::Inferred)) {
        Some(difference) => report(fmt!("seed {seed} inferred"), Some(difference)),
        None => {
            counts.valued += 1;
            report(fmt!("seed {seed} valued"), shrunk(case, &|c| c.check(&db, Way::Valued)));
        }
    }

    let case = checked(&db, seed);
    counts.checked += 1;
    report(fmt!("seed {seed} checked"), shrunk(case.C(), &|c| c.check(&db, Way::Checked)));
    for place in Place::ALL {
        counts.placed += 1;
        let label = fmt!("seed {seed} {}", fmt!("{place:?}").to_lowercase());
        report(label, shrunk(case.C(), &|c| c.check(&db, Way::Placed(place))));
    }

    if let Some((text, description)) = mutated(&db, seed) {
        counts.mutated += 1;
        let written = Written { way: Way::Mutated, ty: None, expr: text };
        let difference = written.check(&db).map(|mut difference| {
            difference.detail = fmt!("{description}\n{}", difference.detail);
            difference
        });
        report(fmt!("seed {seed} mutated"), difference);
    }
    (counts, differences)
}

fn main() {
    let db = Db::default();
    if let Ok(text) = std::env::var("LITERAL_EQUIV_TEXT") {
        match (Written { way: Way::Inferred, ty: None, expr: text.C() }).check(&db) {
            None => println!("\x1b[32msame\x1b[0m {text}"),
            Some(difference) => {
                println!("\x1b[31mFAIL\x1b[0m {text}: {:?}\n{}", difference.differs, difference.detail);
                std::process::exit(1);
            }
        }
        return;
    }
    let mut failed = Vec::new();
    let kept = kept();
    for (name, written) in &kept {
        if let Some(difference) = written.check(&db) {
            failed.push(report(fmt!("kept {name}"), &difference));
        }
    }

    let seeds = env_seeds("LITERAL_EQUIV", DEFAULT_SEED, DEFAULT_COUNT);
    // Each seed on its own, with its own database, and reported in order.
    let mut checked: Vec<(u64, Counts, Vec<(String, Difference)>)> = seeds.iter().collect::<Vec<_>>()
        .into_par_iter()
        .map(|seed| {
            let (counts, differences) = check_seed(seed);
            (seed, counts, differences)
        })
        .collect();
    checked.sort_by_key(|(seed, _, _)| *seed);
    let mut counts = Counts::default();
    for (_, seed_counts, differences) in checked {
        counts.add(&seed_counts);
        for (label, difference) in differences {
            failed.push(report(label, &difference));
        }
    }
    println!(
        "literal equivalence: {} kept cases, and {} inferred, {} valued, {} checked, {} placed and {} mutated cases \
         from {} seeds from {}; {} differ",
        kept.len(), counts.inferred, counts.valued, counts.checked, counts.placed, counts.mutated,
        seeds.count, seeds.start, failed.len(),
    );
    if !failed.is_empty() {
        std::process::exit(1);
    }
}
