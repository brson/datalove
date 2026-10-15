//! Inference of the types of expressions that must agree.
//!
//! Datafun extends datalit's inference ([`datalit::tycheck::infer`]) to its
//! own expressions: the two operands of an operator are typed together, as a
//! list's elements are, and a generic call's type parameters are variables
//! unified with its arguments and with what is expected of its result. So in
//! `x == 0` with `x: u32` the `0` is a `u32`, and in `pick_first(99, : u32 / 1)`
//! the `T` is `u32`.
//!
//! An expression is inferred once, the first time anything asks for its type,
//! and what was inferred for it and everything inside it is kept. Checking
//! then reads those types instead of working them out again, so a statement
//! is inferred in one pass however deeply its calls and operators nest.
//!
//! Inference reports nothing. It reads what it needs leniently, and where it
//! cannot tell a type it leaves a variable no one decides; checking, which
//! comes after, is what reports what is wrong.

use rmx::prelude::*;
use rmx::std::collections::HashMap;
use bct::text::InternedText;
use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use datalit::tycheck::Type as DT;
use datalit::tycheck::infer::{Class, LiteralKind, Origin, PType, Unifier, Unresolved};

use datalove_datafun_common::generics::contains_type_param;

use crate::context::TypeContext;
use crate::types::*;

/// The inference state of a typechecking context.
pub struct Inference<'db> {
    pub(crate) uni: Unifier<'db>,
    /// What each expression inferred so far was inferred to be.
    types: HashMap<ExprKey<'db>, PType<'db>>,
    /// The variable standing for each type parameter of a generic call.
    call_params: HashMap<ExprKey<'db>, Vec<(InternedText<'db>, PType<'db>)>>,
    /// The expressions that leave a type open, by the index their variables
    /// carry as their origin.
    open_exprs: Vec<ExprFun<'db>>,
}

impl<'db> Inference<'db> {
    pub fn new(db: &'db dyn crate::Db) -> Self {
        Inference {
            uni: Unifier::new(db),
            types: HashMap::new(),
            call_params: HashMap::new(),
            open_exprs: Vec::new(),
        }
    }
}

/// Where an expression's type could not be decided.
pub enum Undecided<'db> {
    /// An expression that leaves a type open, `none` or `[]`, with nothing to
    /// decide it.
    Open(ExprFun<'db>),
    /// Something inference could not read, which checking will report.
    Unknown,
}

/// The type inferred for an expression, inferring it now if nothing has yet.
pub fn infer<'db>(ctx: &mut TypeContext<'db>, expr: ExprFun<'db>) -> PType<'db> {
    let key = expr.key();
    if let Some(ty) = ctx.inference.types.get(&key) {
        return ty.clone();
    }
    // Inference reads names and hints the way checking does, and the lookups
    // report what they do not find. Checking will report it again where it
    // belongs, so nothing said here is kept.
    let errors = ctx.errors.len();
    let diagnostics = ctx.pending_diagnostics.len();
    let ty = infer_expr(ctx, expr);
    ctx.errors.truncate(errors);
    ctx.pending_diagnostics.truncate(diagnostics);
    ty
}

/// Unify an expression's inferred type with a known type.
pub fn expect<'db>(ctx: &mut TypeContext<'db>, expr: ExprFun<'db>, expected: &Type<'db>) {
    let Type::Datalit(expected) = expected else { return };
    let inferred = infer(ctx, expr);
    ctx.inference.uni.unify(&inferred, &PType::Known(expected.C()));
}

/// The type an expression comes to, with defaults for whatever nothing
/// decided.
pub fn resolve<'db>(ctx: &mut TypeContext<'db>, expr: ExprFun<'db>) -> Result<Type<'db>, Undecided<'db>> {
    let inferred = infer(ctx, expr);
    resolve_ptype(ctx, &inferred)
}

fn resolve_ptype<'db>(ctx: &mut TypeContext<'db>, ty: &PType<'db>) -> Result<Type<'db>, Undecided<'db>> {
    ctx.inference.uni.resolve(ty).map(Type::Datalit).map_err(|Unresolved { origin }| match origin {
        Origin::Id(id) => Undecided::Open(ctx.inference.open_exprs[id as usize]),
        Origin::None => Undecided::Unknown,
        Origin::Expr(_) => unreachable!("datafun inference names its open expressions by id"),
    })
}

/// The type an expression's form gives it, leaving aside the hint written on
/// it, as far as it can be told: for the message of a mismatch.
pub fn describe_unhinted<'db>(ctx: &mut TypeContext<'db>, expr: ExprFun<'db>) -> String {
    let errors = ctx.errors.len();
    let diagnostics = ctx.pending_diagnostics.len();
    let ty = infer_unhinted(ctx, expr);
    ctx.errors.truncate(errors);
    ctx.pending_diagnostics.truncate(diagnostics);
    let db = ctx.db;
    ctx.inference.uni.describe(db, &ty)
}

/// What each of a generic call's type parameters was inferred to be, or why
/// nothing decided it.
pub fn call_type_args<'db>(
    ctx: &mut TypeContext<'db>,
    call_expr: ExprFun<'db>,
) -> Vec<(InternedText<'db>, Result<DT<'db>, Undecided<'db>>)> {
    infer(ctx, call_expr);
    let key = call_expr.key();
    let params = ctx.inference.call_params.get(&key).cloned().unwrap_or_default();
    params.into_iter()
        .map(|(name, var)| {
            let resolved = resolve_ptype(ctx, &var).map(|ty| match ty {
                Type::Datalit(ty) => ty,
                Type::Function(_) => unreachable!("inference resolves to data types"),
            });
            (name, resolved)
        })
        .collect()
}

/// A variable for a type an expression leaves open, which names the
/// expression if nothing decides it.
fn open<'db>(ctx: &mut TypeContext<'db>, expr: ExprFun<'db>) -> PType<'db> {
    let id = ctx.inference.open_exprs.len() as u32;
    ctx.inference.open_exprs.push(expr);
    ctx.inference.uni.fresh(Class::Open, Origin::Id(id))
}

fn unknown<'db>(ctx: &mut TypeContext<'db>) -> PType<'db> {
    ctx.inference.uni.fresh(Class::Open, Origin::None)
}

fn infer_expr<'db>(ctx: &mut TypeContext<'db>, expr: ExprFun<'db>) -> PType<'db> {
    let ty = match expr.kind().type_hint().cloned() {
        Some(hint) => match ctx.convert_hint(hint) {
            Ok(Type::Datalit(hinted)) => {
                // The parts are still inferred, so that what is inside a hinted
                // expression is typed once like anything else.
                let inner = infer_unhinted(ctx, expr);
                let known = PType::Known(hinted);
                ctx.inference.uni.unify(&known, &inner);
                known
            }
            _ => unknown(ctx),
        },
        None => infer_unhinted(ctx, expr),
    };
    ctx.inference.types.insert(expr.key(), ty.clone());
    ty
}

fn infer_unhinted<'db>(ctx: &mut TypeContext<'db>, expr: ExprFun<'db>) -> PType<'db> {
    let db = ctx.db;
    let lit = |ctx: &mut TypeContext<'db>, kind| ctx.inference.uni.fresh(Class::Literal(kind), Origin::None);
    let at = |id: ExprId| expr.at(id);
    match expr.kind() {
        ExprFunKind::True(_) | ExprFunKind::False(_) => PType::Known(DT::Bool),
        ExprFunKind::String(_) => PType::Known(DT::String),
        ExprFunKind::Int(_) => lit(ctx, LiteralKind::Int),
        ExprFunKind::Hex(_) => lit(ctx, LiteralKind::Hex),
        ExprFunKind::Float(_) => lit(ctx, LiteralKind::Float),

        ExprFunKind::Tuple(ExprTuple { elements }) | ExprFunKind::AnonTuple(ExprAnonTuple { elements, .. }) => {
            PType::AnonTuple(elements.iter().map(|e| infer(ctx, at(*e))).collect())
        }
        ExprFunKind::AnonStruct(s) => {
            PType::AnonStruct(s.fields.iter().map(|f| (f.name, infer(ctx, at(f.value)))).collect())
        }
        ExprFunKind::List(l) => PType::List(Box::new(infer_together(ctx, expr, &l.elements))),
        ExprFunKind::Set(s) => PType::Set(Box::new(infer_together(ctx, expr, &s.elements))),
        ExprFunKind::Tensor(t) => {
            PType::Tensor(Box::new(infer_together(ctx, expr, &t.elements)), t.shape.len() as u32)
        }
        ExprFunKind::Map(m) => {
            let keys: Vec<ExprId> = m.entries.iter().map(|e| e.key).collect();
            let values: Vec<ExprId> = m.entries.iter().map(|e| e.value).collect();
            let key = infer_together(ctx, expr, &keys);
            let value = infer_together(ctx, expr, &values);
            PType::Map(Box::new(key), Box::new(value))
        }
        ExprFunKind::Table(t) => {
            let columns = (0..t.header.len())
                .map(|i| {
                    let column: Vec<ExprId> = t.rows.iter()
                        .filter_map(|row| row.elements.get(i).copied())
                        .collect();
                    infer_together(ctx, expr, &column)
                })
                .collect::<Vec<_>>();
            PType::Table(t.header.iter().copied().zip(columns).collect())
        }

        ExprFunKind::Some(s) => PType::Option(Box::new(infer(ctx, at(s.payload)))),
        ExprFunKind::Ok(o) => PType::Result(Box::new(infer(ctx, at(o.payload)))),
        ExprFunKind::None(_) => PType::Option(Box::new(open(ctx, expr))),
        ExprFunKind::Er(_) => PType::Result(Box::new(open(ctx, expr))),
        // A `data` or `error` payload is typed on its own, when the value is
        // checked.
        ExprFunKind::Data(_) => PType::Known(DT::Data),
        ExprFunKind::Error(_) => PType::Known(DT::Error),
        ExprFunKind::Atom(a) => ctx.inference.uni.fresh(Class::Variant { name: a.name, payload: None }, Origin::None),
        ExprFunKind::Term(t) => {
            let payload = infer(ctx, at(t.payload));
            ctx.inference.uni.fresh(Class::Variant { name: t.name, payload: Some(Box::new(payload)) }, Origin::None)
        }
        ExprFunKind::EnumLiteral(_) => open(ctx, expr),
        ExprFunKind::Hinted(h) => infer(ctx, at(h.inner)),

        ExprFunKind::BinOp(b) => {
            let lhs = infer(ctx, at(b.lhs));
            let rhs = infer(ctx, at(b.rhs));
            ctx.inference.uni.unify(&lhs, &rhs);
            if yields_operand_type(b.op) { lhs } else { PType::Known(DT::Bool) }
        }
        ExprFunKind::UnaryOp(u) => match u.op {
            UnaryOp::Not => PType::Known(DT::Bool),
            UnaryOp::Neg | UnaryOp::NegOptional | UnaryOp::NegResult => infer(ctx, at(u.operand)),
        },
        // With nothing to say what it converts to, `@` clones, and keeps the
        // type it was taken from. Anything that type adapts to may ask for it.
        ExprFunKind::CloneCoerce(c) => {
            let operand = infer(ctx, at(c.operand));
            match ctx.inference.uni.resolve(&operand) {
                Ok(from) => ctx.inference.uni.fresh(Class::Adapt(from), Origin::None),
                Err(_) => operand,
            }
        }
        ExprFunKind::TryOption(t) => match at(t.operand).kind() {
            ExprFunKind::Index(index) => infer_index(ctx, at(index.base)),
            _ => {
                let operand = infer(ctx, at(t.operand));
                match ctx.inference.uni.shallow(&operand) {
                    PType::Option(inner) => *inner,
                    _ => unknown(ctx),
                }
            }
        },
        ExprFunKind::TryResult(t) => match at(t.operand).kind() {
            ExprFunKind::Index(index) => infer_index(ctx, at(index.base)),
            _ => {
                let operand = infer(ctx, at(t.operand));
                match ctx.inference.uni.shallow(&operand) {
                    PType::Result(inner) => *inner,
                    _ => unknown(ctx),
                }
            }
        },
        ExprFunKind::FieldProj(p) => {
            let base = infer(ctx, at(p.base));
            infer_field(ctx, &base, &p.field)
        }
        ExprFunKind::Place(place) => {
            let Some(Type::Datalit(root)) = ctx.lookup_variable(place.root) else { return unknown(ctx) };
            let mut ty = PType::Known(root);
            for step in &place.steps {
                ty = match step {
                    PlaceStep::Field(field) => infer_field(ctx, &ty, field),
                    PlaceStep::Index(_) => element_of(ctx, &ty),
                };
            }
            ty
        }
        ExprFunKind::FunctionCall(call) => infer_call(ctx, expr, call),
        ExprFunKind::IntrinsicCall(icall) => {
            match datalove_datafun_intrinsics::lookup_intrinsic(icall.name.as_str(db)) {
                Some((_, def)) => match crate::synthesize::intrinsic_type_to_datafun(db, def.ret) {
                    Type::Datalit(ret) => PType::Known(ret),
                    Type::Function(_) => unreachable!("an intrinsic returns data"),
                },
                None => unknown(ctx),
            }
        }
        // A data file is typed by the path that reads it, against what is
        // expected of it.
        ExprFunKind::DataFile(_) | ExprFunKind::Index(_) | ExprFunKind::ParseError(_) => unknown(ctx),
    }
}

/// Whether an operator's result has its operands' type, rather than `bool`.
pub(crate) fn yields_operand_type(op: BinOp) -> bool {
    use BinOp::*;
    match op {
        Add | Sub | Mul | Div
        | AddChecked | SubChecked | MulChecked | DivChecked
        | AddOptional | SubOptional | MulOptional | DivOptional => true,
        Lt | Gt | Le | Ge | Eq | Ne | And | Or | Xor => false,
    }
}

/// Infer the types of expressions that must share one, and that type.
///
/// With none of them, the type is open, and it is the collection holding them
/// that needs a hint.
fn infer_together<'db>(ctx: &mut TypeContext<'db>, holder: ExprFun<'db>, exprs: &[ExprId]) -> PType<'db> {
    let shared = if exprs.is_empty() { open(ctx, holder) } else { unknown(ctx) };
    for expr in exprs {
        let ty = infer(ctx, holder.at(*expr));
        ctx.inference.uni.unify(&shared, &ty);
    }
    shared
}

fn infer_field<'db>(ctx: &mut TypeContext<'db>, base: &PType<'db>, field: &FieldSelector<'db>) -> PType<'db> {
    let db = ctx.db;
    match (ctx.inference.uni.shallow(base), field) {
        (PType::Known(known), _) => {
            match crate::synthesize::resolve_field_type(db, &Type::Datalit(known), field) {
                Ok(Type::Datalit(found)) => PType::Known(found),
                _ => unknown(ctx),
            }
        }
        (PType::AnonTuple(fields), FieldSelector::Index(i)) => {
            fields.get(*i as usize).cloned().unwrap_or_else(|| unknown(ctx))
        }
        (PType::AnonStruct(fields), FieldSelector::Name(name)) => {
            let name = name.text(db);
            fields.iter().find(|(n, _)| n.text(db) == name).map(|(_, t)| t.clone())
                .unwrap_or_else(|| unknown(ctx))
        }
        _ => unknown(ctx),
    }
}

/// The type of an element of a list, map or tensor.
fn element_of<'db>(ctx: &mut TypeContext<'db>, base: &PType<'db>) -> PType<'db> {
    match ctx.inference.uni.shallow(base) {
        PType::List(elem) => *elem,
        PType::Map(_, value) => *value,
        PType::Tensor(elem, 1) => *elem,
        PType::Tensor(elem, rank) => PType::Tensor(elem, rank - 1),
        _ => unknown(ctx),
    }
}

fn infer_index<'db>(ctx: &mut TypeContext<'db>, base: ExprFun<'db>) -> PType<'db> {
    let base = infer(ctx, base);
    element_of(ctx, &base)
}

/// A call's result, with its type parameters as variables unified with its
/// arguments.
fn infer_call<'db>(ctx: &mut TypeContext<'db>, expr: ExprFun<'db>, call: &ExprFunctionCall<'db>) -> PType<'db> {
    let db = ctx.db;
    let name = call.name;
    let found = match call.qualifier {
        Some(alias) => ctx.qualified.functions.iter()
            .find(|f| f.alias == alias && f.name == name)
            .map(|f| (f.func, Some(f.ast.clone()))),
        None => ctx.lookup_function(name).map(|func| (func, ctx.lookup_function_ast(name).map(|(ast, _)| ast))),
    };
    let args: Vec<ExprFun<'db>> = call.args.iter().map(|&arg| expr.at(arg)).collect();
    let Some((func, ast)) = found else {
        for arg in &args {
            infer(ctx, *arg);
        }
        return unknown(ctx);
    };
    let params: Vec<(InternedText<'db>, PType<'db>)> = ast
        .map(|ast| ast.type_params().iter().map(|p| (*p, unknown(ctx))).collect())
        .unwrap_or_default();
    for (arg, param_ty) in args.iter().zip(func.param_types(db).iter()) {
        let arg_ty = infer(ctx, *arg);
        if let Type::Datalit(param_ty) = param_ty {
            let param_ty = instantiate(param_ty, &params);
            ctx.inference.uni.unify(&param_ty, &arg_ty);
        }
    }
    for arg in args.iter().skip(func.param_types(db).len()) {
        infer(ctx, *arg);
    }
    let ret = match func.return_type(db) {
        Type::Datalit(ret) => instantiate(&ret, &params),
        Type::Function(_) => unknown(ctx),
    };
    ctx.inference.call_params.insert(expr.key(), params);
    ret
}

/// A type from a signature, with its type parameters replaced by the
/// variables standing for them at a call.
fn instantiate<'db>(ty: &DT<'db>, params: &[(InternedText<'db>, PType<'db>)]) -> PType<'db> {
    if params.is_empty() || !contains_type_param(ty) {
        return PType::Known(ty.C());
    }
    let fields = |fs: &[datalit::tycheck::TypeNamedField<'db>]| {
        fs.iter().map(|f| (f.name, instantiate(&f.ty, params))).collect()
    };
    match ty {
        DT::Var(name) => match params.iter().find(|(p, _)| p == name) {
            Some((_, var)) => var.clone(),
            None => PType::Known(ty.C()),
        },
        DT::AnonTuple(t) => PType::AnonTuple(t.fields.iter().map(|f| instantiate(f, params)).collect()),
        DT::AnonStruct(s) => PType::AnonStruct(fields(&s.fields)),
        DT::Table(t) => PType::Table(fields(&t.columns)),
        DT::List(l) => PType::List(Box::new(instantiate(&l.element_type, params))),
        DT::Set(s) => PType::Set(Box::new(instantiate(&s.element_type, params))),
        DT::Map(m) => PType::Map(
            Box::new(instantiate(&m.key_type, params)),
            Box::new(instantiate(&m.value_type, params)),
        ),
        DT::Option(o) => PType::Option(Box::new(instantiate(&o.inner_type, params))),
        DT::Result(r) => PType::Result(Box::new(instantiate(&r.inner_type, params))),
        DT::Tensor(t) => PType::Tensor(Box::new(instantiate(&t.element_type, params)), t.rank),
        DT::Term(t) => PType::Term(t.name, Box::new(instantiate(&t.payload, params))),
        DT::Enum(e) => PType::Enum(
            e.variants.iter().map(|v| (v.name, v.payload.as_deref().map(|p| instantiate(p, params)))).collect(),
        ),
        _ => PType::Known(ty.C()),
    }
}
