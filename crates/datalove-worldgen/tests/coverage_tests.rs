//! What of the language the generator actually writes.
//!
//! A generator is only worth what it covers, and the way to be wrong about
//! that is to keep a hand-written list of things to look for. A list like that
//! only ever reports on what someone already thought of, and it reported
//! everything was fine through the stretch where `gen_type_hint` was clamped to
//! leaf types and no generated signature held a list, a map, a tuple or an
//! option.
//!
//! So the checklist is not written down. It comes from the AST: every variant
//! of `Statement`, `ExprFunKind`, `BinOp`, `UnaryOp`, `ParamMode` and datalit's
//! `TypeHint`. The worldfiles are parsed with the real parser and walked, and
//! what is never reached has to be named in `NOT_YET_GENERATED` with a reason.
//! Adding a variant to any of those enums makes `kind_names!` fail to compile
//! until someone says which of the two it is.
//!
//! Counting by parsing rather than by searching the text matters more than it
//! sounds. A first pass of this done with regular expressions reported field
//! projection at 100%, matching the dots in `import io_0.fn0`, and division at
//! 100%, matching the slash in `: u32 / 5`. Both are in fact never generated.

use std::collections::{BTreeMap, BTreeSet};

use datalove_datafun::package_load_worldfile::{self, WorldfileSection};
use datalove_datafun_ast::ast::{
    BinOp, ExprFun, ExprFunKind, ParamMode, Statement, UnaryOp,
};
use datalove_datalit::ast::TypeHint;
use datalove_worldgen::{gen_worldfile_seeded, WorldGenConfig};

/// How many worldfiles to look at.
///
/// Enough that anything the generator writes at all shows up: the rarest thing
/// it does write lands in about one file in fifty.
const SEEDS: u64 = 300;

/// Name every variant of an enum, and keep the roll of them beside it.
///
/// The list and the match come from one place, so neither can drift from the
/// other, and the match is exhaustive, so a new variant is a compile error
/// rather than a silent gap in the roll.
macro_rules! kind_names {
    ($fn_name:ident, $enum:ident, $ty:ty, $all:ident, [$($variant:ident),+ $(,)?]) => {
        const $all: &[&str] = &[$(stringify!($variant)),+];

        fn $fn_name(value: &$ty) -> &'static str {
            match value {
                $($enum::$variant { .. } => stringify!($variant),)+
            }
        }
    };
}

kind_names!(statement_name, Statement, Statement<'_>, ALL_STATEMENTS, [
    Let, Var, Const, Set, Fun, Ret, Require, Import, If, Loop, Break, Continue,
    DebugLog, TypeAlias, NativeFun, Match, ExprStatement, ParseError,
]);

kind_names!(expr_name, ExprFunKind, ExprFunKind<'_>, ALL_EXPRS, [
    BinOp, FunctionCall, Tuple, UnaryOp, TryOption, TryResult, CloneCoerce,
    FieldProj, True, False, None, Int, Float, Hex, String, List, Set, Map,
    Tensor, AnonTuple, AnonStruct, Some, Ok, Er, Data, Error, Table, Atom,
    Term, EnumLiteral, Index, Place, ParseError, IntrinsicCall,
]);

kind_names!(binop_name, BinOp, BinOp, ALL_BINOPS, [
    Add, Sub, Mul, Div, AddChecked, SubChecked, MulChecked, DivChecked,
    AddOptional, SubOptional, MulOptional, DivOptional, Lt, Gt, Le, Ge, Eq, Ne,
    And, Or, Xor,
]);

kind_names!(unop_name, UnaryOp, UnaryOp, ALL_UNOPS, [Neg, NegOptional, NegResult, Not]);

kind_names!(param_mode_name, ParamMode, ParamMode, ALL_PARAM_MODES, [In, Out, Ref, Mut]);

kind_names!(type_name, TypeHint, TypeHint<'_>, ALL_TYPES, [
    Bool, U8, I8, U16, I16, U32, I32, U64, I64, Index, Offset, F32, F64, Int,
    String, AnonTuple, AnonStruct, List, Map, Set, Option, Result, Tensor,
    Table, Data, Error, Alias, Atom, Term, Enum, ParseError,
]);

/// Forms that are one variant of an enum with a field present or absent.
///
/// A `loop` with a condition and a bare `loop` are the same `Statement::Loop`,
/// and lower differently. The roll taken from the AST cannot tell them apart,
/// so they are named here. Unlike the enums, this list is hand-written and can
/// go stale -- it is worth what someone thought to put in it.
const ALL_SHAPES: &[&str] = &[
    "fun with a return",
    "void fun",
    "generic fun",
    "bounded generic fun",
    "if alone",
    "if with else",
    "if with a binding",
    "if with an error binding",
    "loop while",
    "bare loop",
    "place with a field step",
    "place with an index step",
];

/// What the generator does not write yet, and why.
///
/// Every one of these is a gap rather than a decision, unless it says
/// otherwise. Take one off this list by making the generator write it -- the
/// test fails on anything listed here that turns up, so the list cannot rot in
/// the other direction either.
const NOT_YET_GENERATED: &[(&str, &str, &str)] = &[
    // (category, kind, why not)
    ("statement", "Const", "const declarations are not generated"),
    ("statement", "Continue", "would make a loop's final break unreachable"),
    ("statement", "NativeFun", "natives need a rider to resolve against"),
    ("statement", "ParseError", "a parse error means the generator wrote something wrong"),

    // `v.a` and `v[i]?` are written, and parse as a place with a step rather
    // than as these -- which are for a base that is not a place, like `f().0`.
    // See the two place-step shapes.
    ("expr", "FieldProj", "a projection off something that is not a place, like `f().0`, is not generated"),
    ("expr", "Hex", "hex literals are not generated"),
    ("expr", "Table", "table literals are not generated"),
    ("expr", "Index", "an index off something that is not a place, like `f()[i]?`, is not generated"),
    ("expr", "IntrinsicCall", "`icall` names intrinsics the generator does not know"),
    ("expr", "ParseError", "a parse error means the generator wrote something wrong"),




    // A type parameter parses as an alias too, and those are everywhere.
    // `TypeParams` tells the two apart, so an alias count is alias references.
    ("type", "Table", "table types are not generated"),
    ("type", "ParseError", "a parse error means the generator wrote something wrong"),
];

/// What was seen, by category and kind.
#[derive(Default)]
struct Coverage {
    seen: BTreeMap<(&'static str, &'static str), usize>,
}

impl Coverage {
    fn hit(&mut self, category: &'static str, kind: &'static str) {
        *self.seen.entry((category, kind)).or_insert(0) += 1;
    }

    fn count(&self, category: &str, kind: &str) -> usize {
        self.seen
            .iter()
            .find(|((c, k), _)| *c == category && *k == kind)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }
}

/// The type parameters a body is being read under.
///
/// `fun f<T>(x: T)` writes `T` where a type goes, and the datalit type parser
/// has no reason to know it is a parameter rather than the name of an alias --
/// both arrive as `TypeHint::Alias`. Without this, a generic body would report
/// aliases as thoroughly covered while no alias had ever been referred to.
#[derive(Default)]
struct TypeParams {
    names: BTreeSet<String>,
}

fn walk_type(
    db: &dyn salsa::Database,
    ty: &TypeHint<'_>,
    params: &TypeParams,
    cov: &mut Coverage,
) {
    let is_type_param = match ty {
        TypeHint::Alias(name) => params.names.contains(name.text(db)),
        _ => false,
    };
    if is_type_param {
        cov.hit("type", "(type parameter)");
    } else {
        cov.hit("type", type_name(ty));
    }
    match ty {
        TypeHint::List(t) => walk_type(db, &t.element_type, params, cov),
        TypeHint::Set(t) => walk_type(db, &t.element_type, params, cov),
        TypeHint::Option(t) => walk_type(db, &t.inner_type, params, cov),
        TypeHint::Result(t) => walk_type(db, &t.inner_type, params, cov),
        TypeHint::Tensor(t) => walk_type(db, &t.element_type, params, cov),
        TypeHint::Map(t) => {
            walk_type(db, &t.key_type, params, cov);
            walk_type(db, &t.value_type, params, cov);
        }
        TypeHint::AnonTuple(t) => t.fields.iter().for_each(|f| walk_type(db, f, params, cov)),
        TypeHint::AnonStruct(t) => t.fields.iter().for_each(|f| walk_type(db, &f.type_hint, params, cov)),
        _ => {}
    }
}

fn walk_opt_type(
    db: &dyn salsa::Database,
    ty: &Option<TypeHint<'_>>,
    params: &TypeParams,
    cov: &mut Coverage,
) {
    if let Some(ty) = ty {
        walk_type(db, ty, params, cov);
    }
}

fn walk_expr<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    params: &TypeParams,
    cov: &mut Coverage,
) {
    let kind = expr.expr(db);
    cov.hit("expr", expr_name(&kind));

    let mut sub = |e: ExprFun<'db>, cov: &mut Coverage| walk_expr(db, e, params, cov);

    match kind {
        ExprFunKind::BinOp(e) => {
            cov.hit("binop", binop_name(&e.op));
            sub(e.lhs, cov);
            sub(e.rhs, cov);
        }
        ExprFunKind::UnaryOp(e) => {
            cov.hit("unop", unop_name(&e.op));
            sub(e.operand, cov);
        }
        ExprFunKind::FunctionCall(e) => {
            for mode in e.args(db).iter().zip(e.arg_modes(db).iter()).filter_map(|(_, m)| m.as_ref()) {
                cov.hit("param_mode", param_mode_name(mode));
            }
            for arg in e.args(db).iter() {
                sub(*arg, cov);
            }
        }
        ExprFunKind::IntrinsicCall(e) => e.args.iter().for_each(|a| sub(*a, cov)),
        ExprFunKind::Tuple(e) => e.elements.iter().for_each(|a| sub(*a, cov)),
        ExprFunKind::TryOption(e) => sub(e.operand, cov),
        ExprFunKind::TryResult(e) => sub(e.operand, cov),
        ExprFunKind::CloneCoerce(e) => sub(e.operand, cov),
        ExprFunKind::FieldProj(e) => sub(e.base, cov),
        ExprFunKind::Index(e) => {
            sub(e.base, cov);
            sub(e.index, cov);
        }
        ExprFunKind::True(e) | ExprFunKind::False(e) | ExprFunKind::None(e) => {
            walk_opt_type(db, &e.type_hint, params, cov)
        }
        ExprFunKind::Int(e) => walk_opt_type(db, &e.type_hint, params, cov),
        ExprFunKind::Float(e) => walk_opt_type(db, &e.type_hint, params, cov),
        ExprFunKind::Hex(e) => walk_opt_type(db, &e.type_hint, params, cov),
        ExprFunKind::String(e) => walk_opt_type(db, &e.type_hint, params, cov),
        ExprFunKind::List(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            e.elements.iter().for_each(|a| sub(*a, cov));
        }
        ExprFunKind::Set(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            e.elements.iter().for_each(|a| sub(*a, cov));
        }
        ExprFunKind::Map(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            for entry in e.entries.iter() {
                sub(entry.key, cov);
                sub(entry.value, cov);
            }
        }
        ExprFunKind::Tensor(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            e.elements.iter().for_each(|a| sub(*a, cov));
        }
        ExprFunKind::AnonTuple(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            e.elements.iter().for_each(|a| sub(*a, cov));
        }
        ExprFunKind::AnonStruct(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            e.fields.iter().for_each(|f| sub(f.value, cov));
        }
        ExprFunKind::Some(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            sub(e.payload, cov);
        }
        ExprFunKind::Ok(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            sub(e.payload, cov);
        }
        ExprFunKind::Er(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            sub(e.payload, cov);
        }
        ExprFunKind::Data(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            sub(e.value, cov);
        }
        ExprFunKind::Error(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            sub(e.value, cov);
        }
        ExprFunKind::Table(e) => {
            walk_opt_type(db, &e.type_hint, params, cov);
            for row in e.rows.iter() {
                row.elements.iter().for_each(|a| sub(*a, cov));
            }
        }
        ExprFunKind::Term(e) => sub(e.payload, cov),
        ExprFunKind::EnumLiteral(e) => sub(e.variant, cov),
        // A place is a name and the steps taken from it, and the steps are
        // where a field projection and an indexing actually land: `v.a` and
        // `v[i]?` parse as a place with a step rather than as `FieldProj` or
        // `Index`, which are for a base that is not a place. Without this the
        // roll reports both as never written while the generator writes them.
        ExprFunKind::Place(place) => {
            for step in place.steps.iter() {
                match step {
                    datalove_datafun_ast::ast::PlaceStep::Field(_) => {
                        cov.hit("shape", "place with a field step")
                    }
                    datalove_datafun_ast::ast::PlaceStep::Index(idx) => {
                        cov.hit("shape", "place with an index step");
                        sub(idx.index, cov);
                    }
                }
            }
        }
        ExprFunKind::Atom(_) | ExprFunKind::ParseError(_) => {}
    }
}

fn walk_statements<'db>(
    db: &'db dyn salsa::Database,
    statements: &[Statement<'db>],
    params: &TypeParams,
    cov: &mut Coverage,
) {
    for statement in statements {
        cov.hit("statement", statement_name(statement));
        match statement {
            Statement::Let(s) => {
                walk_opt_type(db, &s.type_hint, params, cov);
                walk_expr(db, s.value, params, cov);
            }
            Statement::Var(s) => {
                walk_opt_type(db, &s.type_hint, params, cov);
                if let Some(value) = s.value {
                    walk_expr(db, value, params, cov);
                }
            }
            Statement::Const(s) => {
                walk_opt_type(db, &s.type_hint, params, cov);
                walk_expr(db, s.value, params, cov);
            }
            Statement::Set(s) => walk_expr(db, s.value, params, cov),
            Statement::Fun(s) => {
                cov.hit(
                    "shape",
                    if s.return_type(db).is_some() { "fun with a return" } else { "void fun" },
                );
                if !s.type_params(db).is_empty() {
                    cov.hit("shape", "generic fun");
                    if s.type_bounds(db).iter().any(|b| b.is_some()) {
                        cov.hit("shape", "bounded generic fun");
                    }
                }
                // Read the body under this function's own type parameters, so
                // a `T` in it is not counted as a reference to an alias.
                let params = &TypeParams {
                    names: s
                        .type_params(db)
                        .iter()
                        .map(|name| name.text(db).to_string())
                        .collect(),
                };
                for param in s.params(db).iter() {
                    cov.hit("param_mode", param_mode_name(&param.mode));
                    walk_type(db, &param.type_hint, params, cov);
                }
                walk_opt_type(db, &s.return_type(db), params, cov);
                walk_statements(db, s.body(db), params, cov);
            }
            Statement::Ret(s) => {
                if let Some(value) = s.value {
                    walk_expr(db, value, params, cov);
                }
            }
            Statement::If(s) => {
                cov.hit(
                    "shape",
                    if s.else_body.is_some() { "if with else" } else { "if alone" },
                );
                if s.then_binding.is_some() {
                    cov.hit("shape", "if with a binding");
                }
                if s.else_binding.is_some() {
                    cov.hit("shape", "if with an error binding");
                }
                walk_expr(db, s.condition, params, cov);
                walk_statements(db, &s.then_body, params, cov);
                if let Some(else_body) = &s.else_body {
                    walk_statements(db, else_body, params, cov);
                }
            }
            Statement::Loop(s) => {
                cov.hit(
                    "shape",
                    if s.condition.is_some() { "loop while" } else { "bare loop" },
                );
                if let Some(condition) = s.condition {
                    walk_expr(db, condition, params, cov);
                }
                walk_statements(db, &s.body, params, cov);
            }
            Statement::DebugLog(s) => walk_expr(db, s.value, params, cov),
            Statement::TypeAlias(s) => walk_type(db, &s.type_hint, params, cov),
            Statement::Match(s) => {
                walk_expr(db, s.input, params, cov);
                for case in s.cases.iter() {
                    walk_statements(db, &case.body, params, cov);
                }
                if let Some(default_body) = &s.default_body {
                    walk_statements(db, default_body, params, cov);
                }
            }
            Statement::ExprStatement(s) => walk_expr(db, s.expr, params, cov),
            Statement::NativeFun(_)
            | Statement::Require(_)
            | Statement::Import(_)
            | Statement::Break(_)
            | Statement::Continue(_)
            | Statement::ParseError(_) => {}
        }
    }
}

/// Walk every section of a generated worldfile.
fn measure(seeds: u64) -> Coverage {
    let mut cov = Coverage::default();
    let config = WorldGenConfig::default();

    for seed in 0..seeds {
        let worldfile = gen_worldfile_seeded(seed, config.clone());
        let parsed = package_load_worldfile::parse_worldfile_sections(worldfile.as_bytes())
            .expect("generated worldfile parses into sections");

        let db = datalove_datafun::Database::default();
        for section in &parsed.sections {
            let source_text = match section {
                WorldfileSection::Module { source, .. }
                | WorldfileSection::ModuleAdd { source, .. } => source.clone(),
                WorldfileSection::ScriptFragment { source } => source.clone(),
                _ => continue,
            };
            let source = bct::input::Source::new(&db, source_text);
            let parsed = datalove_datafun_parser::parse_integration_test(&db, source);
            walk_statements(&db, &parsed.statements, &TypeParams::default(), &mut cov);
        }
    }

    cov
}

/// Report what the generator writes, and fail on anything it writes that is
/// written down here as something it does not.
#[test]
fn test_language_coverage() {
    let cov = measure(SEEDS);

    let categories: [(&str, &[&str]); 6] = [
        ("statement", ALL_STATEMENTS),
        ("expr", ALL_EXPRS),
        ("binop", ALL_BINOPS),
        ("unop", ALL_UNOPS),
        ("param_mode", ALL_PARAM_MODES),
        ("type", ALL_TYPES),
    ];

    let expected_missing: BTreeSet<(&str, &str)> = NOT_YET_GENERATED
        .iter()
        .map(|(category, kind, _)| (*category, *kind))
        .collect();

    let mut uncovered = Vec::new();
    let mut unexpectedly_covered = Vec::new();

    println!("Language coverage over {} worldfiles:", SEEDS);
    for (category, kinds) in categories {
        println!("  {}:", category);
        for kind in kinds {
            let count = cov.count(category, kind);
            let listed = expected_missing.contains(&(category, *kind));
            match (count, listed) {
                (0, false) => uncovered.push(format!("{} {}", category, kind)),
                (_, true) if count > 0 => {
                    unexpectedly_covered.push(format!("{} {}", category, kind))
                }
                _ => {}
            }
            let note = if count == 0 { "   (never)" } else { "" };
            println!("    {:<16}{:>8}{}", kind, count, note);
        }
    }

    println!("  shape:");
    for shape in ALL_SHAPES {
        let count = cov.count("shape", shape);
        let note = if count == 0 { "   (never)" } else { "" };
        println!("    {:<24}{:>8}{}", shape, count, note);
        let listed = expected_missing.contains(&("shape", *shape));
        match (count, listed) {
            (0, false) => uncovered.push(format!("shape {}", shape)),
            (_, true) if count > 0 => unexpectedly_covered.push(format!("shape {}", shape)),
            _ => {}
        }
    }

    println!(
        "  (type parameter references, which parse as aliases: {})",
        cov.count("type", "(type parameter)")
    );

    assert!(
        uncovered.is_empty(),
        "these are never generated and are not written down in NOT_YET_GENERATED:\n  {}\n\
         Either make the generator write them, or add them to the list with a reason.",
        uncovered.join("\n  ")
    );

    assert!(
        unexpectedly_covered.is_empty(),
        "these are written down in NOT_YET_GENERATED but the generator does write them:\n  {}\n\
         Take them off the list.",
        unexpectedly_covered.join("\n  ")
    );
}

/// How many worldfiles the floors below are counted over.
///
/// More than the roll needs, because a floor is a number rather than a yes: a
/// count that averages a dozen over fifty worldfiles came back three often
/// enough to fail on nothing having changed.
const FLOOR_SEEDS: u64 = 100;

/// The constructs the generator is meant to lean on, with enough of each that
/// a seed is likely to exercise them.
///
/// Separate from the roll above, which only asks whether a thing happens at
/// all. Something that happens once in three hundred worldfiles is covered in
/// the sense that the parser has seen it and not in any sense that matters.
#[test]
fn test_core_constructs_are_common() {
    let cov = measure(FLOOR_SEEDS);

    // (category, kind, how many across FLOOR_SEEDS worldfiles). Each is set
    // well under what it measures, so that noise cannot trip it and a real
    // loss can.
    let floors: &[(&str, &str, usize)] = &[
        ("statement", "Let", 200),
        ("statement", "Var", 20),
        ("statement", "Set", 5),
        ("statement", "Fun", 100),
        ("statement", "Ret", 50),
        ("statement", "If", 5),
        ("statement", "Loop", 2),
        ("statement", "DebugLog", 10),
        ("expr", "FunctionCall", 100),
        ("expr", "List", 20),
        ("expr", "Set", 10),
        ("expr", "Map", 10),
        ("expr", "AnonTuple", 8),
        ("expr", "Some", 10),
        ("type", "List", 20),
        ("type", "Option", 20),
        ("type", "Map", 10),
        // These reached zero between runs before they were reached for
        // deliberately rather than waiting on two rolls to coincide.
        ("shape", "if with a binding", 10),
        ("shape", "if with an error binding", 3),
        ("expr", "TryOption", 3),
        ("expr", "TryResult", 2),
        ("statement", "Match", 20),
        ("expr", "EnumLiteral", 20),
    ];

    let mut thin = Vec::new();
    for (category, kind, floor) in floors {
        let count = cov.count(category, kind);
        if count < *floor {
            thin.push(format!("{} {}: {} < {}", category, kind, count, floor));
        }
    }

    assert!(
        thin.is_empty(),
        "these got rarer than they were:\n  {}",
        thin.join("\n  ")
    );
}
