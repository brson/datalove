//! Funlit equivalence testing infrastructure.
//!
//! Provides conversions and utilities for comparing datafun and datalit AST and typecheck results.
//! Used to verify that datafun's inline literal parsing produces equivalent results to datalit.

use rmx::prelude::*;

use crate::ast;
use crate::datalit;

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

/// Extract a single expression from a Script that contains exactly one Ret statement.
pub fn extract_expr_from_script<'db>(
    db: &'db dyn crate::Db,
    script: ast::Script<'db>,
) -> Result<ast::ExprFun<'db>, ConversionError> {
    let statements = script.statements(db);
    if statements.len() != 1 {
        return Err(ConversionError::InvalidScriptStructure(
            format!("expected 1 statement, got {}", statements.len())
        ));
    }
    match &statements[0] {
        ast::Statement::Ret(ret) => Ok(ret.value(db)),
        other => Err(ConversionError::InvalidScriptStructure(
            format!("expected Ret statement, got {:?}", std::mem::discriminant(other))
        )),
    }
}

/// Convert a datafun ExprFun to datalit::ast_serde::ExprFull.
///
/// Only works for pure datalit expressions. Returns error for datafun-only constructs.
pub fn datafun_expr_to_datalit_serde<'db>(
    db: &'db dyn crate::Db,
    expr: ast::ExprFun<'db>,
) -> Result<datalit::ast_serde::ExprFull, ConversionError> {
    let kind = expr.expr(db);

    // Get heap and type_hint from the expression kind.
    let (heap, type_hint, expr_serde) = match kind {
        ast::ExprFunKind::True(e) => (
            e.heap(db),
            e.type_hint(db),
            datalit::ast_serde::Expr::True,
        ),
        ast::ExprFunKind::False(e) => (
            e.heap(db),
            e.type_hint(db),
            datalit::ast_serde::Expr::False,
        ),
        ast::ExprFunKind::None(e) => (
            e.heap(db),
            e.type_hint(db),
            datalit::ast_serde::Expr::None,
        ),
        ast::ExprFunKind::Int(e) => (
            e.heap(db),
            e.type_hint(db),
            datalit::ast_serde::Expr::Int(datalit::ast_serde::ExprInt {
                value: e.value(db).as_str(db).to_string(),
            }),
        ),
        ast::ExprFunKind::Float(e) => (
            e.heap(db),
            e.type_hint(db),
            datalit::ast_serde::Expr::Float(datalit::ast_serde::ExprFloat {
                value: e.value(db).as_str(db).to_string(),
            }),
        ),
        ast::ExprFunKind::Hex(e) => (
            e.heap(db),
            e.type_hint(db),
            datalit::ast_serde::Expr::Hex(datalit::ast_serde::ExprHex {
                value: e.value(db).as_str(db).to_string(),
            }),
        ),
        ast::ExprFunKind::String(e) => (
            e.heap(db),
            e.type_hint(db),
            datalit::ast_serde::Expr::String(datalit::ast_serde::ExprString {
                value: e.value(db).as_str(db).to_string(),
            }),
        ),
        ast::ExprFunKind::List(e) => {
            let elements = e.elements(db).iter()
                .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                .collect::<Result<Vec<_>, _>>()?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::List(datalit::ast_serde::ExprList { elements }),
            )
        }
        ast::ExprFunKind::Set(e) => {
            let elements = e.elements(db).iter()
                .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                .collect::<Result<Vec<_>, _>>()?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::Set(datalit::ast_serde::ExprSet { elements }),
            )
        }
        ast::ExprFunKind::Map(e) => {
            let entries = e.entries(db).iter()
                .map(|entry| {
                    let key = datafun_expr_to_datalit_serde(db, entry.key(db))?;
                    let value = datafun_expr_to_datalit_serde(db, entry.value(db))?;
                    Ok(datalit::ast_serde::ExprMapEntry { key, value })
                })
                .collect::<Result<Vec<_>, ConversionError>>()?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::Map(datalit::ast_serde::ExprMap { entries }),
            )
        }
        ast::ExprFunKind::Tensor(e) => {
            let elements = e.elements(db).iter()
                .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                .collect::<Result<Vec<_>, _>>()?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::Tensor(datalit::ast_serde::ExprTensor {
                    shape: e.shape(db).clone(),
                    elements,
                }),
            )
        }
        ast::ExprFunKind::AnonTuple(e) => {
            let elements = e.elements(db).iter()
                .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                .collect::<Result<Vec<_>, _>>()?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::AnonTuple(datalit::ast_serde::ExprAnonTuple { elements }),
            )
        }
        ast::ExprFunKind::AnonStruct(e) => {
            let fields = e.fields(db).iter()
                .map(|field| {
                    let value = datafun_expr_to_datalit_serde(db, field.value(db))?;
                    Ok(datalit::ast_serde::ExprStructField {
                        name: field.name(db).as_str(db).to_string(),
                        value,
                    })
                })
                .collect::<Result<Vec<_>, ConversionError>>()?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::AnonStruct(datalit::ast_serde::ExprAnonStruct { fields }),
            )
        }
        ast::ExprFunKind::AnonEnum(e) => {
            let payload = e.payload(db)
                .map(|p| datafun_expr_to_datalit_serde(db, p))
                .transpose()?
                .map(Box::new);
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::AnonEnum(datalit::ast_serde::ExprAnonEnum {
                    variant_name: e.variant_name(db).as_str(db).to_string(),
                    payload,
                }),
            )
        }
        ast::ExprFunKind::Some(e) => {
            let payload = datafun_expr_to_datalit_serde(db, e.payload(db))?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::Some(datalit::ast_serde::ExprSome {
                    payload: Box::new(payload),
                }),
            )
        }
        ast::ExprFunKind::Ok(e) => {
            let payload = datafun_expr_to_datalit_serde(db, e.payload(db))?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::Ok(datalit::ast_serde::ExprOk {
                    payload: Box::new(payload),
                }),
            )
        }
        ast::ExprFunKind::Er(e) => {
            let payload = datafun_expr_to_datalit_serde(db, e.payload(db))?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::Er(datalit::ast_serde::ExprEr {
                    payload: Box::new(payload),
                }),
            )
        }
        ast::ExprFunKind::Data(e) => {
            let value = datafun_expr_to_datalit_serde(db, e.value(db))?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::Data(datalit::ast_serde::ExprData {
                    value: Box::new(value),
                }),
            )
        }
        ast::ExprFunKind::Err(e) => {
            let value = datafun_expr_to_datalit_serde(db, e.value(db))?;
            (
                e.heap(db),
                e.type_hint(db),
                datalit::ast_serde::Expr::Err(datalit::ast_serde::ExprErr {
                    value: Box::new(value),
                }),
            )
        }
        ast::ExprFunKind::ParseError(e) => {
            (
                datalit::ast::Heap::Omitted,
                None,
                datalit::ast_serde::Expr::ParseError(datalit::ast_serde::ExprParseError {
                    message: e.message(db).as_str(db).to_string(),
                }),
            )
        }

        // Datafun Tuple (no type hint) - convert to AnonTuple.
        ast::ExprFunKind::Tuple(e) => {
            let elements = e.elements(db).iter()
                .map(|elem| datafun_expr_to_datalit_serde(db, *elem))
                .collect::<Result<Vec<_>, _>>()?;
            (
                datalit::ast::Heap::Omitted,
                None,
                datalit::ast_serde::Expr::AnonTuple(datalit::ast_serde::ExprAnonTuple { elements }),
            )
        }

        // UnaryOp Neg - convert to negative integer literal if possible.
        ast::ExprFunKind::UnaryOp(e) => {
            match e.op(db) {
                ast::UnaryOp::Neg => {
                    // Try to convert -N to a single negative integer.
                    let operand = e.operand(db);
                    match operand.expr(db) {
                        ast::ExprFunKind::Int(int_expr) => {
                            let value_str = int_expr.value(db).as_str(db);
                            let neg_value = format!("-{}", value_str);
                            (
                                int_expr.heap(db),
                                int_expr.type_hint(db),
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
        ast::ExprFunKind::Name(_) => {
            return Err(ConversionError::NotPureDatalit("Name".to_string()));
        }
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
    };

    // Convert type hint using datalit's from_ast.
    let type_hint_serde = type_hint.map(|th| datalit::ast_serde::TypeHintAndHeap::from_ast(db, th));

    // Convert heap using datalit's from_ast.
    let heap_serde = datalit::ast_serde::Heap::from_ast(heap);

    Ok(datalit::ast_serde::ExprFull {
        type_hint: type_hint_serde,
        expr: datalit::ast_serde::ExprAndHeap {
            heap: heap_serde,
            expr: expr_serde,
        },
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
    pub root_type: Option<TypeAndHeapSerde>,
    pub errors: Vec<TypeErrorSerde>,
}

/// Serializable type with heap annotation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeAndHeapSerde {
    pub heap: HeapSerde,
    pub ty: TypeSerde,
}

/// Serializable heap annotation.
#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HeapSerde {
    Local,
    Global,
    Omitted,
}

/// Serializable type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TypeSerde {
    Bool, U8, I8, U16, I16, U32, I32, U64, I64, F32, Int, String,
    AnonTuple { fields: Vec<TypeAndHeapSerde> },
    AnonStruct { fields: Vec<(String, TypeAndHeapSerde)> },
    AnonEnum { variants: Vec<(String, Option<TypeAndHeapSerde>)> },
    List { element: Box<TypeAndHeapSerde> },
    Map { key: Box<TypeAndHeapSerde>, value: Box<TypeAndHeapSerde> },
    Set { element: Box<TypeAndHeapSerde> },
    Option { inner: Box<TypeAndHeapSerde> },
    Result { inner: Box<TypeAndHeapSerde> },
    Tensor { element: Box<TypeAndHeapSerde>, rank: u32 },
    Data,
    Error,
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
    db: &'db dyn crate::Db,
    result: datalit::tycheck::TypecheckResult<'db>,
) -> TypecheckResultSerde {
    let root_type = result.root_type(db).map(|t| datalit_type_to_serde(db, t));
    let errors = result.errors(db).iter()
        .map(|e| datalit_error_to_serde(&e.error(db)))
        .collect();
    TypecheckResultSerde { root_type, errors }
}

/// Convert datafun TypecheckResult to serde format.
///
/// Note: datafun stores types per-expression, so we need the expression to look up its type.
pub fn datafun_typecheck_to_serde<'db>(
    db: &'db dyn crate::Db,
    result: crate::tycheck::TypecheckResult<'db>,
    expr: ast::ExprFun<'db>,
) -> TypecheckResultSerde {
    use salsa::plumbing::AsId;

    // Find the type for this expression in expr_types.
    let expr_id = expr.as_id().index() as usize;
    let expr_types = result.expr_types(db);
    let root_type = expr_types.get(expr_id)
        .and_then(|opt| opt.as_ref())
        .map(|t| datafun_type_to_serde(db, t));

    let errors = result.errors(db).iter()
        .map(|e| datafun_error_to_serde(&e.error(db)))
        .collect();

    TypecheckResultSerde { root_type, errors }
}

fn datalit_type_to_serde<'db>(
    db: &'db dyn crate::Db,
    ty: datalit::tycheck::TypeAndHeap<'db>,
) -> TypeAndHeapSerde {
    TypeAndHeapSerde {
        heap: heap_to_serde(ty.heap(db)),
        ty: datalit_type_inner_to_serde(db, ty.ty(db)),
    }
}

fn datafun_type_to_serde<'db>(
    db: &'db dyn crate::Db,
    ty: &crate::tycheck::TypeAndHeap<'db>,
) -> TypeAndHeapSerde {
    TypeAndHeapSerde {
        heap: heap_to_serde(ty.heap(db)),
        ty: datafun_type_inner_to_serde(db, ty.ty(db)),
    }
}

fn heap_to_serde(heap: datalit::ast::Heap) -> HeapSerde {
    match heap {
        datalit::ast::Heap::Local => HeapSerde::Local,
        datalit::ast::Heap::Global => HeapSerde::Global,
        datalit::ast::Heap::Omitted => HeapSerde::Omitted,
    }
}

fn datalit_type_inner_to_serde<'db>(
    db: &'db dyn crate::Db,
    ty: &datalit::tycheck::Type<'db>,
) -> TypeSerde {
    use datalit::tycheck::Type;
    match ty {
        Type::Bool => TypeSerde::Bool,
        Type::U8 => TypeSerde::U8,
        Type::I8 => TypeSerde::I8,
        Type::U16 => TypeSerde::U16,
        Type::I16 => TypeSerde::I16,
        Type::U32 => TypeSerde::U32,
        Type::I32 => TypeSerde::I32,
        Type::U64 => TypeSerde::U64,
        Type::I64 => TypeSerde::I64,
        Type::F32 => TypeSerde::F32,
        Type::Int => TypeSerde::Int,
        Type::String => TypeSerde::String,
        Type::AnonTuple(t) => TypeSerde::AnonTuple {
            fields: t.fields(db).iter().map(|f| datalit_type_to_serde(db, *f)).collect(),
        },
        Type::AnonStruct(t) => TypeSerde::AnonStruct {
            fields: t.fields(db).iter()
                .map(|f| (f.name(db).as_str(db).to_string(), datalit_type_to_serde(db, f.ty(db))))
                .collect(),
        },
        Type::AnonEnum(t) => TypeSerde::AnonEnum {
            variants: t.variants(db).iter()
                .map(|v| (v.name(db).as_str(db).to_string(), v.payload(db).map(|p| datalit_type_to_serde(db, p))))
                .collect(),
        },
        Type::List(t) => TypeSerde::List {
            element: Box::new(datalit_type_to_serde(db, t.element_type(db))),
        },
        Type::Map(t) => TypeSerde::Map {
            key: Box::new(datalit_type_to_serde(db, t.key_type(db))),
            value: Box::new(datalit_type_to_serde(db, t.value_type(db))),
        },
        Type::Set(t) => TypeSerde::Set {
            element: Box::new(datalit_type_to_serde(db, t.element_type(db))),
        },
        Type::Option(t) => TypeSerde::Option {
            inner: Box::new(datalit_type_to_serde(db, t.inner_type(db))),
        },
        Type::Result(t) => TypeSerde::Result {
            inner: Box::new(datalit_type_to_serde(db, t.inner_type(db))),
        },
        Type::Tensor(t) => TypeSerde::Tensor {
            element: Box::new(datalit_type_to_serde(db, t.element_type(db))),
            rank: t.rank(db),
        },
        Type::Data => TypeSerde::Data,
        Type::Error => TypeSerde::Error,
    }
}

fn datafun_type_inner_to_serde<'db>(
    db: &'db dyn crate::Db,
    ty: &crate::tycheck::Type<'db>,
) -> TypeSerde {
    use crate::tycheck::Type;
    match ty {
        Type::Datalit(inner) => datalit_type_inner_to_serde(db, inner),
        Type::Function(_) => datafun_type_inner_to_serde_panic("Function"),
        Type::Void => datafun_type_inner_to_serde_panic("Void"),
    }
}

fn datafun_type_inner_to_serde_panic(ty_name: &str) -> TypeSerde {
    panic!("Unexpected non-datalit type in funlit_equiv: {}", ty_name)
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

fn datafun_error_to_serde(err: &crate::tycheck::TypeError) -> TypeErrorSerde {
    use crate::tycheck::TypeError;
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
