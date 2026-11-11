//! Differential tests of the type system against the Lean model in `formal/`.
//!
//! The model transcribes the Rust's type relations and proves things about
//! them; these tests are what tie the transcription to the code. They
//! generate type hints and types, ask the Rust and the model's oracle the same
//! questions, and compare the answers. See `botdocs/formal.md`.

use rmx::prelude::*;
use rmx::rand::seq::SliceRandom;
use rmx::serde_json::{Value, json};
use bct::random::SeededRng;
use bct::text::InternedText;
use datalove_datalit::ast::*;
use datalove_datalit::tycheck::{self as tc, RecErrorKind, Type, TypeError};
use std::path::{Path, PathBuf};

pub type Db = datalove_datalit::Database;

/// How many random pairs of types [`pairs`] asks about, beside those built
/// to be related.
pub const RANDOM_PAIRS: usize = 4000;

/// The oracle executable `lake build` makes in `formal/`.
pub fn oracle_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../formal/.lake/build/bin/datalove-oracle")
}

/// Ask the oracle each query, and return its answers in order.
pub fn ask_oracle(queries: &[Value]) -> Vec<Value> {
    let oracle = oracle_path();
    assert!(
        oracle.exists(),
        "no oracle at {}; run `just formal-build` (needs Lean, see botdocs/formal.md)",
        oracle.display(),
    );
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("queries.jsonl");
    let text: String = queries.iter().map(|q| fmt!("{q}\n")).collect();
    std::fs::write(&path, text).X();
    let output = std::process::Command::new(&oracle).arg(&path).output().X();
    assert!(
        output.status.success(),
        "the oracle failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let answers: Vec<Value> = String::from_utf8(output.stdout).X()
        .lines()
        .map(|line| rmx::serde_json::from_str(line).X())
        .collect();
    assert_eq!(answers.len(), queries.len(), "the oracle answered a different number of queries");
    answers
}

fn scalar_hint_name(hint: &TypeHint<'_>) -> Option<&'static str> {
    Some(match hint {
        TypeHint::Bool => "bool",
        TypeHint::U8 => "u8",
        TypeHint::I8 => "i8",
        TypeHint::U16 => "u16",
        TypeHint::I16 => "i16",
        TypeHint::U32 => "u32",
        TypeHint::I32 => "i32",
        TypeHint::U64 => "u64",
        TypeHint::I64 => "i64",
        TypeHint::Index => "index",
        TypeHint::Offset => "offset",
        TypeHint::F32 => "f32",
        TypeHint::F64 => "f64",
        TypeHint::Int => "int",
        TypeHint::String => "string",
        TypeHint::Data => "data",
        TypeHint::Error => "error",
        _ => return None,
    })
}

/// A hint in the oracle's encoding; see `formal/Datalove/Json.lean`.
pub fn hint_to_json(db: &Db, hint: &TypeHint<'_>) -> Value {
    if let Some(name) = scalar_hint_name(hint) {
        return json!(name);
    }
    let named = |fs: &[TypeHintNamedField<'_>]| -> Value {
        fs.iter().map(|f| json!([f.name.as_str(db), hint_to_json(db, &f.type_hint)])).collect()
    };
    match hint {
        TypeHint::AnonTuple(t) => json!(["tuple", t.fields.iter().map(|f| hint_to_json(db, f)).collect::<Vec<_>>()]),
        TypeHint::AnonStruct(s) => json!(["struct", named(&s.fields)]),
        TypeHint::Table(t) => json!(["table", named(&t.columns)]),
        TypeHint::List(l) => json!(["list", hint_to_json(db, &l.element_type)]),
        TypeHint::Map(m) => json!(["map", hint_to_json(db, &m.key_type), hint_to_json(db, &m.value_type)]),
        TypeHint::Set(s) => json!(["set", hint_to_json(db, &s.element_type)]),
        TypeHint::Option(o) => json!(["option", hint_to_json(db, &o.inner_type)]),
        TypeHint::Result(r) => json!(["result", hint_to_json(db, &r.inner_type)]),
        TypeHint::Tensor(t) => json!(["tensor", hint_to_json(db, &t.element_type), t.rank]),
        TypeHint::Atom(a) => json!(["atom", a.name.as_str(db)]),
        TypeHint::Term(t) => json!(["term", t.name.as_str(db), hint_to_json(db, &t.payload)]),
        TypeHint::Enum(e) => json!(["enum", e.variants.iter()
            .map(|v| json!([v.name.as_str(db), v.payload.as_ref().map(|p| hint_to_json(db, p))]))
            .collect::<Vec<_>>()]),
        TypeHint::Rec(r) => json!(["rec", r.name.as_str(db), hint_to_json(db, &r.body)]),
        TypeHint::Alias(a) => json!(["name", a.name.as_str(db)]),
        TypeHint::ParseError(_) => panic!("the generator makes no parse errors"),
        _ => unreachable!("scalars are handled above"),
    }
}

/// A type in the oracle's encoding.
pub fn type_to_json(db: &Db, ty: &Type<'_>) -> Value {
    let named = |fs: &[datalove_datalit::tycheck::TypeNamedField<'_>]| -> Value {
        fs.iter().map(|f| json!([f.name.as_str(db), type_to_json(db, &f.ty)])).collect()
    };
    match ty {
        Type::Bool => json!("bool"),
        Type::U8 => json!("u8"),
        Type::I8 => json!("i8"),
        Type::U16 => json!("u16"),
        Type::I16 => json!("i16"),
        Type::U32 => json!("u32"),
        Type::I32 => json!("i32"),
        Type::U64 => json!("u64"),
        Type::I64 => json!("i64"),
        Type::Index => json!("index"),
        Type::Offset => json!("offset"),
        Type::F32 => json!("f32"),
        Type::F64 => json!("f64"),
        Type::Int => json!("int"),
        Type::String => json!("string"),
        Type::Data => json!("data"),
        Type::Error => json!("error"),
        Type::AnonTuple(t) => json!(["tuple", t.fields.iter().map(|f| type_to_json(db, f)).collect::<Vec<_>>()]),
        Type::AnonStruct(s) => json!(["struct", named(&s.fields)]),
        Type::Table(t) => json!(["table", named(&t.columns)]),
        Type::List(l) => json!(["list", type_to_json(db, &l.element_type)]),
        Type::Map(m) => json!(["map", type_to_json(db, &m.key_type), type_to_json(db, &m.value_type)]),
        Type::Set(s) => json!(["set", type_to_json(db, &s.element_type)]),
        Type::Option(o) => json!(["option", type_to_json(db, &o.inner_type)]),
        Type::Result(r) => json!(["result", type_to_json(db, &r.inner_type)]),
        Type::Tensor(t) => json!(["tensor", type_to_json(db, &t.element_type), t.rank]),
        Type::Atom(a) => json!(["atom", a.name.as_str(db)]),
        Type::Term(t) => json!(["term", t.name.as_str(db), type_to_json(db, &t.payload)]),
        Type::Enum(e) => json!(["enum", e.variants.iter()
            .map(|v| json!([v.name.as_str(db), v.payload.as_ref().map(|p| type_to_json(db, p))]))
            .collect::<Vec<_>>()]),
        Type::Var(n) => json!(["var", n.as_str(db)]),
        Type::Rec(r) => json!(["rec", type_to_json(db, &r.body)]),
        Type::RecVar(i) => json!(["recvar", i]),
    }
}

/// What converting a hint came to, in the oracle's encoding.
pub fn conversion_to_json<'db>(db: &'db Db, result: &Result<Type<'db>, TypeError>) -> Value {
    match result {
        Ok(ty) => json!({ "ok": type_to_json(db, ty) }),
        Err(TypeError::Rec(e)) => {
            let kind = match e.kind {
                RecErrorKind::Unused => "unused",
                RecErrorKind::Shadowed => "shadowed",
                RecErrorKind::Unguarded => "unguarded",
                RecErrorKind::InKey => "in_key",
                RecErrorKind::Generic => panic!("datalit has no type parameters to be inside a rec"),
            };
            json!({ "err": { "kind": kind, "name": e.name } })
        }
        // Datalit's `NoAliases` says a name no `rec` binds is nothing it can
        // synthesize, and does not say which name.
        Err(TypeError::CannotSynthesize) => json!({ "err": { "kind": "unbound" } }),
        Err(e) => panic!("converting a generated hint failed unexpectedly: {e}"),
    }
}

/// The oracle's answer with what the Rust cannot say taken out: an unbound
/// name's name.
pub fn comparable_conversion(answer: &Value) -> Value {
    let mut answer = answer.C();
    if answer["err"]["kind"] == "unbound" {
        answer["err"].as_object_mut().X().remove("name");
    }
    answer
}

/// Random type hints, of the shapes the parser produces.
pub struct HintGen<'db> {
    db: &'db Db,
    rng: SeededRng,
}

const SCALARS: &[fn() -> TypeHint<'static>] = &[
    || TypeHint::Bool, || TypeHint::U8, || TypeHint::I8, || TypeHint::U16, || TypeHint::I16,
    || TypeHint::U32, || TypeHint::I32, || TypeHint::U64, || TypeHint::I64, || TypeHint::Index,
    || TypeHint::Offset, || TypeHint::F32, || TypeHint::F64, || TypeHint::Int, || TypeHint::String,
    || TypeHint::Data, || TypeHint::Error,
];

/// Few names, so that `rec`s shadow each other and uses find the wrong one.
const REC_NAMES: &[&str] = &["L", "M", "N"];
const FIELD_NAMES: &[&str] = &["a", "b", "c", "d"];
const VARIANT_NAMES: &[&str] = &["A", "B", "C", "D", "E"];

impl<'db> HintGen<'db> {
    pub fn new(db: &'db Db, seed: u64) -> HintGen<'db> {
        HintGen { db, rng: SeededRng::new(seed) }
    }

    fn text(&self, s: &str) -> InternedText<'db> {
        InternedText::new(self.db, s.S())
    }

    fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
        from[self.rng.random_range(0..from.len())]
    }

    /// `n` distinct names from `from`, in a random order.
    fn distinct<'a>(&mut self, from: &[&'a str], n: usize) -> Vec<&'a str> {
        let mut names = from.to_vec();
        names.shuffle(&mut self.rng);
        names.truncate(n);
        names
    }

    /// A hint at most `depth` deep, with the `rec` names in `scope` bound
    /// around it.
    pub fn hint(&mut self, depth: u32, scope: &mut Vec<&'static str>) -> TypeHint<'db> {
        let boxed = |h| Box::new(h);
        if depth == 0 || self.rng.random_bool(0.25) {
            return self.leaf(scope);
        }
        let d = depth - 1;
        match self.rng.random_range(0..15) {
            0 => {
                let n = self.rng.random_range(0..4);
                TypeHint::AnonTuple(TypeHintAnonTuple { fields: (0..n).map(|_| self.hint(d, scope)).collect() })
            }
            1 => TypeHint::AnonStruct(TypeHintAnonStruct { fields: self.fields(d, scope) }),
            2 | 3 => TypeHint::List(TypeHintList { element_type: boxed(self.hint(d, scope)) }),
            4 => TypeHint::Map(TypeHintMap { key_type: boxed(self.hint(d, scope)), value_type: boxed(self.hint(d, scope)) }),
            5 => TypeHint::Set(TypeHintSet { element_type: boxed(self.hint(d, scope)) }),
            6 => TypeHint::Option(TypeHintOption { inner_type: boxed(self.hint(d, scope)) }),
            7 => TypeHint::Result(TypeHintResult { inner_type: boxed(self.hint(d, scope)) }),
            8 => TypeHint::Tensor(TypeHintTensor { element_type: boxed(self.hint(d, scope)), rank: self.rng.random_range(1..4) }),
            9 => TypeHint::Table(TypeHintTable { columns: self.fields(d, scope) }),
            10 => {
                let name = self.pick(VARIANT_NAMES);
                TypeHint::Term(TypeHintTerm { name: self.text(name), payload: boxed(self.hint(d, scope)) })
            }
            11 | 12 => {
                let n = self.rng.random_range(1..5);
                let names = self.distinct(VARIANT_NAMES, n);
                let variants = names.into_iter().map(|name| {
                    let payload = self.rng.random_bool(0.5).then(|| boxed(self.hint(d, scope)));
                    TypeHintEnumVariant { name: self.text(name), payload }
                }).collect();
                TypeHint::Enum(TypeHintEnum { variants })
            }
            _ => {
                let name = self.pick(REC_NAMES);
                scope.push(name);
                let body = self.hint(d, scope);
                scope.pop();
                TypeHint::Rec(TypeHintRec { name: self.text(name), body: boxed(body), local_index: None })
            }
        }
    }

    /// A hint to convert: often a `rec` at the top, which is where meeting at
    /// the head looks.
    pub fn top(&mut self, depth: u32) -> TypeHint<'db> {
        let mut scope = Vec::new();
        if !self.rng.random_bool(0.4) {
            return self.hint(depth, &mut scope);
        }
        let name = self.pick(REC_NAMES);
        scope.push(name);
        let mut body = self.hint(depth - 1, &mut scope);
        // A body that is only a leaf is mostly a scalar, which never uses the
        // name; give it a part to use the name in.
        while matches!(body, TypeHint::Alias(_)) || scalar_hint_name(&body).is_some() {
            body = self.hint(depth - 1, &mut scope);
        }
        TypeHint::Rec(TypeHintRec { name: self.text(name), body: Box::new(body), local_index: None })
    }

    fn fields(&mut self, depth: u32, scope: &mut Vec<&'static str>) -> Vec<TypeHintNamedField<'db>> {
        let n = self.rng.random_range(0..4);
        let names = self.distinct(FIELD_NAMES, n);
        names.into_iter()
            .map(|name| TypeHintNamedField { name: self.text(name), type_hint: Box::new(self.hint(depth, scope)) })
            .collect()
    }

    fn leaf(&mut self, scope: &[&'static str]) -> TypeHint<'db> {
        if !scope.is_empty() && self.rng.random_bool(0.5) {
            // Mostly a name in scope; sometimes one that is not, which is
            // unbound.
            let name = if self.rng.random_bool(0.9) {
                scope[self.rng.random_range(0..scope.len())]
            } else {
                self.pick(REC_NAMES)
            };
            let alias = TypeHint::Alias(TypeHintAlias { name: self.text(name), local_index: None });
            // Often inside a list, which guards it, so that more `rec`s are
            // well formed.
            return if self.rng.random_bool(0.5) {
                TypeHint::List(TypeHintList { element_type: Box::new(alias) })
            } else {
                alias
            };
        }
        match self.rng.random_range(0..10) {
            0 => {
                let name = self.pick(VARIANT_NAMES);
                TypeHint::Atom(TypeHintAtom { name: self.text(name) })
            }
            _ => SCALARS[self.rng.random_range(0..SCALARS.len())](),
        }
    }
}

/// Types the hints do not give: type parameters, which datalit has no
/// syntax for, and every scalar, so that every numeric pair is asked.
pub fn extra_types(db: &Db) -> Vec<Type<'_>> {
    let var = |n: &str| Type::Var(InternedText::new(db, n.S()));
    let mut types = vec![
        Type::Bool, Type::U8, Type::I8, Type::U16, Type::I16, Type::U32, Type::I32,
        Type::U64, Type::I64, Type::Index, Type::Offset, Type::F32, Type::F64, Type::Int,
        Type::String, Type::Data, Type::Error,
        var("T"), var("U"),
    ];
    types.push(Type::List(tc::TypeList { element_type: Box::new(var("T")) }));
    types
}

/// Pairs worth asking about. Random pairs of types are almost never related,
/// so beside them are pairs built to be: each type with itself and with what
/// it exposes to, every pair of scalars, and each variant of an enum with
/// the enum, both as the enum and as the recursive type that unfolds to it.
/// And the other way about: an atom or term, or a type that unfolds to one,
/// with an enum that lists it.
pub fn pairs<'db>(db: &'db Db, pool: &[Type<'db>], seed: u64) -> Vec<(Type<'db>, Type<'db>)> {
    // A variant that sorts after every generated one, so that an enum of it
    // and one other is sorted as the Rust keeps enums.
    let last = InternedText::new(db, "Z".S());
    let mut pairs = Vec::new();
    for ty in pool {
        pairs.push((ty.C(), ty.C()));
        if matches!(ty, Type::Rec(_)) {
            let exposed = tc::expose(ty).into_owned();
            pairs.push((ty.C(), exposed.C()));
            pairs.push((exposed.C(), ty.C()));
        }
        let exposed = tc::expose(ty).into_owned();
        if let Type::Enum(e) = &exposed {
            for v in &e.variants {
                let variant = match &v.payload {
                    None => Type::Atom(tc::TypeAtom { name: v.name }),
                    Some(p) => Type::Term(tc::TypeTerm { name: v.name, payload: p.C() }),
                };
                pairs.push((variant.C(), exposed.C()));
                pairs.push((variant.C(), ty.C()));
                // A term whose payload is not the variant's.
                if v.payload.is_some() {
                    let wrong = Type::Term(tc::TypeTerm { name: v.name, payload: Box::new(Type::Bool) });
                    pairs.push((wrong, exposed.C()));
                }
            }
        }
        let variant = match &exposed {
            Type::Atom(a) => Some(tc::TypeEnumVariant { name: a.name, payload: None }),
            Type::Term(t) => Some(tc::TypeEnumVariant { name: t.name, payload: Some(t.payload.C()) }),
            _ => None,
        };
        if let Some(variant) = variant {
            let other = tc::TypeEnumVariant { name: last, payload: None };
            let listing = Type::Enum(tc::TypeEnum { variants: vec![variant, other] });
            pairs.push((exposed.C(), listing.C()));
            pairs.push((ty.C(), listing));
        }
    }
    let scalars: Vec<&Type<'_>> = pool.iter().filter(|t| tc::is_numeric_type(t) || matches!(t, Type::Bool)).collect();
    for a in &scalars {
        for b in &scalars {
            pairs.push(((*a).C(), (*b).C()));
        }
    }
    let mut rng = SeededRng::derive(seed, "pairs");
    for _ in 0..RANDOM_PAIRS {
        let a = &pool[rng.random_range(0..pool.len())];
        let b = &pool[rng.random_range(0..pool.len())];
        pairs.push((a.C(), b.C()));
    }
    pairs
}

/// The distinct types the hints from `seeds` convert to, with
/// [`extra_types`], and every hint with what converting it came to.
pub fn type_pool<'db>(
    db: &'db Db,
    seeds: impl Iterator<Item = u64>,
    hints_per_seed: usize,
    depth: u32,
) -> (Vec<Type<'db>>, Vec<(u64, TypeHint<'db>, Result<Type<'db>, TypeError>)>) {
    let mut pool = Vec::new();
    let mut hints = Vec::new();
    for seed in seeds {
        let mut generator = HintGen::new(db, seed);
        for _ in 0..hints_per_seed {
            let hint = generator.top(depth);
            let converted = datalove_datalit::tycheck::convert_type_hint(db, &hint);
            if let Ok(ty) = &converted {
                pool.push(ty.C());
            }
            hints.push((seed, hint, converted));
        }
    }
    pool.extend(extra_types(db));
    let mut seen = std::collections::BTreeSet::new();
    pool.retain(|ty| seen.insert(type_to_json(db, ty).S()));
    (pool, hints)
}
