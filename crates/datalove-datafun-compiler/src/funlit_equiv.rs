//! Funlit equivalence testing infrastructure.
//!
//! Provides conversions and utilities for comparing datafun and datalit AST and typecheck results.
//! Used to verify that datafun's inline literal parsing produces equivalent results to datalit.

use rmx::prelude::*;

use datalove_datafun_ast::ast;
use datalove_datalit as datalit;

/// Error during conversion from datafun to datalit serde format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionError {
    /// Expression contains datafun-only constructs (Name, BinOp, etc.)
    NotPureDatalit(String),
    /// Script doesn't contain exactly one ret statement.
    InvalidScriptStructure(String),
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversionError::NotPureDatalit(msg) => write!(f, "not pure datalit: {}", msg),
            ConversionError::InvalidScriptStructure(msg) => write!(f, "invalid script structure: {}", msg),
        }
    }
}

impl std::error::Error for ConversionError {}

/// Convert a datafun ExprFun to datalit::ast_serde::ExprFull.
///
/// Only works for pure datalit expressions. Returns error for datafun-only constructs.
pub fn datafun_expr_to_datalit_serde<'db>(
    db: &'db dyn salsa::Database,
    expr: ast::ExprFun<'db>,
) -> Result<datalit::ast_serde::ExprFull, ConversionError> {
    let kind = expr.expr(db);

    // Get type_hint and expr_serde from the expression kind.
    let (type_hint, expr_serde) = match kind {
        ast::ExprFunKind::True(e) => (
            e.type_hint,
            datalit::ast_serde::Expr::True,
        ),
        ast::ExprFunKind::False(e) => (
            e.type_hint,
            datalit::ast_serde::Expr::False,
        ),
        ast::ExprFunKind::None(e) => (
            e.type_hint,
            datalit::ast_serde::Expr::None,
        ),
        ast::ExprFunKind::Int(e) => (
            e.type_hint,
            datalit::ast_serde::Expr::Int(datalit::ast_serde::ExprInt {
                value: e.value.as_str(db).S(),
            }),
        ),
        ast::ExprFunKind::Float(e) => (
            e.type_hint,
            datalit::ast_serde::Expr::Float(datalit::ast_serde::ExprFloat {
                value: e.value.as_str(db).S(),
            }),
        ),
        ast::ExprFunKind::Hex(e) => (
            e.type_hint,
            datalit::ast_serde::Expr::Hex(datalit::ast_serde::ExprHex {
                value: e.value.as_str(db).S(),
            }),
        ),
        ast::ExprFunKind::String(e) => (
            e.type_hint,
            datalit::ast_serde::Expr::String(datalit::ast_serde::ExprString {
                value: e.value.as_str(db).S(),
            }),
        ),
        ast::ExprFunKind::List(e) => {
            let elements = e.elements.iter()
                .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                .collect::<Result<Vec<_>, _>>()?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::List(datalit::ast_serde::ExprList { elements }),
            )
        }
        ast::ExprFunKind::Set(e) => {
            let elements = e.elements.iter()
                .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                .collect::<Result<Vec<_>, _>>()?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Set(datalit::ast_serde::ExprSet { elements }),
            )
        }
        ast::ExprFunKind::Map(e) => {
            let entries = e.entries.iter()
                .map(|entry| {
                    let key = datafun_expr_to_datalit_serde(db, entry.key)?;
                    let value = datafun_expr_to_datalit_serde(db, entry.value)?;
                    Ok(datalit::ast_serde::ExprMapEntry { key, value })
                })
                .collect::<Result<Vec<_>, ConversionError>>()?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Map(datalit::ast_serde::ExprMap { entries }),
            )
        }
        ast::ExprFunKind::Tensor(e) => {
            let elements = e.elements.iter()
                .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                .collect::<Result<Vec<_>, _>>()?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Tensor(datalit::ast_serde::ExprTensor {
                    shape: e.shape.C(),
                    elements,
                    header: e.header,
                }),
            )
        }
        ast::ExprFunKind::AnonTuple(e) => {
            let elements = e.elements.iter()
                .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                .collect::<Result<Vec<_>, _>>()?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::AnonTuple(datalit::ast_serde::ExprAnonTuple { elements }),
            )
        }
        ast::ExprFunKind::AnonStruct(e) => {
            let fields = e.fields.iter()
                .map(|field| {
                    let value = datafun_expr_to_datalit_serde(db, field.value)?;
                    Ok(datalit::ast_serde::ExprStructField {
                        name: field.name.as_str(db).S(),
                        value,
                    })
                })
                .collect::<Result<Vec<_>, ConversionError>>()?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::AnonStruct(datalit::ast_serde::ExprAnonStruct { fields }),
            )
        }

        ast::ExprFunKind::Some(e) => {
            let payload = datafun_expr_to_datalit_serde(db, e.payload)?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Some(datalit::ast_serde::ExprSome {
                    payload: Box::new(payload),
                }),
            )
        }
        ast::ExprFunKind::Ok(e) => {
            let payload = datafun_expr_to_datalit_serde(db, e.payload)?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Ok(datalit::ast_serde::ExprOk {
                    payload: Box::new(payload),
                }),
            )
        }
        ast::ExprFunKind::Er(e) => {
            let payload = datafun_expr_to_datalit_serde(db, e.payload)?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Er(datalit::ast_serde::ExprEr {
                    payload: Box::new(payload),
                }),
            )
        }
        ast::ExprFunKind::Data(e) => {
            let value = datafun_expr_to_datalit_serde(db, e.value)?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Data(datalit::ast_serde::ExprData {
                    value: Box::new(value),
                }),
            )
        }
        ast::ExprFunKind::Error(e) => {
            let value = datafun_expr_to_datalit_serde(db, e.value)?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Error(datalit::ast_serde::ExprError {
                    value: Box::new(value),
                }),
            )
        }
        ast::ExprFunKind::Atom(e) => (
            e.type_hint,
            datalit::ast_serde::Expr::Atom(datalit::ast_serde::ExprAtom {
                name: e.name.as_str(db).S(),
            }),
        ),
        ast::ExprFunKind::Term(e) => {
            let payload = datafun_expr_to_datalit_serde(db, e.payload)?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Term(datalit::ast_serde::ExprTerm {
                    name: e.name.as_str(db).S(),
                    payload: Box::new(payload),
                }),
            )
        }
        ast::ExprFunKind::EnumLiteral(e) => {
            let variant = datafun_expr_to_datalit_serde(db, e.variant)?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Enum(datalit::ast_serde::ExprEnum {
                    variant: Box::new(variant),
                }),
            )
        }
        ast::ExprFunKind::Table(e) => {
            let header = e.header.iter()
                .map(|h| h.as_str(db).to_string())
                .collect();
            let rows = e.rows.iter()
                .map(|row| {
                    let elements = row.elements.iter()
                        .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(datalit::ast_serde::ExprTableRow { elements })
                })
                .collect::<Result<Vec<_>, ConversionError>>()?;
            (
                e.type_hint,
                datalit::ast_serde::Expr::Table(datalit::ast_serde::ExprTable { header, rows }),
            )
        }
        ast::ExprFunKind::DataFile(_) => {
            return Err(ConversionError::NotPureDatalit("DataFile".to_string()));
        }
        ast::ExprFunKind::ParseError(e) => {
            (
                None,
                datalit::ast_serde::Expr::ParseError(datalit::ast_serde::ExprParseError {
                    message: e.message.as_str(db).to_string(),
                }),
            )
        }

        // Datafun Tuple (no type hint) - convert to AnonTuple.
        ast::ExprFunKind::Tuple(e) => {
            let elements = e.elements.iter()
                .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                .collect::<Result<Vec<_>, _>>()?;
            (
                None,
                datalit::ast_serde::Expr::AnonTuple(datalit::ast_serde::ExprAnonTuple { elements }),
            )
        }

        // UnaryOp Neg - convert to negative integer literal if possible.
        ast::ExprFunKind::UnaryOp(e) => {
            match e.op {
                ast::UnaryOp::Neg => {
                    // Try to convert -N to a single negative integer.
                    let operand = e.operand;
                    match operand.expr(db) {
                        ast::ExprFunKind::Int(int_expr) => {
                            let value_str = int_expr.value.as_str(db);
                            let neg_value = format!("-{}", value_str);
                            (
                                int_expr.type_hint,
                                datalit::ast_serde::Expr::Int(datalit::ast_serde::ExprInt {
                                    value: neg_value,
                                }),
                            )
                        }
                        _ => {
                            return Err(ConversionError::NotPureDatalit("UnaryOp(Neg) on non-int".to_string()));
                        }
                    }
                }
                _ => {
                    return Err(ConversionError::NotPureDatalit("UnaryOp (non-Neg)".to_string()));
                }
            }
        }

        // Datafun-only constructs - return error.
        ast::ExprFunKind::BinOp(_) => {
            return Err(ConversionError::NotPureDatalit("BinOp".to_string()));
        }
        ast::ExprFunKind::FunctionCall(_) => {
            return Err(ConversionError::NotPureDatalit("FunctionCall".to_string()));
        }
        ast::ExprFunKind::TryOption(_) => {
            return Err(ConversionError::NotPureDatalit("TryOption".to_string()));
        }
        ast::ExprFunKind::TryResult(_) => {
            return Err(ConversionError::NotPureDatalit("TryResult".to_string()));
        }
        ast::ExprFunKind::CloneCoerce(_) => {
            return Err(ConversionError::NotPureDatalit("CloneCoerce".to_string()));
        }
        ast::ExprFunKind::FieldProj(_) => {
            return Err(ConversionError::NotPureDatalit("FieldProj".to_string()));
        }
        ast::ExprFunKind::IntrinsicCall(_) => {
            return Err(ConversionError::NotPureDatalit("IntrinsicCall".to_string()));
        }
        // A hint over parentheses. Datalit keeps the parentheses only where
        // what they hold has a hint of its own, and otherwise reads them as
        // what they hold.
        ast::ExprFunKind::Hinted(e) => {
            let inner = datafun_expr_to_datalit_serde(db, e.inner)?;
            let expr_serde = match inner.type_hint {
                Some(_) => datalit::ast_serde::Expr::Group(datalit::ast_serde::ExprGroup {
                    inner: Box::new(inner),
                }),
                None => inner.expr,
            };
            (Some(e.type_hint), expr_serde)
        }
        ast::ExprFunKind::Index(_) => {
            return Err(ConversionError::NotPureDatalit("Index".to_string()));
        }
        ast::ExprFunKind::Place(_) => {
            return Err(ConversionError::NotPureDatalit("Place".to_string()));
        }
    };

    // Convert type hint to serde format.
    let type_hint_serde = type_hint.map(|th| datalit::ast_serde::TypeHint::from_ast(db, th));

    Ok(datalit::ast_serde::ExprFull {
        type_hint: type_hint_serde,
        expr: expr_serde,
    })
}

// ============================================================================
// Typecheck result serde types
// ============================================================================

extern crate rmx;
use rmx::serde as serde;

/// Serializable typecheck result for comparison.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypecheckResultSerde {
    pub root_type: Option<TypeSerde>,
    pub errors: Vec<TypeErrorSerde>,
}

/// Serializable type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TypeSerde {
    Bool, U8, I8, U16, I16, U32, I32, U64, I64, Usize, Isize, F32, F64, Int, String,
    AnonTuple { fields: Vec<TypeSerde> },
    AnonStruct { fields: Vec<(String, TypeSerde)> },

    List { element: Box<TypeSerde> },
    Map { key: Box<TypeSerde>, value: Box<TypeSerde> },
    Set { element: Box<TypeSerde> },
    Option { inner: Box<TypeSerde> },
    Result { inner: Box<TypeSerde> },
    Tensor { element: Box<TypeSerde>, rank: u32 },
    Data,
    Error,
    Atom { name: String },
    Term { name: String, payload: Box<TypeSerde> },
    Enum { variants: Vec<(String, Option<TypeSerde>)> },
    Table { columns: Vec<(String, TypeSerde)> },
}

/// Serializable type error (common subset).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TypeErrorSerde {
    TypeMismatch { expected: String, actual: String },
    UnresolvedName(String),
    CannotSynthesize,
    ArityMismatch { expected: usize, actual: usize },
    /// Catch-all for other error types.
    Other(String),
}

/// Convert datalit TypecheckResult to serde format.
pub fn datalit_typecheck_to_serde<'db>(
    db: &'db dyn salsa::Database,
    result: datalit::tycheck::TypecheckResult<'db>,
) -> TypecheckResultSerde {
    let root_type = result.root_type(db).clone().map(|t| datalit_type_to_serde(db, t));
    let errors = result.errors(db).iter()
        .map(|e| datalit_error_to_serde(&e.error(db)))
        .collect();
    TypecheckResultSerde { root_type, errors }
}

/// Convert UnitTypecheckResultTracked to serde format (production path).
pub fn datafun_unit_typecheck_to_serde<'db>(
    db: &'db dyn salsa::Database,
    result: datalove_datafun_tycheck::UnitTypecheckResultTracked<'db>,
    expr: ast::ExprFun<'db>,
) -> TypecheckResultSerde {

    // Find the type for this expression in expr_types.
    let expr_types = result.expr_types(db);
    let root_type = expr_types
        .get(&datalove_datafun_ast::ast::ExprKey::of(db, expr))
        .map(|t| datafun_type_to_serde(db, t));

    let errors = result.errors(db).iter()
        .map(|e| datafun_error_to_serde(&e.error(db)))
        .collect();

    TypecheckResultSerde { root_type, errors }
}

fn datalit_type_to_serde<'db>(
    db: &'db dyn salsa::Database,
    ty: datalit::tycheck::Type<'db>,
) -> TypeSerde {
    use datalit::tycheck::Type;
    match &ty {
        // A funlit is a data literal, and a data literal has no enclosing
        // generic function to take a type parameter from.
        Type::Var(name) => unreachable!("type parameter {name:?} in a data literal"),
        Type::Bool => TypeSerde::Bool,
        Type::U8 => TypeSerde::U8,
        Type::I8 => TypeSerde::I8,
        Type::U16 => TypeSerde::U16,
        Type::I16 => TypeSerde::I16,
        Type::U32 => TypeSerde::U32,
        Type::I32 => TypeSerde::I32,
        Type::U64 => TypeSerde::U64,
        Type::I64 => TypeSerde::I64,
        Type::Index => TypeSerde::Usize,
        Type::Offset => TypeSerde::Isize,
        Type::F32 => TypeSerde::F32,
        Type::F64 => TypeSerde::F64,
        Type::Int => TypeSerde::Int,
        Type::String => TypeSerde::String,
        Type::AnonTuple(t) => TypeSerde::AnonTuple {
            fields: t.fields.iter().map(|f| datalit_type_to_serde(db, f.clone())).collect(),
        },
        Type::AnonStruct(t) => TypeSerde::AnonStruct {
            fields: t.fields.iter()
                .map(|f| (f.name.as_str(db).to_string(), datalit_type_to_serde(db, (*f.ty).clone())))
                .collect(),
        },

        Type::List(t) => TypeSerde::List {
            element: Box::new(datalit_type_to_serde(db, (*t.element_type).clone())),
        },
        Type::Map(t) => TypeSerde::Map {
            key: Box::new(datalit_type_to_serde(db, (*t.key_type).clone())),
            value: Box::new(datalit_type_to_serde(db, (*t.value_type).clone())),
        },
        Type::Set(t) => TypeSerde::Set {
            element: Box::new(datalit_type_to_serde(db, (*t.element_type).clone())),
        },
        Type::Option(t) => TypeSerde::Option {
            inner: Box::new(datalit_type_to_serde(db, (*t.inner_type).clone())),
        },
        Type::Result(t) => TypeSerde::Result {
            inner: Box::new(datalit_type_to_serde(db, (*t.inner_type).clone())),
        },
        Type::Tensor(t) => TypeSerde::Tensor {
            element: Box::new(datalit_type_to_serde(db, (*t.element_type).clone())),
            rank: t.rank,
        },
        Type::Data => TypeSerde::Data,
        Type::Error => TypeSerde::Error,
        Type::Table(t) => TypeSerde::Table {
            columns: t.columns.iter().map(|c| (
                c.name.as_str(db).to_string(),
                datalit_type_to_serde(db, (*c.ty).clone()),
            )).collect(),
        },
        Type::Atom(a) => TypeSerde::Atom {
            name: a.name.as_str(db).to_string(),
        },
        Type::Term(t) => TypeSerde::Term {
            name: t.name.as_str(db).to_string(),
            payload: Box::new(datalit_type_to_serde(db, (*t.payload).clone())),
        },
        Type::Enum(e) => TypeSerde::Enum {
            variants: e.variants.iter().map(|v| (
                v.name.as_str(db).to_string(),
                v.payload.as_ref().map(|p| datalit_type_to_serde(db, (**p).clone())),
            )).collect(),
        },
    }
}

fn datafun_type_to_serde<'db>(
    db: &'db dyn salsa::Database,
    ty: &datalove_datafun_tycheck::Type<'db>,
) -> TypeSerde {
    use datalove_datafun_tycheck::Type;
    match ty {
        Type::Datalit(inner) => datalit_type_to_serde(db, inner.clone()),
        Type::Function(_) => panic!("Unexpected Function type in funlit_equiv"),
    }
}

fn datalit_error_to_serde(err: &datalit::tycheck::TypeError) -> TypeErrorSerde {
    use datalit::tycheck::TypeError;
    match err {
        TypeError::TypeMismatch { expected, actual } => {
            TypeErrorSerde::TypeMismatch {
                expected: expected.clone(),
                actual: actual.clone(),
            }
        }
        TypeError::CannotSynthesize => TypeErrorSerde::CannotSynthesize,
        TypeError::ArityMismatch { expected, actual } => {
            TypeErrorSerde::ArityMismatch { expected: *expected, actual: *actual }
        }
        other => TypeErrorSerde::Other(format!("{:?}", other)),
    }
}

fn datafun_error_to_serde(err: &datalove_datafun_tycheck::TypeError) -> TypeErrorSerde {
    use datalove_datafun_tycheck::TypeError;
    match err {
        TypeError::TypeMismatch { expected, actual } => {
            TypeErrorSerde::TypeMismatch {
                expected: expected.clone(),
                actual: actual.clone(),
            }
        }
        TypeError::CannotSynthesize => TypeErrorSerde::CannotSynthesize,
        TypeError::UnresolvedName(name) => TypeErrorSerde::UnresolvedName(name.clone()),
        TypeError::ArityMismatch { expected, actual } => {
            TypeErrorSerde::ArityMismatch { expected: *expected, actual: *actual }
        }
        other => TypeErrorSerde::Other(format!("{:?}", other)),
    }
}
