//! Inference of the type an expression synthesizes.
//!
//! Synthesis types the parts of an expression together: a collection's
//! elements go along with each other, so in `[1, : u8 / 2]` the `1` is a `u8`.
//! To find that type in one pass, inference gives each literal, and each
//! expression that leaves part of its type open (`none`, `[]`), a type
//! variable, and unifies the types that must agree. Variables nothing decides
//! take their literal's default afterwards, and a variable with no default is
//! a missing type hint.
//!
//! Inference reports nothing but malformed hints and parse errors. Where two
//! types disagree it keeps the first and moves on: synthesis then checks the
//! expression against the type it found, and that check reports the
//! disagreement, at the later expression, with the diagnostics checking
//! already has.

use bct::diagnostic::DiagnosticBuilder;
use bct::text::InternedText;
use datalove_diagnostic::DiagnosticBuilderExt;
use crate::ast::*;
use super::context::TypeContext;
use super::types::*;

/// A type variable, an index into a [`Unifier`].
pub type VarId = u32;

/// A type that may still hold variables.
#[derive(Clone)]
pub enum PType<'db> {
    /// A type with no variables in it.
    Known(Type<'db>),
    Var(VarId),
    AnonTuple(Vec<PType<'db>>),
    AnonStruct(Vec<(InternedText<'db>, PType<'db>)>),
    List(Box<PType<'db>>),
    Set(Box<PType<'db>>),
    Map(Box<PType<'db>>, Box<PType<'db>>),
    Option(Box<PType<'db>>),
    Result(Box<PType<'db>>),
    Tensor(Box<PType<'db>>, u32),
    Table(Vec<(InternedText<'db>, PType<'db>)>),
    Term(InternedText<'db>, Box<PType<'db>>),
    /// An enum's variants in name order, each with its payload if it has one.
    Enum(Vec<(InternedText<'db>, Option<PType<'db>>)>),
}

/// What a variable not yet bound to a type stands for.
#[derive(Clone)]
pub enum Class<'db> {
    /// An open type with no default, as for the payload of `none`.
    Open,
    /// A numeric literal, which defaults to the type its kind does.
    Literal(LiteralKind),
    /// An atom (no payload) or term literal, which defaults to its own type.
    Variant {
        name: InternedText<'db>,
        payload: Option<Box<PType<'db>>>,
    },
    /// A value adapted with `@` from the given type, which it keeps when
    /// nothing asks for another.
    Adapt(Type<'db>),
}

/// The kinds of numeric literal, which differ in the types they check
/// against.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LiteralKind {
    Int,
    Hex,
    Float,
}

impl LiteralKind {
    fn default_type<'db>(self) -> Type<'db> {
        match self {
            LiteralKind::Int | LiteralKind::Hex => Type::Int,
            LiteralKind::Float => Type::F64,
        }
    }
}

impl<'db> Class<'db> {
    /// Whether a value of this class could be of the type, by the type's
    /// head: whether the value is out of its range is for checking to say.
    fn admits(&self, db: &'db dyn crate::Db, ty: &PType<'db>) -> bool {
        match (self, ty) {
            (Class::Open, _) => true,
            (Class::Adapt(from), PType::Known(to)) => can_adapt_to(db, from, to),
            // A type with parts still undecided: what the parts must be for
            // the value to adapt is worked out when the variable is bound.
            (Class::Adapt(_), _) => true,
            (Class::Literal(LiteralKind::Int), PType::Known(t)) => is_fixed_int_type(t) || is_bigint_type(t),
            (Class::Literal(LiteralKind::Hex), PType::Known(t)) => {
                is_unsigned_int_type(t) || is_bigint_type(t) || is_float_type(t)
            }
            (Class::Literal(LiteralKind::Float), PType::Known(t)) => is_float_type(t),
            (Class::Variant { name, .. }, PType::Known(Type::Atom(a))) => a.name == *name,
            (Class::Variant { name, .. }, PType::Known(Type::Term(t))) => t.name == *name,
            (Class::Variant { name, .. }, PType::Term(term_name, _)) => term_name == name,
            (Class::Variant { .. }, PType::Known(Type::Enum(_)) | PType::Enum(_)) => true,
            _ => false,
        }
    }
}

/// Where an open variable came from, to say what needs a type hint.
#[derive(Clone)]
pub enum Origin<'db> {
    None,
    /// An expression whose own form leaves a type open.
    Expr(ExprFull<'db>),
    /// An expression of another language that leaves a type open, by an
    /// index of that language's choosing.
    Id(u32),
}

enum VarState<'db> {
    Link(VarId),
    Free { class: Class<'db>, origin: Origin<'db>, rank: u32 },
    Bound(PType<'db>),
}

/// The type variables of one synthesis, and what each has been unified with.
pub struct Unifier<'db> {
    db: &'db dyn crate::Db,
    vars: Vec<VarState<'db>>,
}

/// A variable that resolution reached with nothing to decide it.
pub struct Unresolved<'db> {
    pub origin: Origin<'db>,
}

impl<'db> Unifier<'db> {
    pub fn new(db: &'db dyn crate::Db) -> Self {
        Unifier { db, vars: Vec::new() }
    }

    pub fn fresh(&mut self, class: Class<'db>, origin: Origin<'db>) -> PType<'db> {
        let id = self.vars.len() as VarId;
        self.vars.push(VarState::Free { class, origin, rank: 0 });
        PType::Var(id)
    }

    /// The representative of a variable's set.
    fn find(&mut self, var: VarId) -> VarId {
        let mut root = var;
        while let VarState::Link(next) = self.vars[root as usize] {
            root = next;
        }
        let mut node = var;
        while let VarState::Link(next) = self.vars[node as usize] {
            self.vars[node as usize] = VarState::Link(root);
            node = next;
        }
        root
    }

    /// A type with its outermost variable, if any, replaced by what it is
    /// bound to.
    fn prune(&mut self, ty: &PType<'db>) -> PType<'db> {
        match ty {
            PType::Var(var) => {
                let root = self.find(*var);
                match &self.vars[root as usize] {
                    VarState::Bound(bound) => bound.clone(),
                    _ => PType::Var(root),
                }
            }
            _ => ty.clone(),
        }
    }

    /// Make two types one, as far as they can be.
    ///
    /// Where they cannot, the first is kept and nothing is reported; the check
    /// that follows inference reports it.
    pub fn unify(&mut self, a: &PType<'db>, b: &PType<'db>) {
        let a = self.prune(a);
        let b = self.prune(b);
        match (a, b) {
            (PType::Var(x), PType::Var(y)) => self.union(x, y),
            (PType::Var(x), other) | (other, PType::Var(x)) => self.bind(x, other),
            (PType::Known(_), PType::Known(_)) => {}
            (PType::Known(known), other) | (other, PType::Known(known)) => {
                if let Some(expanded) = expand(&known) {
                    self.unify_parts(&expanded, &other);
                }
            }
            (a, b) => self.unify_parts(&a, &b),
        }
    }

    /// Unify two types with no variable at the top, part by part.
    fn unify_parts(&mut self, a: &PType<'db>, b: &PType<'db>) {
        match (a, b) {
            (PType::AnonTuple(xs), PType::AnonTuple(ys)) => {
                for (x, y) in xs.iter().zip(ys) {
                    self.unify(x, y);
                }
            }
            (PType::AnonStruct(xs), PType::AnonStruct(ys))
            | (PType::Table(xs), PType::Table(ys)) => {
                for ((xn, x), (yn, y)) in xs.iter().zip(ys) {
                    if xn == yn {
                        self.unify(x, y);
                    }
                }
            }
            (PType::List(x), PType::List(y))
            | (PType::Set(x), PType::Set(y))
            | (PType::Option(x), PType::Option(y))
            | (PType::Result(x), PType::Result(y)) => self.unify(x, y),
            (PType::Tensor(x, xr), PType::Tensor(y, yr)) if xr == yr => self.unify(x, y),
            (PType::Map(xk, xv), PType::Map(yk, yv)) => {
                self.unify(xk, yk);
                self.unify(xv, yv);
            }
            (PType::Term(xn, x), PType::Term(yn, y)) if xn == yn => self.unify(x, y),
            (PType::Enum(xs), PType::Enum(ys)) => {
                for ((xn, x), (yn, y)) in xs.iter().zip(ys) {
                    if let (true, Some(x), Some(y)) = (xn == yn, x, y) {
                        self.unify(x, y);
                    }
                }
            }
            _ => {}
        }
    }

    fn union(&mut self, x: VarId, y: VarId) {
        if x == y {
            return;
        }
        let rank_of = |state: &VarState<'db>| match state {
            VarState::Free { rank, .. } => *rank,
            _ => unreachable!("a pruned variable is free"),
        };
        let class_of = |state: &VarState<'db>| match state {
            VarState::Free { class, .. } => class.clone(),
            _ => unreachable!("a pruned variable is free"),
        };
        // Two variables whose classes no one type could satisfy stay apart.
        // Each goes on to its own type, and the expressions that must agree
        // are reported as disagreeing where they are checked.
        let Some((class, payloads)) = merge_classes(self.db, class_of(&self.vars[x as usize]), class_of(&self.vars[y as usize])) else {
            return;
        };
        let x_rank = rank_of(&self.vars[x as usize]);
        let y_rank = rank_of(&self.vars[y as usize]);
        let root = if x_rank >= y_rank { x } else { y };
        let VarState::Free { origin: x_origin, .. } = std::mem::replace(&mut self.vars[x as usize], VarState::Link(root)) else { unreachable!() };
        let VarState::Free { origin: y_origin, .. } = std::mem::replace(&mut self.vars[y as usize], VarState::Link(root)) else { unreachable!() };
        let origin = match x_origin {
            Origin::None => y_origin,
            origin => origin,
        };
        let rank = if x_rank == y_rank { x_rank + 1 } else { x_rank.max(y_rank) };
        self.vars[root as usize] = VarState::Free { class, origin, rank };
        if let Some((a, b)) = payloads {
            self.unify(&a, &b);
        }
    }

    fn bind(&mut self, var: VarId, ty: PType<'db>) {
        // A literal does not take a type it could not have. The two
        // disagree, and the variable keeps its class: if it came first, the
        // other type is the one checking reports.
        let VarState::Free { class, .. } = &self.vars[var as usize] else { unreachable!("a pruned variable is free") };
        if !class.admits(self.db, &ty) {
            return;
        }
        let state = std::mem::replace(&mut self.vars[var as usize], VarState::Bound(ty.clone()));
        let VarState::Free { class, .. } = state else { unreachable!("a pruned variable is free") };
        // `@` converts between types with parts only by cloning, but for an
        // atom or term becoming an enum. So a value adapted to a type whose
        // parts are undecided has those parts.
        if let Class::Adapt(from) = &class {
            match (from, &ty) {
                (_, PType::Known(_)) => {}
                (Type::Atom(_), PType::Enum(_)) => {}
                (Type::Term(term), PType::Enum(variants)) => {
                    let given = variants.iter().find(|(n, _)| *n == term.name).and_then(|(_, p)| p.clone());
                    if let Some(given) = given {
                        self.unify(&PType::Known((*term.payload).clone()), &given);
                    }
                }
                _ => self.unify(&PType::Known(from.clone()), &ty),
            }
        }
        // A term literal's payload goes along with the payload the type gives
        // its variant.
        if let Class::Variant { name, payload: Some(payload) } = class {
            let given = match &ty {
                PType::Term(term_name, given) if *term_name == name => Some((**given).clone()),
                PType::Known(Type::Term(term)) if term.name == name => Some(PType::Known((*term.payload).clone())),
                PType::Known(Type::Enum(en)) => en.variants.iter()
                    .find(|v| v.name == name)
                    .and_then(|v| v.payload.as_deref())
                    .map(|p| PType::Known(p.clone())),
                PType::Enum(variants) => variants.iter()
                    .find(|(n, _)| *n == name)
                    .and_then(|(_, p)| p.clone()),
                _ => None,
            };
            if let Some(given) = given {
                self.unify(&payload, &given);
            }
        }
    }

    /// A type's outermost part, with a variable at the top replaced by what it
    /// is bound to and a known type opened up one level, for reading off what
    /// is inside: the payload of an option, a field of a struct.
    pub fn shallow(&mut self, ty: &PType<'db>) -> PType<'db> {
        match self.prune(ty) {
            PType::Known(known) => expand(&known).unwrap_or(PType::Known(known)),
            other => other,
        }
    }

    /// The type a variable-holding type comes to, with each variable nothing
    /// decided taking its default.
    pub fn resolve(&mut self, ty: &PType<'db>) -> Result<Type<'db>, Unresolved<'db>> {
        Ok(match self.prune(ty) {
            PType::Known(known) => known,
            PType::Var(var) => {
                let VarState::Free { class, origin, .. } = &self.vars[var as usize] else { unreachable!("a pruned variable is free") };
                match class.clone() {
                    Class::Literal(kind) => kind.default_type(),
                    Class::Adapt(from) => from,
                    Class::Variant { name, payload: None } => Type::Atom(TypeAtom { name }),
                    Class::Variant { name, payload: Some(payload) } => Type::Term(TypeTerm {
                        name,
                        payload: Box::new(self.resolve(&payload)?),
                    }),
                    Class::Open => return Err(Unresolved { origin: origin.clone() }),
                }
            }
            PType::AnonTuple(fields) => Type::AnonTuple(TypeAnonTuple {
                fields: fields.iter().map(|f| self.resolve(f)).collect::<Result<_, _>>()?,
            }),
            PType::AnonStruct(fields) => Type::AnonStruct(TypeAnonStruct {
                fields: self.resolve_fields(&fields)?,
            }),
            PType::Table(columns) => Type::Table(TypeTable {
                columns: self.resolve_fields(&columns)?,
            }),
            PType::List(elem) => Type::List(TypeList { element_type: Box::new(self.resolve(&elem)?) }),
            PType::Set(elem) => Type::Set(TypeSet { element_type: Box::new(self.resolve(&elem)?) }),
            PType::Map(key, value) => Type::Map(TypeMap {
                key_type: Box::new(self.resolve(&key)?),
                value_type: Box::new(self.resolve(&value)?),
            }),
            PType::Option(inner) => Type::Option(TypeOption { inner_type: Box::new(self.resolve(&inner)?) }),
            PType::Result(inner) => Type::Result(TypeResult { inner_type: Box::new(self.resolve(&inner)?) }),
            PType::Tensor(elem, rank) => Type::Tensor(TypeTensor { element_type: Box::new(self.resolve(&elem)?), rank }),
            PType::Term(name, payload) => Type::Term(TypeTerm { name, payload: Box::new(self.resolve(&payload)?) }),
            PType::Enum(variants) => Type::Enum(TypeEnum {
                variants: variants.iter()
                    .map(|(name, payload)| Ok(TypeEnumVariant {
                        name: *name,
                        payload: match payload {
                            Some(p) => Some(Box::new(self.resolve(p)?)),
                            None => None,
                        },
                    }))
                    .collect::<Result<_, _>>()?,
            }),
        })
    }

    fn resolve_fields(
        &mut self,
        fields: &[(InternedText<'db>, PType<'db>)],
    ) -> Result<Vec<TypeNamedField<'db>>, Unresolved<'db>> {
        fields.iter()
            .map(|(name, ty)| Ok(TypeNamedField { name: *name, ty: Box::new(self.resolve(ty)?) }))
            .collect()
    }

    /// The type as far as it is known, for an error message: `_` for a type
    /// nothing decides.
    pub fn describe(&mut self, db: &'db dyn crate::Db, ty: &PType<'db>) -> String {
        match self.prune(ty) {
            PType::Var(var) => match self.resolve(&PType::Var(var)) {
                Ok(resolved) => type_to_string(db, &resolved),
                Err(_) => "_".to_string(),
            },
            PType::Known(known) => type_to_string(db, &known),
            PType::AnonTuple(fields) => {
                let parts: Vec<String> = fields.iter().map(|f| self.describe(db, f)).collect();
                if parts.len() == 1 { format!("({},)", parts[0]) } else { format!("({})", parts.join(", ")) }
            }
            PType::AnonStruct(fields) => format!("{{{}}}", self.describe_fields(db, &fields)),
            PType::Table(columns) => format!("{{|{}|}}", self.describe_fields(db, &columns)),
            PType::List(elem) => format!("[{}]", self.describe(db, &elem)),
            PType::Set(elem) => format!("#{{{}}}", self.describe(db, &elem)),
            PType::Map(key, value) => format!("%{{{} = {}}}", self.describe(db, &key), self.describe(db, &value)),
            PType::Option(inner) => format!("?{}", self.describe(db, &inner)),
            PType::Result(inner) => format!("!{}", self.describe(db, &inner)),
            PType::Tensor(elem, rank) => format!("[|{}, {}|]", self.describe(db, &elem), rank),
            PType::Term(name, payload) => format!("term {} {}", name.as_str(db), self.describe(db, &payload)),
            PType::Enum(variants) => {
                let parts: Vec<String> = variants.iter()
                    .map(|(name, payload)| match payload {
                        Some(p) => format!("term {} {}", name.as_str(db), self.describe(db, p)),
                        None => format!("atom {}", name.as_str(db)),
                    })
                    .collect();
                format!("enum{{{}}}", parts.join(", "))
            }
        }
    }

    fn describe_fields(&mut self, db: &'db dyn crate::Db, fields: &[(InternedText<'db>, PType<'db>)]) -> String {
        fields.iter()
            .map(|(name, ty)| format!("{}: {}", name.as_str(db), self.describe(db, ty)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The class of two variables made one, and a pair of payloads that must
/// then be unified too, or `None` if no type is of both classes.
fn merge_classes<'db>(
    db: &'db dyn crate::Db,
    first: Class<'db>,
    second: Class<'db>,
) -> Option<(Class<'db>, Option<(PType<'db>, PType<'db>)>)> {
    Some(match (first, second) {
        (Class::Open, other) | (other, Class::Open) => (other, None),
        (Class::Literal(a), Class::Literal(b)) => match (a, b) {
            _ if a == b => (Class::Literal(a), None),
            (LiteralKind::Int, LiteralKind::Hex) | (LiteralKind::Hex, LiteralKind::Int) => {
                (Class::Literal(LiteralKind::Int), None)
            }
            // A hex literal is a float's bits as readily as an integer.
            (LiteralKind::Float, LiteralKind::Hex) | (LiteralKind::Hex, LiteralKind::Float) => {
                (Class::Literal(LiteralKind::Float), None)
            }
            _ => return None,
        },
        (
            Class::Variant { name: a_name, payload: a },
            Class::Variant { name: b_name, payload: b },
        ) => match (a_name == b_name, a, b) {
            (true, None, None) => (Class::Variant { name: a_name, payload: None }, None),
            (true, Some(a), Some(b)) => (Class::Variant { name: a_name, payload: Some(a.clone()) }, Some((*a, *b))),
            _ => return None,
        },
        // An adapted value is of the type it was taken from unless something
        // asks for another, so the two meet at that type, if the other class
        // admits it.
        (Class::Adapt(from), other) | (other, Class::Adapt(from)) => {
            if other.admits(db, &PType::Known(from.clone())) {
                (Class::Adapt(from), None)
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

/// One level of a known type as a variable-holding type, so that it can be
/// unified part by part. `None` for a type with no parts to unify.
fn expand<'db>(ty: &Type<'db>) -> Option<PType<'db>> {
    let known = |t: &Type<'db>| PType::Known(t.clone());
    let fields = |fs: &[TypeNamedField<'db>]| fs.iter().map(|f| (f.name, known(&f.ty))).collect();
    Some(match ty {
        Type::AnonTuple(t) => PType::AnonTuple(t.fields.iter().map(known).collect()),
        Type::AnonStruct(s) => PType::AnonStruct(fields(&s.fields)),
        Type::Table(t) => PType::Table(fields(&t.columns)),
        Type::List(l) => PType::List(Box::new(known(&l.element_type))),
        Type::Set(s) => PType::Set(Box::new(known(&s.element_type))),
        Type::Map(m) => PType::Map(Box::new(known(&m.key_type)), Box::new(known(&m.value_type))),
        Type::Option(o) => PType::Option(Box::new(known(&o.inner_type))),
        Type::Result(r) => PType::Result(Box::new(known(&r.inner_type))),
        Type::Tensor(t) => PType::Tensor(Box::new(known(&t.element_type)), t.rank),
        Type::Term(t) => PType::Term(t.name, Box::new(known(&t.payload))),
        Type::Enum(e) => PType::Enum(
            e.variants.iter().map(|v| (v.name, v.payload.as_deref().map(known))).collect(),
        ),
        _ => return None,
    })
}

/// Infer the type of an expression, with variables where it is not yet
/// decided.
pub fn infer<'db>(
    ctx: &TypeContext<'db>,
    uni: &mut Unifier<'db>,
    expr: &ExprFull<'db>,
) -> Result<PType<'db>, TypeError> {
    // Rule: Inf-TypedExpr - a hint is the type, and what is under it is
    // typed by checking against the hint.
    if let Some(hint) = expr.type_hint() {
        return Ok(PType::Known(ctx.convert_hint(expr, &hint)?));
    }
    infer_unhinted(ctx, uni, expr)
}

/// Infer the type of an expression from what it is, leaving aside its hint.
pub fn infer_unhinted<'db>(
    ctx: &TypeContext<'db>,
    uni: &mut Unifier<'db>,
    expr: &ExprFull<'db>,
) -> Result<PType<'db>, TypeError> {
    // A collection's shared type is open until its elements say what it is.
    // An empty one leaves it open, so it is the collection that needs a hint.
    let shared = |uni: &mut Unifier<'db>, empty: bool| {
        let origin = if empty { Origin::Expr(expr.clone()) } else { Origin::None };
        uni.fresh(Class::Open, origin)
    };
    Ok(match expr.expr() {
        // Rule: Inf-Bool, Inf-String
        Expr::True | Expr::False => PType::Known(Type::Bool),
        Expr::String(_) => PType::Known(Type::String),
        // Rule: Inf-Int, Inf-Hex, Inf-Float
        Expr::Int(_) => uni.fresh(Class::Literal(LiteralKind::Int), Origin::None),
        Expr::Hex(_) => uni.fresh(Class::Literal(LiteralKind::Hex), Origin::None),
        Expr::Float(_) => uni.fresh(Class::Literal(LiteralKind::Float), Origin::None),
        // Rule: Inf-Tuple
        Expr::AnonTuple(t) => PType::AnonTuple(
            t.elements.iter().map(|e| infer(ctx, uni, e)).collect::<Result<_, _>>()?,
        ),
        // Rule: Inf-Struct
        Expr::AnonStruct(s) => PType::AnonStruct(
            s.fields.iter().map(|f| Ok((f.name, infer(ctx, uni, &f.value)?))).collect::<Result<_, _>>()?,
        ),
        // Rule: Inf-List
        Expr::List(l) => {
            let elem = shared(uni, l.elements.is_empty());
            infer_together(ctx, uni, &elem, &l.elements)?;
            PType::List(Box::new(elem))
        }
        // Rule: Inf-Set
        Expr::Set(s) => {
            let elem = shared(uni, s.elements.is_empty());
            infer_together(ctx, uni, &elem, &s.elements)?;
            PType::Set(Box::new(elem))
        }
        // Rule: Inf-Tensor
        Expr::Tensor(t) => {
            let elem = shared(uni, t.elements.is_empty());
            infer_together(ctx, uni, &elem, &t.elements)?;
            PType::Tensor(Box::new(elem), t.shape.len() as u32)
        }
        // Rule: Inf-Map
        Expr::Map(m) => {
            let key = shared(uni, m.entries.is_empty());
            let value = shared(uni, m.entries.is_empty());
            for entry in &m.entries {
                let k = infer(ctx, uni, &entry.key)?;
                uni.unify(&key, &k);
                let v = infer(ctx, uni, &entry.value)?;
                uni.unify(&value, &v);
            }
            PType::Map(Box::new(key), Box::new(value))
        }
        // Rule: Inf-Table
        Expr::Table(t) => {
            let columns: Vec<PType<'db>> = t.header.iter().map(|_| shared(uni, t.rows.is_empty())).collect();
            for row in &t.rows {
                for (elem, column) in row.elements.iter().zip(&columns) {
                    let ty = infer(ctx, uni, elem)?;
                    uni.unify(column, &ty);
                }
            }
            PType::Table(t.header.iter().copied().zip(columns).collect())
        }
        // Rule: Inf-Some, Inf-Ok
        Expr::Some(s) => PType::Option(Box::new(infer(ctx, uni, &s.payload)?)),
        Expr::Ok(o) => PType::Result(Box::new(infer(ctx, uni, &o.payload)?)),
        // Rule: Inf-None, Inf-Er
        Expr::None | Expr::Er(_) => {
            let inner = uni.fresh(Class::Open, Origin::Expr(expr.clone()));
            match expr.expr() {
                Expr::None => PType::Option(Box::new(inner)),
                _ => PType::Result(Box::new(inner)),
            }
        }
        // Rule: Inf-Data, Inf-Error - the payload is typed on its own, when
        // the value is checked.
        Expr::Data(_) => PType::Known(Type::Data),
        Expr::Error(_) => PType::Known(Type::Error),
        Expr::Group(g) => infer(ctx, uni, &g.inner)?,
        // Rule: Inf-Atom
        Expr::Atom(a) => uni.fresh(Class::Variant { name: a.name, payload: None }, Origin::None),
        // Rule: Inf-Term
        Expr::Term(t) => {
            let payload = infer(ctx, uni, &t.payload)?;
            uni.fresh(Class::Variant { name: t.name, payload: Some(Box::new(payload)) }, Origin::None)
        }
        // Rule: Inf-Enum
        Expr::Enum(_) => uni.fresh(Class::Open, Origin::Expr(expr.clone())),
        Expr::ParseError(_) => {
            if let Some(ts) = ctx.get_span(expr) {
                DiagnosticBuilder::error(ctx.db, "cannot type-check expression with parse errors")
                    .code("T017")
                    .primary_label(ts, "parse error occurred here")
                    .note("fix the parse error before type checking")
                    .emit_type();
            }
            return Err(TypeError::CannotSynthesize);
        }
    })
}

/// Infer the types of expressions that must share one, and unify each with
/// it.
fn infer_together<'db>(
    ctx: &TypeContext<'db>,
    uni: &mut Unifier<'db>,
    shared: &PType<'db>,
    exprs: &[ExprFull<'db>],
) -> Result<(), TypeError> {
    for expr in exprs {
        let ty = infer(ctx, uni, expr)?;
        uni.unify(shared, &ty);
    }
    Ok(())
}

/// Report an expression that leaves a type open with nothing to decide it.
pub fn report_unresolved<'db>(ctx: &TypeContext<'db>, unresolved: &Unresolved<'db>) {
    let Origin::Expr(expr) = &unresolved.origin else {
        unreachable!("only a datalit expression leaves a type open in datalit")
    };
    let Some(ts) = ctx.get_span(expr) else { return };
    let msg = match expr.expr() {
        Expr::None => "cannot infer type for None value",
        Expr::Er(_) => "cannot infer type for Er value",
        Expr::Enum(_) => "cannot infer type for enum literal",
        Expr::List(_) => "cannot infer type for empty list",
        Expr::Set(_) => "cannot infer type for empty set",
        Expr::Map(_) => "cannot infer type for empty map",
        Expr::Tensor(_) => "cannot infer type for empty tensor",
        Expr::Table(_) => "cannot infer type for table with no rows",
        _ => unreachable!("no other expression leaves a type open"),
    };
    DiagnosticBuilder::error(ctx.db, msg)
        .code("T016")
        .primary_label(ts, "type annotation required")
        .note("provide a type hint to specify the expected type")
        .emit_type();
}
