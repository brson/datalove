//! Matching type parameters against the types a call site supplies.
//!
//! A generic signature is an ordinary type with `Type::Var` sitting wherever
//! the source wrote a type parameter, at any depth. Binding a call is then a
//! walk down two types in step: the parameter's and the argument's. Where the
//! parameter has a `Var`, whatever the argument has there is what that
//! parameter stands for; everywhere else the two shapes are simply compared,
//! and disagreements are left for the caller's ordinary type check to report.

use std::collections::HashMap;

use bct::text::InternedText;
use datalove_datalit as datalit;
use datalit::tycheck::Type;

use salsa::Database as Db;

/// What the type parameters of one call were bound to.
pub type TypeParamBindings<'db> = HashMap<InternedText<'db>, Type<'db>>;

/// A type parameter that two arguments wanted to be two different types.
///
/// `swap(a, b)` over an `int` and a `string` produces one of these rather than
/// silently reinterpreting the second argument as the first one's type.
pub struct TypeParamConflict<'db> {
    pub name: InternedText<'db>,
    pub bound: Type<'db>,
    pub found: Type<'db>,
}

/// Bind the type parameters in `param` from the corresponding parts of `arg`.
///
/// Shape disagreements are not errors here. A call that passes a `{int: int}`
/// where the signature wants a `[T]` binds nothing and is reported by the
/// argument check that follows, which knows the expression to blame.
pub fn bind_type_params<'db>(
    db: &'db dyn Db,
    param: &Type<'db>,
    arg: &Type<'db>,
    bindings: &mut TypeParamBindings<'db>,
) -> Result<(), TypeParamConflict<'db>> {
    match (param, arg) {
        (Type::Var(name), found) => {
            match bindings.get(name) {
                Some(bound) if !datalit::tycheck::types_equivalent(db, bound, found) => {
                    Err(TypeParamConflict {
                        name: *name,
                        bound: bound.clone(),
                        found: found.clone(),
                    })
                }
                Some(_) => Ok(()),
                None => {
                    bindings.insert(*name, found.clone());
                    Ok(())
                }
            }
        }

        (Type::List(p), Type::List(a)) => {
            bind_type_params(db, &p.element_type, &a.element_type, bindings)
        }
        (Type::Set(p), Type::Set(a)) => {
            bind_type_params(db, &p.element_type, &a.element_type, bindings)
        }
        (Type::Option(p), Type::Option(a)) => {
            bind_type_params(db, &p.inner_type, &a.inner_type, bindings)
        }
        (Type::Result(p), Type::Result(a)) => {
            bind_type_params(db, &p.inner_type, &a.inner_type, bindings)
        }
        (Type::Tensor(p), Type::Tensor(a)) if p.rank == a.rank => {
            bind_type_params(db, &p.element_type, &a.element_type, bindings)
        }
        (Type::Term(p), Type::Term(a)) if p.name == a.name => {
            bind_type_params(db, &p.payload, &a.payload, bindings)
        }

        (Type::Map(p), Type::Map(a)) => {
            bind_type_params(db, &p.key_type, &a.key_type, bindings)?;
            bind_type_params(db, &p.value_type, &a.value_type, bindings)
        }

        (Type::AnonTuple(p), Type::AnonTuple(a)) => {
            for (pf, af) in p.fields.iter().zip(a.fields.iter()) {
                bind_type_params(db, pf, af, bindings)?;
            }
            Ok(())
        }

        (Type::AnonStruct(p), Type::AnonStruct(a)) => {
            for (pf, af) in p.fields.iter().zip(a.fields.iter()) {
                if pf.name != af.name {
                    return Ok(());
                }
                bind_type_params(db, &pf.ty, &af.ty, bindings)?;
            }
            Ok(())
        }

        (Type::Table(p), Type::Table(a)) => {
            for (pc, ac) in p.columns.iter().zip(a.columns.iter()) {
                if pc.name != ac.name {
                    return Ok(());
                }
                bind_type_params(db, &pc.ty, &ac.ty, bindings)?;
            }
            Ok(())
        }

        (Type::Enum(p), Type::Enum(a)) => {
            for (pv, av) in p.variants.iter().zip(a.variants.iter()) {
                if pv.name != av.name {
                    return Ok(());
                }
                if let (Some(pp), Some(ap)) = (&pv.payload, &av.payload) {
                    bind_type_params(db, pp, ap, bindings)?;
                }
            }
            Ok(())
        }

        // Either the parameter holds no type parameter here, or the two shapes
        // disagree. Both are for the argument check to speak to.
        _ => Ok(()),
    }
}

/// Replace every bound type parameter in `ty` with what it was bound to.
///
/// A parameter the call site never pinned down is left alone, so it reaches
/// the argument check still written as `T` and is reported that way.
pub fn substitute_type_params<'db>(
    ty: &Type<'db>,
    bindings: &TypeParamBindings<'db>,
) -> Type<'db> {
    let sub = |t: &Type<'db>| Box::new(substitute_type_params(t, bindings));

    match ty {
        Type::Var(name) => bindings.get(name).cloned().unwrap_or_else(|| ty.clone()),

        Type::List(t) => Type::List(datalit::tycheck::TypeList {
            element_type: sub(&t.element_type),
        }),
        Type::Set(t) => Type::Set(datalit::tycheck::TypeSet {
            element_type: sub(&t.element_type),
        }),
        Type::Option(t) => Type::Option(datalit::tycheck::TypeOption {
            inner_type: sub(&t.inner_type),
        }),
        Type::Result(t) => Type::Result(datalit::tycheck::TypeResult {
            inner_type: sub(&t.inner_type),
        }),
        Type::Tensor(t) => Type::Tensor(datalit::tycheck::TypeTensor {
            element_type: sub(&t.element_type),
            rank: t.rank,
        }),
        Type::Term(t) => Type::Term(datalit::tycheck::TypeTerm {
            name: t.name,
            payload: sub(&t.payload),
        }),
        Type::Map(t) => Type::Map(datalit::tycheck::TypeMap {
            key_type: sub(&t.key_type),
            value_type: sub(&t.value_type),
        }),

        Type::AnonTuple(t) => Type::AnonTuple(datalit::tycheck::TypeAnonTuple {
            fields: t.fields.iter().map(|f| substitute_type_params(f, bindings)).collect(),
        }),
        Type::AnonStruct(t) => Type::AnonStruct(datalit::tycheck::TypeAnonStruct {
            fields: t.fields.iter()
                .map(|f| datalit::tycheck::TypeNamedField { name: f.name, ty: sub(&f.ty) })
                .collect(),
        }),
        Type::Table(t) => Type::Table(datalit::tycheck::TypeTable {
            columns: t.columns.iter()
                .map(|c| datalit::tycheck::TypeNamedField { name: c.name, ty: sub(&c.ty) })
                .collect(),
        }),
        Type::Enum(t) => Type::Enum(datalit::tycheck::TypeEnum {
            variants: t.variants.iter()
                .map(|v| datalit::tycheck::TypeEnumVariant {
                    name: v.name,
                    payload: v.payload.as_ref().map(|p| sub(p)),
                })
                .collect(),
        }),

        Type::Bool | Type::U8 | Type::I8 | Type::U16 | Type::I16
        | Type::U32 | Type::I32 | Type::U64 | Type::I64
        | Type::Index | Type::Offset | Type::F32 | Type::F64
        | Type::Int | Type::String | Type::Data | Type::Error
        | Type::Atom(_) => ty.clone(),
    }
}

/// Whether a type parameter appears anywhere in a type.
///
/// A signature holding one of these is generic at that position, so the call
/// site has to erase into it rather than pass the value along unchanged.
pub fn contains_type_param(ty: &Type<'_>) -> bool {
    match ty {
        Type::Var(_) => true,

        Type::List(t) => contains_type_param(&t.element_type),
        Type::Set(t) => contains_type_param(&t.element_type),
        Type::Option(t) => contains_type_param(&t.inner_type),
        Type::Result(t) => contains_type_param(&t.inner_type),
        Type::Tensor(t) => contains_type_param(&t.element_type),
        Type::Term(t) => contains_type_param(&t.payload),
        Type::Map(t) => {
            contains_type_param(&t.key_type) || contains_type_param(&t.value_type)
        }

        Type::AnonTuple(t) => t.fields.iter().any(contains_type_param),
        Type::AnonStruct(t) => t.fields.iter().any(|f| contains_type_param(&f.ty)),
        Type::Table(t) => t.columns.iter().any(|c| contains_type_param(&c.ty)),
        Type::Enum(t) => t.variants.iter().any(|v| {
            v.payload.as_ref().is_some_and(|p| contains_type_param(p))
        }),

        Type::Bool | Type::U8 | Type::I8 | Type::U16 | Type::I16
        | Type::U32 | Type::I32 | Type::U64 | Type::I64
        | Type::Index | Type::Offset | Type::F32 | Type::F64
        | Type::Int | Type::String | Type::Data | Type::Error
        | Type::Atom(_) => false,
    }
}

/// The first type parameter sitting somewhere erasure cannot convert.
///
/// Erasing a value into the shape a generic callee was compiled for converts
/// it in place, and that only works where the conversion can reach the
/// parameter's position. An option or a result holds its payload inline, so
/// converting one means converting the payload and writing it at the other
/// side's offset. A collection holds its elements packed by size, so `[T]` and
/// `[data]` do not agree on where any element after the first begins, and
/// converting between them would mean rebuilding the collection element by
/// element. Until that is worth its cost, a type parameter under one is
/// refused rather than reinterpreted.
pub fn first_unerasable_type_param<'db>(ty: &Type<'db>) -> Option<InternedText<'db>> {
    match ty {
        // Erasure converts the payload of these and writes it where the other
        // side keeps it, so a parameter below one is still reachable.
        Type::Option(t) => first_unerasable_type_param(&t.inner_type),
        Type::Result(t) => first_unerasable_type_param(&t.inner_type),

        // A parameter standing alone is the position erasure is built for.
        Type::Var(_) => None,

        other => first_type_param(other),
    }
}

/// The first type parameter appearing anywhere in a type.
fn first_type_param<'db>(ty: &Type<'db>) -> Option<InternedText<'db>> {
    match ty {
        Type::Var(name) => Some(*name),

        Type::List(t) => first_type_param(&t.element_type),
        Type::Set(t) => first_type_param(&t.element_type),
        Type::Option(t) => first_type_param(&t.inner_type),
        Type::Result(t) => first_type_param(&t.inner_type),
        Type::Tensor(t) => first_type_param(&t.element_type),
        Type::Term(t) => first_type_param(&t.payload),
        Type::Map(t) => {
            first_type_param(&t.key_type).or_else(|| first_type_param(&t.value_type))
        }

        Type::AnonTuple(t) => t.fields.iter().find_map(first_type_param),
        Type::AnonStruct(t) => t.fields.iter().find_map(|f| first_type_param(&f.ty)),
        Type::Table(t) => t.columns.iter().find_map(|c| first_type_param(&c.ty)),
        Type::Enum(t) => t.variants.iter()
            .find_map(|v| v.payload.as_ref().and_then(|p| first_type_param(p))),

        Type::Bool | Type::U8 | Type::I8 | Type::U16 | Type::I16
        | Type::U32 | Type::I32 | Type::U64 | Type::I64
        | Type::Index | Type::Offset | Type::F32 | Type::F64
        | Type::Int | Type::String | Type::Data | Type::Error
        | Type::Atom(_) => None,
    }
}
