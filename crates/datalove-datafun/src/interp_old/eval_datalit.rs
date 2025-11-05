//! Evaluators for datalit expressions.

use rmx::prelude::*;
use datalove_rt;
use datalove_rtdt as rtdt;
use crate::datalit;
use crate::interp_old::interp::{InterpContext, InterpResult, InterpError};
use crate::interp_old::value::Value;

/// Tracked wrapper for converting a type hint to a type.
///
/// This allows calling convert_type_hint from non-tracked contexts.
#[salsa::tracked]
pub fn convert_type_hint_tracked<'db>(
    db: &'db dyn crate::Db,
    type_hint: datalit::ast::TypeHintAndHeap<'db>,
) -> Option<datalit::tycheck::TypeAndHeap<'db>> {
    datalit::tycheck::convert_type_hint(db, type_hint).ok()
}

/// Tracked wrapper for datalit type checking with expected type.
///
/// This allows calling type_check_with_expected from non-tracked contexts
/// while maintaining Salsa memoization.
#[salsa::tracked]
fn typecheck_datalit<'db>(
    db: &'db dyn crate::Db,
    expr: datalit::ast::ExprFull<'db>,
    expected: Option<datalit::tycheck::TypeAndHeap<'db>>,
) -> datalit::tycheck::TypecheckResult<'db> {
    let dummy_source = bct::input::Source::new(db, String::new());
    let resolved = datalit::resolve::resolve_names(db, dummy_source, expr);
    datalit::tycheck::type_check_with_expected(db, expr, resolved, expected)
}

/// Convert an InstantiatedValue from instantiate2 to the interpreter's Value enum.
///
/// For primitive types that are copied into the Value enum, the original
/// allocation is destroyed. For reference types, ownership is transferred.
fn instantiated_to_value(
    inst: datalit::instantiate2::InstantiatedValue,
    rt: datalove_rt::c::LocalRtHandle,
) -> InterpResult {
    use rtdt::TyTag;

    let type_tag = inst.tydesc.type_tag();

    match type_tag {
        TyTag::Bool => {
            let value = unsafe { *(inst.ptr as *const bool) };
            // Free the wrapper allocation since we copied the value.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(rt, inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            }
            Ok(Value::Bool(value))
        }
        TyTag::U8 => {
            let value = unsafe { *(inst.ptr as *const u8) };
            // Free the wrapper allocation since we copied the value.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(rt, inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            }
            Ok(Value::U32(value as u32))
        }
        TyTag::I8 => {
            let value = unsafe { *(inst.ptr as *const i8) };
            // Free the wrapper allocation since we copied the value.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(rt, inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            }
            Ok(Value::U32((value as i32) as u32))
        }
        TyTag::U16 => {
            let value = unsafe { *(inst.ptr as *const u16) };
            // Free the wrapper allocation since we copied the value.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(rt, inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            }
            Ok(Value::U32(value as u32))
        }
        TyTag::I16 => {
            let value = unsafe { *(inst.ptr as *const i16) };
            // Free the wrapper allocation since we copied the value.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(rt, inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            }
            Ok(Value::U32((value as i32) as u32))
        }
        TyTag::U32 => {
            let value = unsafe { *(inst.ptr as *const u32) };
            // Free the wrapper allocation since we copied the value.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(rt, inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            }
            Ok(Value::U32(value))
        }
        TyTag::I32 => {
            let value = unsafe { *(inst.ptr as *const i32) };
            // Free the wrapper allocation since we copied the value.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(rt, inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            }
            Ok(Value::U32(value as u32))
        }
        TyTag::F32 => {
            let value = unsafe { *(inst.ptr as *const f32) };
            // Free the wrapper allocation since we copied the value.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(rt, inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            }
            Ok(Value::F32(value))
        }
        TyTag::Int => {
            Ok(Value::Int {
                ptr: inst.ptr as *mut rtdt::Int,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::String => {
            Ok(Value::String {
                ptr: inst.ptr as *mut rtdt::String,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::Tuple => {
            Ok(Value::Tuple {
                ptr: inst.ptr as *mut u8,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::Struct => {
            Ok(Value::Struct {
                ptr: inst.ptr as *mut u8,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::Enum => {
            Ok(Value::Enum {
                ptr: inst.ptr as *mut u8,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::List => {
            Ok(Value::List {
                ptr: inst.ptr as *mut rtdt::List,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::Map => {
            Ok(Value::Map {
                ptr: inst.ptr as *mut rtdt::Map,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::Set => {
            Ok(Value::Set {
                ptr: inst.ptr as *mut rtdt::Set,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::Option => {
            Ok(Value::Option {
                ptr: inst.ptr as *mut u8,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::Result => {
            Ok(Value::Result {
                ptr: inst.ptr as *mut u8,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::Data => {
            Ok(Value::Data {
                ptr: inst.ptr as *mut rtdt::Data,
                tydesc: inst.tydesc.as_ptr(),
            })
        }
        TyTag::Error => {
            Ok(Value::Error {
                ptr: inst.ptr as *mut rtdt::Error,
                tydesc: inst.tydesc.as_ptr(),
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
    expected: Option<datalit::tycheck::TypeAndHeap<'db>>,
) -> InterpResult {
    // Run typechecker to get TypecheckResult needed by instantiate2.
    // Use tracked wrapper to allow calling from non-tracked context.
    let typechecked = typecheck_datalit(ctx.db, expr, expected);

    // Check for type errors.
    if !typechecked.errors(ctx.db).is_empty() {
        return Err(InterpError::TypeError(
            format!("Typecheck errors in datalit expression: {} errors", typechecked.errors(ctx.db).len()),
        ));
    }

    // Use instantiate2 to create the runtime value.
    let rt_handle = &mut *ctx.rt as *mut _ as datalove_rt::c::LocalRtHandle;
    let inst = datalit::instantiate2::instantiate_value(
        ctx.db,
        rt_handle,
        &mut ctx.tydesc_table,
        typechecked,
    ).map_err(|e| InterpError::RuntimeError(
        format!("Failed to instantiate value: {}", e),
    ))?;

    // Convert InstantiatedValue to Value enum.
    instantiated_to_value(inst, rt_handle)
}

