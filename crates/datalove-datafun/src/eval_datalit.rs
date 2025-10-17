//! Evaluators for datalit expressions.

use rmx::prelude::*;
use datalove_rt;
use datalove_rtdt as rtdt;
use crate::datalit;
use crate::interp::{InterpContext, InterpResult, InterpError};
use crate::value::Value;

/// Convert an InstantiatedValue from instantiate2 to the interpreter's Value enum.
fn instantiated_to_value(inst: datalit::instantiate2::InstantiatedValue) -> InterpResult {
    use rtdt::TyTag;

    let type_tag = unsafe { (*inst.tydesc).type_tag };

    match type_tag {
        TyTag::Bool => {
            let value = unsafe { *(inst.ptr as *const bool) };
            Ok(Value::Bool(value))
        }
        TyTag::U32 => {
            let value = unsafe { *(inst.ptr as *const u32) };
            Ok(Value::U32(value))
        }
        TyTag::F32 => {
            let value = unsafe { *(inst.ptr as *const f32) };
            Ok(Value::F32(value))
        }
        TyTag::Int => {
            Ok(Value::Int {
                ptr: inst.ptr as *mut rtdt::Int,
                tydesc: inst.tydesc,
            })
        }
        TyTag::String => {
            Ok(Value::String {
                ptr: inst.ptr as *mut rtdt::String,
                tydesc: inst.tydesc,
            })
        }
        TyTag::Tuple => {
            Ok(Value::Tuple {
                ptr: inst.ptr as *mut u8,
                tydesc: inst.tydesc,
            })
        }
        TyTag::Struct => {
            Ok(Value::Struct {
                ptr: inst.ptr as *mut u8,
                tydesc: inst.tydesc,
            })
        }
        TyTag::Enum => {
            Ok(Value::Enum {
                ptr: inst.ptr as *mut u8,
                tydesc: inst.tydesc,
            })
        }
        TyTag::List => {
            Ok(Value::List {
                ptr: inst.ptr as *mut rtdt::List,
                tydesc: inst.tydesc,
            })
        }
        TyTag::Map => {
            Ok(Value::Map {
                ptr: inst.ptr as *mut rtdt::Map,
                tydesc: inst.tydesc,
            })
        }
        TyTag::Set => {
            Ok(Value::Set {
                ptr: inst.ptr as *mut rtdt::Set,
                tydesc: inst.tydesc,
            })
        }
        TyTag::Option => {
            Ok(Value::Option {
                ptr: inst.ptr as *mut u8,
                tydesc: inst.tydesc,
            })
        }
        TyTag::Result => {
            Ok(Value::Result {
                ptr: inst.ptr as *mut u8,
                tydesc: inst.tydesc,
            })
        }
        TyTag::Data => {
            Ok(Value::Data {
                ptr: inst.ptr as *mut rtdt::Data,
                tydesc: inst.tydesc,
            })
        }
        TyTag::Error => {
            Ok(Value::Error {
                ptr: inst.ptr as *mut rtdt::Error,
                tydesc: inst.tydesc,
            })
        }
        _ => {
            Err(InterpError::RuntimeError(
                format!("Unsupported type tag: {:?}", type_tag),
            ))
        }
    }
}

/// Evaluate a datalit expression.
pub fn eval_datalit<'db>(
    ctx: &mut InterpContext<'db>,
    expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    // Run typechecker to get TypecheckResult needed by instantiate2.
    let resolved = datalit::resolve::resolve_names(ctx.db, expr);
    let typechecked = datalit::tycheck::type_check(ctx.db, expr, resolved);

    // Check for type errors.
    if !typechecked.errors(ctx.db).is_empty() {
        return Err(InterpError::TypeError(
            format!("Typecheck errors in datalit expression: {} errors", typechecked.errors(ctx.db).len()),
        ));
    }

    // Use instantiate2 to create the runtime value.
    let inst = datalit::instantiate2::instantiate_value(
        ctx.db,
        &mut ctx.rt,
        &mut ctx.tydesc_table,
        typechecked,
    ).map_err(|e| InterpError::RuntimeError(
        format!("Failed to instantiate value: {}", e),
    ))?;

    // Convert InstantiatedValue to Value enum.
    instantiated_to_value(inst)
}

