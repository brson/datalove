//! Instantiate runtime values from typechecked AST (v2 - runtime-integrated).
//!
//! Integrates with the datalove-rt runtime directly, using LocalRt for all allocations.
//!
//! ## Design Philosophy
//!
//! Unlike instantiate.rs which maintains its own `ValueHeap`, this module
//! directly calls the datalove-rt runtime. The caller provides a LocalRt
//! and is responsible for lifetime management.
//!
//! ## Intended Usage
//!
//! ```ignore
//! // In datafun or other runtime-integrated code:
//! let mut rt = datalove_rt::rust::Runtime::new();
//! let mut tydesc_table = TyDescTable::new(db);
//! let result = instantiate_value(db, &mut rt, &mut tydesc_table, typechecked)?;
//! // Values are owned by rt, cleaned up when rt.shutdown() is called
//! ```
//!
//! ## Benefits Over Original Design
//!
//! 1. Single source of truth for allocations (no separate ValueHeap)
//! 2. Integrates with runtime leak detection
//! 3. Uses runtime's size-class allocator
//! 4. Consistent with datafun interpreter pattern
//! 5. Can call runtime helper functions (string_create_local, etc.)

use rmx::prelude::*;
use crate::ast::*;
use crate::resolve::ResolvedExpr;
use crate::tycheck::*;
use datalove_rtdt as rtdt;
use crate::tydesc_table::TyDescTable;

/// An instantiated value with its type descriptor.
///
/// The value's lifetime is managed by the allocator that created it.
pub struct InstantiatedValue<'a> {
    pub ptr: *const u8,
    pub tydesc: rtdt::TyDescRef<'a>,
}

/// Instantiate a value from typechecked AST using the runtime.
///
/// This is the primary entry point for runtime-integrated instantiation.
/// The caller provides a LocalRt and is responsible for cleanup.
pub fn instantiate_value<'db, 't>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    tydesc_table: &'t mut TyDescTable<'db>,
    typechecked: TypecheckResult<'db>,
) -> AnyResult<InstantiatedValue<'t>> {
    let root_type = typechecked.root_type(db).clone()
        .ok_or_else(|| anyhow!("No root type"))?;
    let root_expr = typechecked.root_expr(db);
    let resolved = typechecked.resolved(db);

    let value_ptr = instantiate_expr(db, rt, root_expr, &root_type, tydesc_table, resolved)?;

    let tydesc = tydesc_table.get_or_create_ref(&root_type);

    Ok(InstantiatedValue {
        ptr: value_ptr,
        tydesc,
    })
}

/// Instantiate an expression into a runtime value.
///
/// Allocates memory and returns the pointer.
fn instantiate_expr<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    expr: ExprFull<'db>,
    ty: &Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    let tydesc_ptr = tydesc_table.get_or_create(ty);

    // Use RAII guard for automatic cleanup on error.
    let guard = datalove_rt::rust::MemGuard::new(rt, tydesc_ptr, 1)
        .ok_or_else(|| anyhow!("Failed to allocate memory"))?;
    let dest_ptr = guard.ptr();

    // Try to instantiate. On success, leak the guard to transfer ownership.
    instantiate_expr_into(db, rt, expr, ty, tydesc_table, dest_ptr, resolved)?;
    Ok(guard.leak())
}

/// Instantiate an expression into pre-allocated memory at dest_ptr.
///
/// dest_ptr must be valid and properly aligned.
fn instantiate_expr_into<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    expr: ExprFull<'db>,
    ty: &Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null(), "dest_ptr must be non-null");
    let expr_inner = expr.expr(db);

    match (expr_inner, ty) {
        (Expr::True, Type::Bool) => instantiate_bool(rt, true, dest_ptr),
        (Expr::False, Type::Bool) => instantiate_bool(rt, false, dest_ptr),

        (Expr::Int(int_expr), Type::U8) => instantiate_u8(rt, db, int_expr, dest_ptr),
        (Expr::Int(int_expr), Type::I8) => instantiate_i8(rt, db, int_expr, dest_ptr),
        (Expr::Int(int_expr), Type::U16) => instantiate_u16(rt, db, int_expr, dest_ptr),
        (Expr::Int(int_expr), Type::I16) => instantiate_i16(rt, db, int_expr, dest_ptr),
        (Expr::Int(int_expr), Type::U32) => instantiate_u32(rt, db, int_expr, dest_ptr),
        (Expr::Int(int_expr), Type::I32) => instantiate_i32(rt, db, int_expr, dest_ptr),
        (Expr::Int(int_expr), Type::U64) => instantiate_u64(rt, db, int_expr, dest_ptr),
        (Expr::Int(int_expr), Type::I64) => instantiate_i64(rt, db, int_expr, dest_ptr),
        (Expr::Int(int_expr), Type::Index) => instantiate_usize(rt, db, int_expr, dest_ptr),
        (Expr::Int(int_expr), Type::Offset) => instantiate_isize(rt, db, int_expr, dest_ptr),
        (Expr::Int(int_expr), Type::Int) => instantiate_bigint(rt, db, int_expr, dest_ptr),

        (Expr::Float(float_expr), Type::F32) => instantiate_f32(rt, db, float_expr, dest_ptr),
        (Expr::Float(float_expr), Type::F64) => instantiate_f64(rt, db, float_expr, dest_ptr),

        // Hex literals for integer types.
        (Expr::Hex(hex_expr), Type::U8) => instantiate_hex_u8(rt, db, hex_expr, dest_ptr),
        (Expr::Hex(hex_expr), Type::U16) => instantiate_hex_u16(rt, db, hex_expr, dest_ptr),
        (Expr::Hex(hex_expr), Type::U32) => instantiate_hex_u32(rt, db, hex_expr, dest_ptr),
        (Expr::Hex(hex_expr), Type::U64) => instantiate_hex_u64(rt, db, hex_expr, dest_ptr),
        (Expr::Hex(hex_expr), Type::Index) => instantiate_hex_usize(rt, db, hex_expr, dest_ptr),
        (Expr::Hex(hex_expr), Type::Int) => instantiate_hex_bigint(rt, db, hex_expr, dest_ptr),

        // Hex literals for floats - interpret as bit patterns.
        (Expr::Hex(hex_expr), Type::F32) => instantiate_hex_f32(rt, db, hex_expr, dest_ptr),
        (Expr::Hex(hex_expr), Type::F64) => instantiate_hex_f64(rt, db, hex_expr, dest_ptr),

        (Expr::String(string_expr), Type::String) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_string(rt, db, string_expr, tydesc, dest_ptr)
        }

        (Expr::AnonTuple(tuple_expr), Type::AnonTuple(tuple_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_tuple(db, rt, &tuple_expr.elements, &tuple_ty.fields, tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::AnonStruct(struct_expr), Type::AnonStruct(struct_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_struct(db, rt, &struct_expr.fields, &struct_ty.fields, tydesc_table, tydesc, dest_ptr, resolved)
        }


        (Expr::List(list_expr), Type::List(list_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_list(db, rt, &list_expr.elements, *list_ty.element_type.clone(), tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::None, Type::Option(opt)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_option(db, rt, false, None, *opt.inner_type.clone(), tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::Some(some_expr), Type::Option(opt)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_option(db, rt, true, Some(some_expr.payload), *opt.inner_type.clone(), tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::Er(er_expr), Type::Result(res)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_result(db, rt, false, None, Some(er_expr.payload), *res.inner_type.clone(), tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::Ok(ok_expr), Type::Result(res)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_result(db, rt, true, Some(ok_expr.payload), None, *res.inner_type.clone(), tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::Data(data_expr), Type::Data) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_data(db, rt, data_expr.value, tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::Error(err_expr), Type::Error) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_error(db, rt, err_expr.value, tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::Map(map_expr), Type::Map(map_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_map(db, rt, map_expr.clone(), *map_ty.key_type.clone(), *map_ty.value_type.clone(), tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::Set(set_expr), Type::Set(set_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_set(db, rt, set_expr.clone(), *set_ty.element_type.clone(), tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::Tensor(tensor_expr), Type::Tensor(tensor_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_tensor(db, rt, &tensor_expr.shape, &tensor_expr.elements, *tensor_ty.element_type.clone(), tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::Table(table_expr), Type::Table(table_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_table(db, rt, &table_expr.rows, &table_ty.columns, tydesc_table, tydesc, dest_ptr, resolved)
        }

        (Expr::Group(group), _) => {
            instantiate_expr_into(db, rt, group.inner, ty, tydesc_table, dest_ptr, resolved)
        }

        (Expr::Atom(_), Type::Atom(_)) => Ok(dest_ptr as *const u8),

        (Expr::Term(term_expr), Type::Term(term_ty)) => {
            instantiate_expr_into(db, rt, term_expr.payload, &term_ty.payload, tydesc_table, dest_ptr, resolved)?;
            Ok(dest_ptr as *const u8)
        }

        (Expr::Enum(enum_expr), Type::Enum(_)) => {
            instantiate_expr_into(db, rt, enum_expr.variant, ty, tydesc_table, dest_ptr, resolved)
        }

        (Expr::Atom(atom_expr), Type::Enum(enum_ty)) => {
            instantiate_variant(db, rt, atom_expr.name, None, enum_ty, ty, tydesc_table, dest_ptr, resolved)
        }

        (Expr::Term(term_expr), Type::Enum(enum_ty)) => {
            instantiate_variant(db, rt, term_expr.name, Some(term_expr.payload), enum_ty, ty, tydesc_table, dest_ptr, resolved)
        }

        _ => bail!("Unsupported expression/type combination for instantiation"),
    }
}

// ============================================================================
// Scalar type instantiation
// ============================================================================

fn instantiate_bool(_rt: datalove_rt::c::LocalRtHandle, value: bool, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    unsafe {
        *dest_ptr = if value { 1 } else { 0 };
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u8(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: u8 = value_str.parse()?;
    unsafe {
        *dest_ptr = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i8(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: i8 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i8) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u16(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: u16 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut u16) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i16(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: i16 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i16) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u32(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: u32 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut u32) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i32(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: i32 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i32) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u64(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: u64 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut u64) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i64(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: i64 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i64) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_usize(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: datalove_rtdt::IndexRepr = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut datalove_rtdt::Index) = datalove_rtdt::Index(value);
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_isize(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: datalove_rtdt::OffsetRepr = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut datalove_rtdt::Offset) = datalove_rtdt::Offset(value);
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_f32(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, float_expr: &ExprFloat, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(float_expr.value.as_str(db));
    let value: f32 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut f32) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_f64(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, float_expr: &ExprFloat, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(float_expr.value.as_str(db));
    let value: f64 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut f64) = value;
        Ok(dest_ptr as *const u8)
    }
}

// ============================================================================
// Hex literal instantiation
// ============================================================================

/// Parse hex string, stripping the `0x` or `0X` prefix.
/// A hex literal's digits: no prefix, and no separators either.
fn parse_hex_str(s: &str) -> String {
    let digits = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    crate::parser_util::strip_separators(digits).into_owned()
}

fn instantiate_hex_u8(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, hex_expr: &ExprHex, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = hex_expr.value.as_str(db);
    let value = u8::from_str_radix(&parse_hex_str(value_str), 16)?;
    unsafe {
        *dest_ptr = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_hex_u16(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, hex_expr: &ExprHex, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = hex_expr.value.as_str(db);
    let value = u16::from_str_radix(&parse_hex_str(value_str), 16)?;
    unsafe {
        *(dest_ptr as *mut u16) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_hex_u32(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, hex_expr: &ExprHex, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = hex_expr.value.as_str(db);
    let value = u32::from_str_radix(&parse_hex_str(value_str), 16)?;
    unsafe {
        *(dest_ptr as *mut u32) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_hex_u64(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, hex_expr: &ExprHex, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = hex_expr.value.as_str(db);
    let value = u64::from_str_radix(&parse_hex_str(value_str), 16)?;
    unsafe {
        *(dest_ptr as *mut u64) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_hex_usize(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, hex_expr: &ExprHex, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = hex_expr.value.as_str(db);
    let value = datalove_rtdt::IndexRepr::from_str_radix(&parse_hex_str(value_str), 16)?;
    unsafe {
        *(dest_ptr as *mut datalove_rtdt::Index) = datalove_rtdt::Index(value);
        Ok(dest_ptr as *const u8)
    }
}

/// Hex literal interpreted as f32 bit pattern.
fn instantiate_hex_f32(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, hex_expr: &ExprHex, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = hex_expr.value.as_str(db);
    let bits = u32::from_str_radix(&parse_hex_str(value_str), 16)?;
    let value = f32::from_bits(bits);
    unsafe {
        *(dest_ptr as *mut f32) = value;
        Ok(dest_ptr as *const u8)
    }
}

/// Hex literal interpreted as f64 bit pattern.
fn instantiate_hex_f64(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, hex_expr: &ExprHex, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = hex_expr.value.as_str(db);
    let bits = u64::from_str_radix(&parse_hex_str(value_str), 16)?;
    let value = f64::from_bits(bits);
    unsafe {
        *(dest_ptr as *mut f64) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_hex_bigint(rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, hex_expr: &ExprHex, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = hex_expr.value.as_str(db);
    let value = u128::from_str_radix(&parse_hex_str(value_str), 16)?;

    // A hex literal takes no sign.
    let is_negative = false;

    let mut limbs = Vec::new();
    let mut remaining = value;
    while remaining > 0 {
        limbs.push((remaining & 0xFFFFFFFF) as u32);
        remaining >>= 32;
    }

    // Note: For zero, limbs stays empty. Canonical zero has size_and_sign=0, data=null.

    unsafe {
        // Allocate limbs array via runtime.
        let limbs_ptr = if !limbs.is_empty() {
            let ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, 4, 4, (limbs.len() as u32).into()) as *mut u32;
            for (i, &limb) in limbs.iter().enumerate() {
                *ptr.add(i) = limb;
            }
            ptr as *const u32
        } else {
            std::ptr::null()
        };

        let int_ptr = dest_ptr as *mut rtdt::Int;
        (*int_ptr).data = limbs_ptr;
        (*int_ptr).size_and_sign = if is_negative {
            -(limbs.len() as i32)
        } else {
            limbs.len() as i32
        };
        (*int_ptr).capacity = rtdt::Index(limbs.len() as rtdt::IndexRepr);

        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_bigint(rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: &ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::strip_separators(int_expr.value.as_str(db));
    let value: i128 = value_str.parse()?;

    let abs_value = value.unsigned_abs();
    let is_negative = value < 0;

    let mut limbs = Vec::new();
    let mut remaining = abs_value;
    while remaining > 0 {
        limbs.push((remaining & 0xFFFFFFFF) as u32);
        remaining >>= 32;
    }

    // Note: For zero, limbs stays empty. Canonical zero has size_and_sign=0, data=null.

    unsafe {
        // Allocate limbs array via runtime using size=4, align=4, count=len.
        let limbs_ptr = if !limbs.is_empty() {
            let ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, 4, 4, (limbs.len() as u32).into()) as *mut u32;
            for (i, &limb) in limbs.iter().enumerate() {
                *ptr.add(i) = limb;
            }
            ptr as *const u32
        } else {
            std::ptr::null()
        };

        let int_ptr = dest_ptr as *mut rtdt::Int;
        (*int_ptr).data = limbs_ptr;
        (*int_ptr).size_and_sign = if is_negative {
            -(limbs.len() as i32)
        } else {
            limbs.len() as i32
        };
        (*int_ptr).capacity = rtdt::Index(limbs.len() as rtdt::IndexRepr);

        Ok(dest_ptr as *const u8)
    }
}

// ============================================================================
// String instantiation
// ============================================================================

fn instantiate_string(
    rt: datalove_rt::c::LocalRtHandle,
    db: &dyn crate::Db,
    string_expr: &ExprString,
    string_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = crate::parser_util::string_literal_value(string_expr.value.as_str(db))
        .expect("the parser reads a string's escapes");

    unsafe {
        let string_ptr = dest_ptr as *mut rtdt::String;

        // Create empty string using runtime helper.
        let rt_handle = rt as *mut _ as datalove_rt::c::LocalRtHandle;
        let status = datalove_rt::c::dtlv_rti_string_create_local(
            rt_handle,
            string_ptr as *mut u8,
            string_tydesc,
        );

        if status != datalove_rt::c::RtStatus::Ok {
            bail!("Failed to create string");
        }

        // Push the string data if non-empty.
        if !value_str.is_empty() {
            let status = datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt_handle,
                string_ptr as *mut u8,
                string_tydesc,
                value_str.as_ptr(),
                (value_str.len() as u32).into(),
            );

            if status != datalove_rt::c::RtStatus::Ok {
                bail!("Failed to push string bytes");
            }
        }

        Ok(dest_ptr as *const u8)
    }
}

// ============================================================================
// Compound type instantiation
// ============================================================================

fn instantiate_tuple<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    elements: &[ExprFull<'db>],
    field_types: &[Type<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    tuple_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_tuple_layout(rtdt::TyDescRef::from_ptr(tuple_tydesc)) };

    for (i, (elem, field_ty)) in elements.iter().zip(field_types.iter()).enumerate() {
        let field_offset = layout.field_offsets[i];
        let field_dest = unsafe { dest_ptr.add(field_offset as usize) };
        if let Err(e) = instantiate_expr_into(db, rt, *elem, field_ty, tydesc_table, field_dest, resolved) {
            // Destroy successfully instantiated fields.
            for j in 0..i {
                let prev_field_ty = &field_types[j];
                let prev_field_tydesc = tydesc_table.get_or_create(&*prev_field_ty);
                let prev_field_offset = layout.field_offsets[j];
                let prev_field_dest = unsafe { dest_ptr.add(prev_field_offset as usize) };
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, prev_field_dest, prev_field_tydesc);
                }
            }
            return Err(e);
        }
    }

    Ok(dest_ptr as *const u8)
}

fn instantiate_struct<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    expr_fields: &[ExprStructField<'db>],
    type_fields: &[TypeNamedField<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    struct_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_struct_layout(rtdt::TyDescRef::from_ptr(struct_tydesc)) };

    // For small structs, linear search is faster than HashMap allocation.
    const SMALL_STRUCT_THRESHOLD: usize = 8;

    if expr_fields.len() <= SMALL_STRUCT_THRESHOLD {
        // Linear search for small structs.
        for (i, type_field) in type_fields.iter().enumerate() {
            let field_name = type_field.name.as_str(db);
            let field_ty = &type_field.ty;

            let field_expr = expr_fields
                .iter()
                .find(|ef| ef.name.as_str(db) == field_name)
                .ok_or_else(|| anyhow!("Missing field: {}", field_name))?
                .value;

            let field_offset = layout.field_offsets[i];
            let field_dest = unsafe { dest_ptr.add(field_offset as usize) };
            if let Err(e) = instantiate_expr_into(db, rt, field_expr, &**field_ty, tydesc_table, field_dest, resolved) {
                // Destroy successfully instantiated fields.
                for j in 0..i {
                    let prev_field_ty = &type_fields[j].ty;
                    let prev_field_tydesc = tydesc_table.get_or_create(&**prev_field_ty);
                    let prev_field_offset = layout.field_offsets[j];
                    let prev_field_dest = unsafe { dest_ptr.add(prev_field_offset as usize) };
                    unsafe {
                        datalove_rt::c::dtlv_rti_any_destroy_local(rt, prev_field_dest, prev_field_tydesc);
                    }
                }
                return Err(e);
            }
        }
    } else {
        // HashMap for large structs.
        let mut field_map: std::collections::HashMap<&str, ExprFull<'db>> = std::collections::HashMap::new();
        for expr_field in expr_fields {
            let name = expr_field.name.as_str(db);
            field_map.insert(name, expr_field.value);
        }

        for (i, type_field) in type_fields.iter().enumerate() {
            let field_name = type_field.name.as_str(db);
            let field_ty = &type_field.ty;

            let field_expr = field_map.get(field_name)
                .ok_or_else(|| anyhow!("Missing field: {}", field_name))?;

            let field_offset = layout.field_offsets[i];
            let field_dest = unsafe { dest_ptr.add(field_offset as usize) };
            if let Err(e) = instantiate_expr_into(db, rt, *field_expr, &**field_ty, tydesc_table, field_dest, resolved) {
                // Destroy successfully instantiated fields.
                for j in 0..i {
                    let prev_field_ty = &type_fields[j].ty;
                    let prev_field_tydesc = tydesc_table.get_or_create(&**prev_field_ty);
                    let prev_field_offset = layout.field_offsets[j];
                    let prev_field_dest = unsafe { dest_ptr.add(prev_field_offset as usize) };
                    unsafe {
                        datalove_rt::c::dtlv_rti_any_destroy_local(rt, prev_field_dest, prev_field_tydesc);
                    }
                }
                return Err(e);
            }
        }
    }

    Ok(dest_ptr as *const u8)
}


fn instantiate_list<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    elements: &[ExprFull<'db>],
    element_type: Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _list_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let element_ty = element_type;
    let element_tydesc = tydesc_table.get_or_create(&element_ty);
    let element_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(element_tydesc) };
    let element_size = element_tydesc_ref.size();

    unsafe {
        // Allocate list data array.
        let data_ptr = if !elements.is_empty() {
            let array_ptr = datalove_rt::c::dtlv_rti_mem_alloc_local(rt, element_tydesc, (elements.len() as u32).into());

            // Try to instantiate all elements. If any fail, clean up and return error.
            for (i, elem) in elements.iter().enumerate() {
                let elem_dest = array_ptr.add(i * element_size as usize);
                if let Err(e) = instantiate_expr_into(db, rt, *elem, &element_ty, tydesc_table, elem_dest, resolved) {
                    // Destroy successfully instantiated elements.
                    for j in 0..i {
                        let elem_to_destroy = array_ptr.add(j * element_size as usize);
                        datalove_rt::c::dtlv_rti_any_destroy_local(rt, elem_to_destroy, element_tydesc);
                    }
                    // Free the array.
                    datalove_rt::c::dtlv_rti_mem_free_local(rt, element_tydesc, (elements.len() as u32).into(), array_ptr);
                    return Err(e);
                }
            }
            array_ptr as *const u8
        } else {
            std::ptr::null()
        };

        let list_ptr = dest_ptr as *mut rtdt::List;
        (*list_ptr).data = data_ptr;
        (*list_ptr).size = (elements.len() as u32).into();
        (*list_ptr).capacity = (elements.len() as u32).into();

        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_table<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    rows: &[ExprTableRow<'db>],
    columns: &[TypeNamedField<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    table_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());

    // Create empty table.
    unsafe {
        let status = datalove_rt::c::dtlv_rti_table_create_local(rt, dest_ptr, table_tydesc);
        if status != datalove_rt::c::RtStatus::Ok {
            return Err(anyhow!("Failed to create table"));
        }
    }

    if rows.is_empty() {
        return Ok(dest_ptr as *const u8);
    }

    // Build a row tuple type descriptor from column types.
    let column_tydescs: Vec<*const rtdt::TyDesc> = columns
        .iter()
        .map(|col| tydesc_table.get_or_create(&*col.ty))
        .collect();
    let row_tuple_tydesc = tydesc_table.get_or_create_tuple(&column_tydescs);
    let row_tuple_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(row_tuple_tydesc) };
    let row_layout = rtdt::layout::compute_tuple_layout(row_tuple_tydesc_ref);

    // Allocate temporary buffer for a single row tuple.
    let row_buffer = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt, row_tuple_tydesc, 1)
    };
    if row_buffer.is_null() {
        // Destroy the table we just created.
        unsafe {
            datalove_rt::c::dtlv_rti_table_destroy_local(rt, dest_ptr, table_tydesc);
        }
        return Err(anyhow!("Failed to allocate row buffer"));
    }

    // Push each row.
    for (row_idx, row) in rows.iter().enumerate() {
        if row.elements.len() != columns.len() {
            // Cleanup and error.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(rt, row_tuple_tydesc, 1, row_buffer);
                datalove_rt::c::dtlv_rti_table_destroy_local(rt, dest_ptr, table_tydesc);
            }
            return Err(anyhow!(
                "Row {} has {} elements but table has {} columns",
                row_idx,
                row.elements.len(),
                columns.len()
            ));
        }

        // Instantiate each cell into the row tuple buffer.
        for (col_idx, (elem, col)) in row.elements.iter().zip(columns.iter()).enumerate() {
            let field_offset = row_layout.field_offsets[col_idx];
            let cell_dest = unsafe { row_buffer.add(field_offset as usize) };
            if let Err(e) = instantiate_expr_into(db, rt, *elem, &*col.ty, tydesc_table, cell_dest, resolved) {
                // Destroy already instantiated cells in this row.
                for cleanup_col in 0..col_idx {
                    let cleanup_offset = row_layout.field_offsets[cleanup_col];
                    let cleanup_dest = unsafe { row_buffer.add(cleanup_offset as usize) };
                    let cleanup_tydesc = column_tydescs[cleanup_col];
                    unsafe {
                        datalove_rt::c::dtlv_rti_any_destroy_local(rt, cleanup_dest, cleanup_tydesc);
                    }
                }
                // Cleanup and return error.
                unsafe {
                    datalove_rt::c::dtlv_rti_mem_free_local(rt, row_tuple_tydesc, 1, row_buffer);
                    datalove_rt::c::dtlv_rti_table_destroy_local(rt, dest_ptr, table_tydesc);
                }
                return Err(e);
            }
        }

        // Push the row to the table.
        unsafe {
            let status = datalove_rt::c::dtlv_rti_table_push_row_local(
                rt,
                dest_ptr,
                table_tydesc,
                row_buffer,
                row_tuple_tydesc,
            );
            if status != datalove_rt::c::RtStatus::Ok {
                // Destroy row cells.
                for (col_idx, col_tydesc) in column_tydescs.iter().enumerate() {
                    let cleanup_offset = row_layout.field_offsets[col_idx];
                    let cleanup_dest = row_buffer.add(cleanup_offset as usize);
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, cleanup_dest, *col_tydesc);
                }
                datalove_rt::c::dtlv_rti_mem_free_local(rt, row_tuple_tydesc, 1, row_buffer);
                datalove_rt::c::dtlv_rti_table_destroy_local(rt, dest_ptr, table_tydesc);
                return Err(anyhow!("Failed to push row {} to table", row_idx));
            }
        }

        // Nothing to destroy here: the push takes the cells, and the buffer is
        // written over by the next row. It used to clone them instead, which
        // left this loop freeing what the table now held.
    }

    // Free row buffer.
    unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(rt, row_tuple_tydesc, 1, row_buffer);
    }

    Ok(dest_ptr as *const u8)
}

fn instantiate_option<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    is_some: bool,
    payload_expr: Option<ExprFull<'db>>,
    inner_type: Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    option_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(option_tydesc)) };

    if is_some {
        unsafe { *dest_ptr = rtdt::OptionTag::Some as u8 };

        let payload = payload_expr.ok_or_else(|| anyhow!("Some variant missing payload"))?;
        let payload_dest = unsafe { dest_ptr.add(layout.payload_offset as usize) };
        instantiate_expr_into(db, rt, payload, &inner_type, tydesc_table, payload_dest, resolved)?;
    } else {
        unsafe { *dest_ptr = rtdt::OptionTag::None as u8 };
    }

    Ok(dest_ptr as *const u8)
}

fn instantiate_result<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    is_ok: bool,
    ok_payload_expr: Option<ExprFull<'db>>,
    err_payload_expr: Option<ExprFull<'db>>,
    ok_type: Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    result_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(result_tydesc)) };

    if is_ok {
        unsafe { *dest_ptr = rtdt::ResultTag::Ok as u8 };

        let payload = ok_payload_expr.ok_or_else(|| anyhow!("Ok variant missing payload"))?;
        let payload_dest = unsafe { dest_ptr.add(layout.payload_offset as usize) };
        instantiate_expr_into(db, rt, payload, &ok_type, tydesc_table, payload_dest, resolved)?;
    } else {
        unsafe { *dest_ptr = rtdt::ResultTag::Err as u8 };

        let err_payload = err_payload_expr.ok_or_else(|| anyhow!("Err variant missing payload"))?;
        let payload_dest = unsafe { dest_ptr.add(layout.payload_offset as usize) };
        // The payload is an `error` expression, and is the error in the slot.
        instantiate_expr_into(db, rt, err_payload, &Type::Error, tydesc_table, payload_dest, resolved)?;
    }

    Ok(dest_ptr as *const u8)
}

fn instantiate_data<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    inner_expr: ExprFull<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _data_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    // Typechecked again for the type it has, which is what the value carries.
    let typechecked = crate::tycheck::type_check(db, inner_expr, resolved);

    let inner_type = typechecked.root_type(db).clone()
        .expect("the typechecker checked the payload");

    let inner_tydesc = tydesc_table.get_or_create(&inner_type);
    let inner_value = instantiate_expr(db, rt, inner_expr, &inner_type, tydesc_table, resolved)?;

    let data_ptr = dest_ptr as *mut rtdt::Data;

    unsafe {
        std::ptr::write(
            data_ptr,
            rtdt::Data::from_pointers(inner_tydesc, inner_value)
        );
    }

    Ok(dest_ptr as *const u8)
}

fn instantiate_error<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    inner_expr: ExprFull<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _error_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    // Typechecked again for the type it has, which is what the value carries.
    let typechecked = crate::tycheck::type_check(db, inner_expr, resolved);

    let inner_type = typechecked.root_type(db).clone()
        .expect("the typechecker checked the payload");

    let inner_tydesc = tydesc_table.get_or_create(&inner_type);
    let inner_value = instantiate_expr(db, rt, inner_expr, &inner_type, tydesc_table, resolved)?;

    let error_ptr = dest_ptr as *mut rtdt::Error;

    unsafe {
        // Error has same layout as Data, so we write it as Data.
        std::ptr::write(
            error_ptr as *mut rtdt::Data,
            rtdt::Data::from_pointers(inner_tydesc, inner_value)
        );
    }

    Ok(dest_ptr as *const u8)
}

// ============================================================================
// Enum instantiation
// ============================================================================

/// Instantiate an atom or term as a variant of an enum.
///
/// The discriminant is the variant's place among the type's variants, which
/// are held in name order, and the payload goes at the variant's offset.
fn instantiate_variant<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    name: bct::text::InternedText<'db>,
    payload: Option<ExprFull<'db>>,
    enum_ty: &TypeEnum<'db>,
    ty: &Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let index = enum_ty.variants.iter().position(|v| v.name == name)
        .ok_or_else(|| anyhow!("Enum has no variant {}", name.as_str(db)))?;
    let enum_tydesc = unsafe { rtdt::TyDescRef::from_ptr(tydesc_table.get_or_create(ty)) };
    let offset = enum_tydesc.enum_info().variant(index).X().offset();

    match (payload, &enum_ty.variants[index].payload) {
        (Some(payload), Some(payload_ty)) => {
            let payload_dest = unsafe { dest_ptr.add(offset as usize) };
            instantiate_expr_into(db, rt, payload, payload_ty, tydesc_table, payload_dest, resolved)?;
        }
        (None, None) => {}
        _ => unreachable!("typechecking matched the variant's payload"),
    }

    unsafe {
        *(dest_ptr as *mut u32) = index as u32;
    }
    Ok(dest_ptr as *const u8)
}

// ============================================================================
// Map instantiation
// ============================================================================

fn instantiate_map<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    map_expr: ExprMap<'db>,
    key_type: Type<'db>,
    value_type: Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _map_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let map_ptr = dest_ptr as *mut rtdt::Map;

    let entries = &map_expr.entries;
    if entries.is_empty() {
        unsafe {
            (*map_ptr).root = std::ptr::null();
            (*map_ptr).len = rtdt::Index::ZERO;
        }
        return Ok(map_ptr as *const u8);
    }

    // Entries go in one at a time, in the order they were written; see
    // `instantiate_set` for why they are not sorted here first.
    let key_tydesc = tydesc_table.get_or_create(&key_type);
    let value_tydesc = tydesc_table.get_or_create(&value_type);
    let key_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(key_tydesc) };
    let value_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(value_tydesc) };
    let key_size = key_tydesc_ref.size() as usize;
    let value_size = value_tydesc_ref.size() as usize;
    let key_align = key_tydesc_ref.align() as u32;
    let value_align = value_tydesc_ref.align() as u32;

    unsafe {
        // Allocate buffers for all keys and values.
        let keys_buffer_size = (entries.len() * key_size) as u32;
        let values_buffer_size = (entries.len() * value_size) as u32;

        let keys_buffer = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, keys_buffer_size, key_align, 1);
        if keys_buffer.is_null() {
            return Err(anyhow!("Failed to allocate buffer for map keys"));
        }

        let values_buffer = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, values_buffer_size, value_align, 1);
        if values_buffer.is_null() {
            datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, keys_buffer_size, key_align, 1, keys_buffer);
            return Err(anyhow!("Failed to allocate buffer for map values"));
        }

        // Track how many entries we've successfully instantiated (for cleanup on error).
        let mut instantiated_count = 0usize;

        // Instantiate entries into the buffers in the order they were written.
        for (i, entry) in entries.iter().enumerate() {
            let key_dest = keys_buffer.add(i * key_size);
            let value_dest = values_buffer.add(i * value_size);

            // Try to instantiate key.
            if let Err(e) = instantiate_expr_into(db, rt, entry.key, &key_type, tydesc_table, key_dest, resolved) {
                // Cleanup: destroy already-instantiated entries.
                for j in 0..instantiated_count {
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, keys_buffer.add(j * key_size), key_tydesc);
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, values_buffer.add(j * value_size), value_tydesc);
                }
                datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, keys_buffer_size, key_align, 1, keys_buffer);
                datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, values_buffer_size, value_align, 1, values_buffer);
                return Err(e);
            }

            // Try to instantiate value.
            if let Err(e) = instantiate_expr_into(db, rt, entry.value, &value_type, tydesc_table, value_dest, resolved) {
                // Destroy the key we just instantiated.
                datalove_rt::c::dtlv_rti_any_destroy_local(rt, key_dest, key_tydesc);
                for j in 0..instantiated_count {
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, keys_buffer.add(j * key_size), key_tydesc);
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, values_buffer.add(j * value_size), value_tydesc);
                }
                datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, keys_buffer_size, key_align, 1, keys_buffer);
                datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, values_buffer_size, value_align, 1, values_buffer);
                return Err(e);
            }

            instantiated_count += 1;
        }

        // By key, in the runtime's order; see `instantiate_set`.
        let sorted = sort_instantiated(rt, keys_buffer, entries.len(), key_size, key_tydesc);
        let sorted_keys = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, keys_buffer_size, key_align, 1);
        let sorted_values = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, values_buffer_size, value_align, 1);
        if sorted_keys.is_null() || sorted_values.is_null() {
            for j in 0..entries.len() {
                datalove_rt::c::dtlv_rti_any_destroy_local(rt, keys_buffer.add(j * key_size), key_tydesc);
                datalove_rt::c::dtlv_rti_any_destroy_local(rt, values_buffer.add(j * value_size), value_tydesc);
            }
            datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, keys_buffer_size, key_align, 1, keys_buffer);
            datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, values_buffer_size, value_align, 1, values_buffer);
            return Err(anyhow!("Failed to allocate sorted buffers for map entries"));
        }
        // A map holds each key once, and where the literal wrote one twice the
        // later entry is the one it has. The sort is stable, so equal keys are
        // neighbours in the order they were written and the last of a run is
        // the one to keep; the entry it displaces is let go of, key and value
        // both, rather than merely skipped.
        let mut kept = 0usize;
        for (i, &from) in sorted.iter().enumerate() {
            let key_src = keys_buffer.add(from * key_size);
            let value_src = values_buffer.add(from * value_size);
            if i > 0 && same_value(rt, sorted_keys.add((kept - 1) * key_size), key_src, key_tydesc) {
                datalove_rt::c::dtlv_rti_any_destroy_local(
                    rt, sorted_keys.add((kept - 1) * key_size), key_tydesc);
                datalove_rt::c::dtlv_rti_any_destroy_local(
                    rt, sorted_values.add((kept - 1) * value_size), value_tydesc);
                kept -= 1;
            }
            std::ptr::copy_nonoverlapping(key_src, sorted_keys.add(kept * key_size), key_size);
            std::ptr::copy_nonoverlapping(value_src, sorted_values.add(kept * value_size), value_size);
            kept += 1;
        }
        datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, keys_buffer_size, key_align, 1, keys_buffer);
        datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, values_buffer_size, value_align, 1, values_buffer);

        // Build the B-tree using the runtime function (takes ownership of buffer contents).
        let status = datalove_rt::c::dtlv_rti_btreemap_build_from_sorted_slices_local(
            rt,
            map_ptr as *mut u8,
            key_tydesc,
            value_tydesc,
            sorted_keys,
            sorted_values,
            (kept as u32).into(),
        );

        // Free the buffer memory (data has been moved to the tree).
        datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, keys_buffer_size, key_align, 1, sorted_keys);
        datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, values_buffer_size, value_align, 1, sorted_values);

        if status != datalove_rt::c::RtStatus::Ok {
            return Err(anyhow!("Failed to build map B-tree"));
        }
    }

    Ok(map_ptr as *const u8)
}

// ============================================================================
// Set instantiation
// ============================================================================

fn instantiate_set<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    set_expr: ExprSet<'db>,
    element_type: Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _set_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let set_ptr = dest_ptr as *mut rtdt::Set;

    let elements = &set_expr.elements;
    if elements.is_empty() {
        unsafe {
            (*set_ptr).root = std::ptr::null();
            (*set_ptr).len = rtdt::Index::ZERO;
        }
        return Ok(set_ptr as *const u8);
    }

    let element_tydesc = tydesc_table.get_or_create(&element_type);
    let element_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(element_tydesc) };
    let element_size = element_tydesc_ref.size() as usize;
    let element_align = element_tydesc_ref.align() as u32;

    unsafe {
        // Allocate buffer for all elements.
        let buffer_size = (elements.len() * element_size) as u32;
        let buffer = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, buffer_size, element_align, 1);
        if buffer.is_null() {
            return Err(anyhow!("Failed to allocate buffer for set elements"));
        }

        // Track how many elements we've successfully instantiated (for cleanup on error).
        let mut instantiated_count = 0usize;

        // Instantiate elements into the buffer in the order they were written.
        for (i, elem) in elements.iter().enumerate() {
            let elem_dest = buffer.add(i * element_size);
            if let Err(e) = instantiate_expr_into(db, rt, *elem, &element_type, tydesc_table, elem_dest, resolved) {
                // Cleanup: destroy already-instantiated elements.
                for j in 0..instantiated_count {
                    let elem_to_destroy = buffer.add(j * element_size);
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, elem_to_destroy, element_tydesc);
                }
                // Free the buffer.
                datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, buffer_size, element_align, 1, buffer);
                return Err(e);
            }
            instantiated_count += 1;
        }

        // Sorted by the runtime's own comparator, which is what the tree this
        // run is about to become uses to decide where anything sits.
        //
        // The order used to be worked out from the expressions instead, by a
        // second comparison written over the AST in `canon`. Nothing held the
        // two to one answer, and they differed: over how a `data` orders
        // against another holding a different type, over sequences of unequal
        // length, over a struct's fields, and over tables, which `canon` gave
        // a type tag and no comparison at all, so that a set of two of them
        // reached an `unreachable!`. Where they differed, the run handed to
        // build-from-sorted-slice was not sorted by the comparator that
        // believed it was, and the tree came out holding values it could no
        // longer find.
        let sorted = sort_instantiated(rt, buffer, elements.len(), element_size, element_tydesc);
        let sorted_buffer = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, buffer_size, element_align, 1);
        if sorted_buffer.is_null() {
            for j in 0..elements.len() {
                datalove_rt::c::dtlv_rti_any_destroy_local(rt, buffer.add(j * element_size), element_tydesc);
            }
            datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, buffer_size, element_align, 1, buffer);
            return Err(anyhow!("Failed to allocate sorted buffer for set elements"));
        }
        // A set holds each element once, however the literal wrote them. The
        // builder takes the run as given, so the repeats are let go of here --
        // equal elements are neighbours now that the run is sorted.
        let mut kept = 0usize;
        for (i, &from) in sorted.iter().enumerate() {
            let src = buffer.add(from * element_size);
            if i > 0 && same_value(rt, sorted_buffer.add((kept - 1) * element_size), src, element_tydesc) {
                datalove_rt::c::dtlv_rti_any_destroy_local(rt, src, element_tydesc);
                continue;
            }
            // Moves, not copies: the bytes are handed over and the old run is
            // left to be freed without being destroyed.
            std::ptr::copy_nonoverlapping(src, sorted_buffer.add(kept * element_size), element_size);
            kept += 1;
        }
        datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, buffer_size, element_align, 1, buffer);

        // Build the B-tree using the runtime function (takes ownership of buffer contents).
        let status = datalove_rt::c::dtlv_rti_btreeset_build_from_sorted_slice_local(
            rt,
            set_ptr as *mut u8,
            element_tydesc,
            sorted_buffer,
            (kept as u32).into(),
        );

        // Free the buffer memory (elements have been moved to the tree).
        datalove_rt::c::dtlv_rti_mem_free_raw_local(rt, buffer_size, element_align, 1, sorted_buffer);

        if status != datalove_rt::c::RtStatus::Ok {
            return Err(anyhow!("Failed to build set B-tree"));
        }
    }

    Ok(set_ptr as *const u8)
}

/// Whether two instantiated values are the one value, as the tree would judge.
///
/// A set holds each element once and a map each key once, and which elements
/// are the same one is the tree's question rather than the literal's: two
/// strings written differently can still be one key.
unsafe fn same_value(
    rt: datalove_rt::c::LocalRtHandle,
    a: *const u8,
    b: *const u8,
    tydesc: *const rtdt::TyDesc,
) -> bool {
    unsafe {
        datalove_rt::c::dtlv_rti_cmp_total_local(rt, a, tydesc, b, tydesc)
            == datalove_rt::c::RtOrdering::Equal
    }
}

/// Order instantiated values by the runtime's comparison, returning where each
/// belongs rather than moving anything.
///
/// A set or a map literal is built by handing a sorted run of values to the
/// runtime, which takes the order on trust, so the order has to be the one its
/// own comparator would give. Asking it is the only way to be sure of that.
unsafe fn sort_instantiated(
    rt: datalove_rt::c::LocalRtHandle,
    buffer: *const u8,
    count: usize,
    stride: usize,
    tydesc: *const rtdt::TyDesc,
) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| unsafe {
        match datalove_rt::c::dtlv_rti_cmp_total_local(
            rt,
            buffer.add(a * stride),
            tydesc,
            buffer.add(b * stride),
            tydesc,
        ) {
            datalove_rt::c::RtOrdering::Less => std::cmp::Ordering::Less,
            datalove_rt::c::RtOrdering::Greater => std::cmp::Ordering::Greater,
            datalove_rt::c::RtOrdering::Equal => std::cmp::Ordering::Equal,
            // Two values of one type always compare, and everything in one of
            // these runs has one type.
            datalove_rt::c::RtOrdering::Error => unreachable!("cmp of two values of one type"),
        }
    });
    order
}

fn instantiate_tensor<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    shape: &[u32],
    elements: &[ExprFull<'db>],
    element_type: Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _tensor_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
    resolved: ResolvedExpr<'db>,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let element_ty = element_type;
    let element_tydesc = tydesc_table.get_or_create(&element_ty);
    let element_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(element_tydesc) };
    let element_size = element_tydesc_ref.size();

    let rank = shape.len();
    let total_elems: usize = shape.iter().map(|&d| d as usize).product();

    unsafe {
        // Allocate tensor data array.
        let data_ptr = if total_elems > 0 {
            let array_ptr = datalove_rt::c::dtlv_rti_mem_alloc_local(rt, element_tydesc, (total_elems as u32).into());

            // Try to instantiate all elements. If any fail, clean up and return error.
            for (i, elem) in elements.iter().enumerate() {
                let elem_dest = array_ptr.add(i * element_size as usize);
                if let Err(e) = instantiate_expr_into(db, rt, *elem, &element_ty, tydesc_table, elem_dest, resolved) {
                    // Destroy successfully instantiated elements.
                    for j in 0..i {
                        let elem_to_destroy = array_ptr.add(j * element_size as usize);
                        datalove_rt::c::dtlv_rti_any_destroy_local(rt, elem_to_destroy, element_tydesc);
                    }
                    // Free the array.
                    datalove_rt::c::dtlv_rti_mem_free_local(rt, element_tydesc, (total_elems as u32).into(), array_ptr);
                    return Err(e);
                }
            }
            array_ptr
        } else {
            std::ptr::null_mut()
        };

        // Allocate shape array (IndexRepr per dimension). A tensor has at
        // least one axis, and keeps its shape even when it holds nothing.
        assert!(rank > 0, "a tensor has at least one axis");
        let shape_ptr = {
            let shape_array = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, (rank as u32).into()) as *mut rtdt::IndexRepr;
            for (i, &dim) in shape.iter().enumerate() {
                *shape_array.add(i) = dim as rtdt::IndexRepr;
            }
            shape_array as *const rtdt::Index
        };

        // Allocate and compute strides array (IndexRepr per dimension).
        let strides_ptr = {
            let strides_array = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, (rank as u32).into()) as *mut rtdt::IndexRepr;

            // Compute strides for row-major layout.
            // RowMajor: strides[i] = product of dims[i+1..rank].
            for i in 0..rank {
                let stride: rtdt::IndexRepr = shape[i+1..rank].iter().map(|&d| d as rtdt::IndexRepr).product();
                *strides_array.add(i) = if stride == 0 { 1 } else { stride };
            }

            strides_array as *const rtdt::Index
        };

        // Always use row-major layout for tensor literals.
        let rtdt_layout = rtdt::TensorLayout::RowMajor;

        // Fill in the Tensor struct.
        let tensor_ptr = dest_ptr as *mut rtdt::Tensor;
        (*tensor_ptr).ptr_base = data_ptr;
        (*tensor_ptr).capacity_elems = rtdt::Index(total_elems as rtdt::IndexRepr);
        (*tensor_ptr).offset_elems = rtdt::Index::ZERO;
        (*tensor_ptr).shape = shape_ptr;
        (*tensor_ptr).strides = strides_ptr;
        (*tensor_ptr).layout = rtdt_layout;

        Ok(dest_ptr as *const u8)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    /// RAII guard for Runtime to prevent memory leaks on panic.
    struct RtGuard {
        rt: datalove_rt::rust::Runtime,
    }

    impl RtGuard {
        fn new(rt: datalove_rt::rust::Runtime) -> Self {
            Self { rt }
        }

        fn handle(&self) -> datalove_rt::c::LocalRtHandle {
            self.rt.handle()
        }
    }

    /// RAII guard for InstantiatedValue to ensure proper cleanup.
    struct InstGuard<'a> {
        rt_handle: datalove_rt::c::LocalRtHandle,
        inst: InstantiatedValue<'a>,
    }

    impl<'a> InstGuard<'a> {
        fn new(rt_handle: datalove_rt::c::LocalRtHandle, inst: InstantiatedValue<'a>) -> Self {
            Self { rt_handle, inst }
        }

        fn value(&self) -> &InstantiatedValue<'a> {
            &self.inst
        }
    }

    impl<'a> Drop for InstGuard<'a> {
        fn drop(&mut self) {
            unsafe {
                datalove_rt::c::dtlv_rti_any_destroy_local(
                    self.rt_handle,
                    self.inst.ptr as *mut u8,
                    self.inst.tydesc.as_ptr(),
                );
                datalove_rt::c::dtlv_rti_mem_free_local(
                    self.rt_handle,
                    self.inst.tydesc.as_ptr(),
                    1,
                    self.inst.ptr as *mut u8,
                );
            }
        }
    }

    fn compile_str<'db>(db: &'db Database, source_text: &str) -> AnyResult<TypecheckResult<'db>> {
        let source = bct::input::Source::new(db, source_text.to_string());
        let parsed = crate::parser::parse_for_test(db, source);
        let resolved = crate::resolve::resolve_names(db, source, parsed);
        let typechecked = crate::tycheck::type_check(db, parsed, resolved);
        Ok(typechecked)
    }

    #[test]
    fn test_instantiate_bool_true() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, "true")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Bool);
            assert_eq!(*inst.ptr, 1);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_bool_false() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, "false")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Bool);
            assert_eq!(*inst.ptr, 0);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": u32 / 42")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::U32);
            assert_eq!(*(inst.ptr as *const u32), 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_f32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": f32 / 3.14")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::F32);
            assert_eq!(*(inst.ptr as *const f32), 3.14);
        }
        Ok(())
    }

    /// A literal with nothing to infer from is an f64, so that a default
    /// nobody asked for keeps what was written.
    #[test]
    fn test_instantiate_float_default() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, "3.14")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::F64);
            assert_eq!(*(inst.ptr as *const f64), 3.14);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, r#""hello""#)?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::String);
            let string = &*(inst.ptr as *const rtdt::String);
            assert_eq!(string.size, rtdt::Index(5));
            assert!(string.capacity >= rtdt::Index(5));
            let str_slice = std::slice::from_raw_parts(string.data, string.size.as_usize());
            assert_eq!(str_slice, b"hello");
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_empty_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, r#""""#)?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::String);
            let string = &*(inst.ptr as *const rtdt::String);
            assert_eq!(string.size, rtdt::Index::ZERO);
            assert_eq!(string.capacity, rtdt::Index::ZERO);
            assert!(string.data.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tuple_simple() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": (bool, u32) / (true, 42)")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tuple);
            let tuple_info = inst.tydesc.tuple_info();
            assert_eq!(tuple_info.num_fields(), 2);

            let fields = tuple_info.fields();
            let bool_value = *(inst.ptr.add(fields[0].offset as usize));
            assert_eq!(bool_value, 1);

            let u32_value = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(u32_value, 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_small() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": int / 42")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Int);
            let int = &*(inst.ptr as *const rtdt::Int);
            assert_eq!(int.size_and_sign, 1);
            assert_eq!(int.capacity, rtdt::Index(1));
            let limbs = std::slice::from_raw_parts(int.data, 1);
            assert_eq!(limbs[0], 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_zero() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": int / 0")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Int);
            let int = &*(inst.ptr as *const rtdt::Int);
            // Canonical zero: no limbs, size_and_sign=0, data=null.
            assert_eq!(int.size_and_sign, 0);
            assert_eq!(int.capacity, rtdt::Index::ZERO);
            assert!(int.data.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_anon_struct() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": {x: u32, y: u32} / {x = 1, y = 2}")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Struct);
            let struct_info = inst.tydesc.struct_info();
            assert_eq!(struct_info.num_fields(), 2);

            let fields = struct_info.fields();

            let field0_name = std::slice::from_raw_parts(fields[0].name, fields[0].name_len as usize);
            assert_eq!(field0_name, b"x");

            let field1_name = std::slice::from_raw_parts(fields[1].name, fields[1].name_len as usize);
            assert_eq!(field1_name, b"y");

            let x_value = *(inst.ptr.add(fields[0].offset as usize) as *const u32);
            assert_eq!(x_value, 1);

            let y_value = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(y_value, 2);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_list_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": [u32] / [1, 2, 3, 4, 5]")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, rtdt::Index(5));
            assert_eq!(list.capacity, rtdt::Index(5));

            let element_tydesc = inst.tydesc.list_element_ty().as_ptr();
            assert!(!element_tydesc.is_null());
            assert_eq!((*element_tydesc).type_tag, rtdt::TyTag::U32);

            let elements = std::slice::from_raw_parts(list.data as *const u32, list.size.as_usize());
            assert_eq!(elements, &[1, 2, 3, 4, 5]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_empty_list() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": [u32] / []")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, rtdt::Index::ZERO);
            assert_eq!(list.capacity, rtdt::Index::ZERO);
            assert!(list.data.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?u32 / none")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_some_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?u32 / some 42")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize) as *const u32;
            assert_eq!(*payload_ptr, 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_list_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, r#"["hello", "world"]"#)?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, rtdt::Index(2));

            let element_tydesc = inst.tydesc.list_element_ty().as_ptr();
            assert_eq!((*element_tydesc).type_tag, rtdt::TyTag::String);

            let strings = std::slice::from_raw_parts(list.data as *const rtdt::String, list.size.as_usize());

            assert_eq!(strings[0].size, rtdt::Index(5));
            let str0 = std::slice::from_raw_parts(strings[0].data, strings[0].size.as_usize());
            assert_eq!(str0, b"hello");

            assert_eq!(strings[1].size, rtdt::Index(5));
            let str1 = std::slice::from_raw_parts(strings[1].data, strings[1].size.as_usize());
            assert_eq!(str1, b"world");
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_nested_option_some_some() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ??u32 / some some 42")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let outer_tag = *inst.ptr;
            assert_eq!(outer_tag, rtdt::OptionTag::Some as u8);

            let outer_layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let outer_payload_ptr = inst.ptr.add(outer_layout.payload_offset as usize);

            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Option);
            let inner_tag = *outer_payload_ptr;
            assert_eq!(inner_tag, rtdt::OptionTag::Some as u8);

            let inner_layout = rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(inner_tydesc));
            let inner_payload_ptr = outer_payload_ptr.add(inner_layout.payload_offset as usize);
            let value = *(inner_payload_ptr as *const u32);
            assert_eq!(value, 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_large() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": int / 1234567890123456789")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Int);
            let int = &*(inst.ptr as *const rtdt::Int);
            assert!(int.size_and_sign > 0);
            let num_limbs = int.size_and_sign as usize;
            let limbs = std::slice::from_raw_parts(int.data, num_limbs);

            let mut reconstructed: u64 = 0;
            for (i, &limb) in limbs.iter().enumerate() {
                reconstructed |= (limb as u64) << (32 * i);
            }
            assert_eq!(reconstructed, 1234567890123456789);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_list_of_tuples() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": [(u32, u32)] / [(1, 2), (3, 4), (5, 6)]")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, rtdt::Index(3));

            let element_tydesc = inst.tydesc.list_element_ty().as_ptr();
            assert!(!element_tydesc.is_null());
            let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
            assert_eq!(element_tydesc_ref.type_tag(), rtdt::TyTag::Tuple);

            let tuple_info = &(*element_tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let tuple_fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let tuple_size = element_tydesc_ref.size() as usize;

            for i in 0..3 {
                let tuple_ptr = list.data.add(i * tuple_size);
                let first = *(tuple_ptr.add(tuple_fields[0].offset as usize) as *const u32);
                let second = *(tuple_ptr.add(tuple_fields[1].offset as usize) as *const u32);

                assert_eq!(first, (i * 2 + 1) as u32);
                assert_eq!(second, (i * 2 + 2) as u32);
            }
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_tuple_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?(u32, u32) / none")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*inner_tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_tuple_some() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?(u32, u32) / some (10, 20)")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*inner_tydesc).type_info.tuple;
            let tuple_fields = std::slice::from_raw_parts(tuple_info.fields, 2);

            let first = *(payload_ptr.add(tuple_fields[0].offset as usize) as *const u32);
            let second = *(payload_ptr.add(tuple_fields[1].offset as usize) as *const u32);
            assert_eq!(first, 10);
            assert_eq!(second, 20);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_struct_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?{x: u32, y: u32} / none")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*inner_tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_struct_some() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?{x: u32, y: u32} / some {x = 100, y = 200}")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*inner_tydesc).type_info.struct_;
            let struct_fields = std::slice::from_raw_parts(struct_info.fields, struct_info.num_fields as usize);

            let x_value = *(payload_ptr.add(struct_fields[0].offset as usize) as *const u32);
            let y_value = *(payload_ptr.add(struct_fields[1].offset as usize) as *const u32);
            assert_eq!(x_value, 100);
            assert_eq!(y_value, 200);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_list_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?[u32] / none")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::List);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_list_some_empty() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?[u32] / some []")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            let list = &*(payload_ptr as *const rtdt::List);
            assert_eq!(list.size, rtdt::Index::ZERO);
            assert_eq!(list.capacity, rtdt::Index::ZERO);
            assert!(list.data.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_list_some_nonempty() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?[u32] / some [1, 2, 3]")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            let list = &*(payload_ptr as *const rtdt::List);
            assert_eq!(list.size, rtdt::Index(3));
            assert_eq!(list.capacity, rtdt::Index(3));

            let elements = std::slice::from_raw_parts(list.data as *const u32, list.size.as_usize());
            assert_eq!(elements, &[1, 2, 3]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_string_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?string / none")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::String);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_nested_option_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ??u32 / none")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Option);

            let innermost_tydesc = (*inner_tydesc).type_info.option.inner_tydesc;
            assert_eq!((*innermost_tydesc).type_tag, rtdt::TyTag::U32);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_nested_option_some_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ??u32 / none")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_option_of_tuple() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ??(u32, bool) / some some (5, true)")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let outer_tag = *inst.ptr;
            assert_eq!(outer_tag, rtdt::OptionTag::Some as u8);

            let outer_layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let outer_payload_ptr = inst.ptr.add(outer_layout.payload_offset as usize);

            let inner_opt_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_opt_tydesc).type_tag, rtdt::TyTag::Option);
            let inner_tag = *outer_payload_ptr;
            assert_eq!(inner_tag, rtdt::OptionTag::Some as u8);

            let inner_layout = rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(inner_opt_tydesc));
            let inner_payload_ptr = outer_payload_ptr.add(inner_layout.payload_offset as usize);

            let tuple_tydesc = (*inner_opt_tydesc).type_info.option.inner_tydesc;
            assert_eq!((*tuple_tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*tuple_tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let tuple_fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let u32_value = *(inner_payload_ptr.add(tuple_fields[0].offset as usize) as *const u32);
            let bool_value = *(inner_payload_ptr.add(tuple_fields[1].offset as usize) as *const u8);
            assert_eq!(u32_value, 5);
            assert_eq!(bool_value, 1);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_result_ok_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": !u32 / ok 42")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Result);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::ResultTag::Ok as u8);

            let layout = rtdt::layout::compute_result_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize) as *const u32;
            assert_eq!(*payload_ptr, 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_result_err() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": !u32 / er error \"oops\"")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Result);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::ResultTag::Err as u8);

            let layout = rtdt::layout::compute_result_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            // The error payload is an Error type (which has same layout as Data).
            let error_ptr = payload_ptr as *const rtdt::Error;
            let inner_tydesc = (*error_ptr).tydesc();
            let inner_value_ptr = (*error_ptr).value_ptr();

            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::String);
            let string_ptr = inner_value_ptr as *const rtdt::String;
            let string_bytes = std::slice::from_raw_parts((*string_ptr).data, (*string_ptr).size.as_usize());
            assert_eq!(string_bytes, b"oops");
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_error() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, "error : u32 / 42")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Error);

            let error_ptr = inst.ptr as *const rtdt::Error;
            let inner_tydesc = (*error_ptr).tydesc();
            let inner_value_ptr = (*error_ptr).value_ptr();

            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::U32);
            let inner_value = *(inner_value_ptr as *const u32);
            assert_eq!(inner_value, 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tensor_2d_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": [|u32, 2|] / [| 1 2 3, 4 5 6 |]")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tensor);

            let tensor = inst.ptr as *const rtdt::Tensor;
            assert_eq!((*tensor).capacity_elems, rtdt::Index(6));
            assert_eq!((*tensor).offset_elems, rtdt::Index::ZERO);
            assert_eq!((*tensor).layout, rtdt::TensorLayout::RowMajor);

            // Check shape [2, 3]
            let shape = std::slice::from_raw_parts((*tensor).shape, 2);
            assert_eq!(shape, &[rtdt::Index(2), rtdt::Index(3)]);

            // Check strides [3, 1] (row-major)
            let strides = std::slice::from_raw_parts((*tensor).strides, 2);
            assert_eq!(strides, &[rtdt::Index(3), rtdt::Index(1)]);

            // Check data [1, 2, 3, 4, 5, 6]
            let data = std::slice::from_raw_parts((*tensor).ptr_base as *const u32, 6);
            assert_eq!(data, &[1, 2, 3, 4, 5, 6]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tensor_1d_f32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": [|f32, 1|] / [| 1.0 2.0 3.0 4.0 5.0 |]")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tensor);

            let tensor = inst.ptr as *const rtdt::Tensor;
            assert_eq!((*tensor).capacity_elems, rtdt::Index(5));
            assert_eq!((*tensor).offset_elems, rtdt::Index::ZERO);
            assert_eq!((*tensor).layout, rtdt::TensorLayout::RowMajor);

            // Check shape [5]
            let shape = std::slice::from_raw_parts((*tensor).shape, 1);
            assert_eq!(shape, &[rtdt::Index(5)]);

            // Check strides [1] (row-major, but for 1D doesn't matter)
            let strides = std::slice::from_raw_parts((*tensor).strides, 1);
            assert_eq!(strides, &[rtdt::Index(1)]);

            // Check data [1.0, 2.0, 3.0, 4.0, 5.0]
            let data = std::slice::from_raw_parts((*tensor).ptr_base as *const f32, 5);
            assert_eq!(data, &[1.0, 2.0, 3.0, 4.0, 5.0]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tensor_rank4() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": [|u32, 4|] / [| 1 2, 3 4,, 5 6, 7 8,,, 9 10, 11 12,, 13 14, 15 16 |]")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tensor);

            let tensor = inst.ptr as *const rtdt::Tensor;
            assert_eq!((*tensor).capacity_elems, rtdt::Index(16));
            assert_eq!((*tensor).offset_elems, rtdt::Index::ZERO);
            assert_eq!((*tensor).layout, rtdt::TensorLayout::RowMajor);

            // Check shape [2, 2, 2, 2]
            let shape = std::slice::from_raw_parts((*tensor).shape, 4);
            assert_eq!(shape, &[rtdt::Index(2), rtdt::Index(2), rtdt::Index(2), rtdt::Index(2)]);

            // Check strides [8, 4, 2, 1] (row-major for 4D)
            let strides = std::slice::from_raw_parts((*tensor).strides, 4);
            assert_eq!(strides, &[rtdt::Index(8), rtdt::Index(4), rtdt::Index(2), rtdt::Index(1)]);

            // Check data
            let data = std::slice::from_raw_parts((*tensor).ptr_base as *const u32, 16);
            assert_eq!(data, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tensor_3d_i32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": [|i32, 3|] / [| 1 2, 3 4,, 5 6, 7 8 |]")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tensor);

            let tensor = inst.ptr as *const rtdt::Tensor;
            assert_eq!((*tensor).capacity_elems, rtdt::Index(8));
            assert_eq!((*tensor).offset_elems, rtdt::Index::ZERO);
            assert_eq!((*tensor).layout, rtdt::TensorLayout::RowMajor);

            // Check shape [2, 2, 2]
            let shape = std::slice::from_raw_parts((*tensor).shape, 3);
            assert_eq!(shape, &[rtdt::Index(2), rtdt::Index(2), rtdt::Index(2)]);

            // Check strides [4, 2, 1] (row-major for 3D)
            let strides = std::slice::from_raw_parts((*tensor).strides, 3);
            assert_eq!(strides, &[rtdt::Index(4), rtdt::Index(2), rtdt::Index(1)]);

            // Check data [1, 2, 3, 4, 5, 6, 7, 8]
            let data = std::slice::from_raw_parts((*tensor).ptr_base as *const i32, 8);
            assert_eq!(data, &[1, 2, 3, 4, 5, 6, 7, 8]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tensor_of_tuples() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": [|(u32, f32), 2|] / [| (1, 1.0) (2, 2.0), (3, 3.0) (4, 4.0) |]")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tensor);

            let tensor = inst.ptr as *const rtdt::Tensor;
            assert_eq!((*tensor).capacity_elems, rtdt::Index(4));
            assert_eq!((*tensor).offset_elems, rtdt::Index::ZERO);
            assert_eq!((*tensor).layout, rtdt::TensorLayout::RowMajor);

            // Check shape [2, 2]
            let shape = std::slice::from_raw_parts((*tensor).shape, 2);
            assert_eq!(shape, &[rtdt::Index(2), rtdt::Index(2)]);

            // Check strides [2, 1]
            let strides = std::slice::from_raw_parts((*tensor).strides, 2);
            assert_eq!(strides, &[rtdt::Index(2), rtdt::Index(1)]);

            // Verify we have tuple elements (just check the tensor structure is correct)
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_list_of_tensors() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": [[|u32, 2|]] / [[| 1 2, 3 4 |], [| 5 6, 7 8 |]]")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::List);

            let list = inst.ptr as *const rtdt::List;
            assert_eq!((*list).size, rtdt::Index(2));

            // Verify both elements are tensors (basic structure check)
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_tensor() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?[|u32, 2|] / some [| 1 2, 3 4 |]")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let _payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            // Verify the payload is a tensor
            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Tensor);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_table_basic() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": {| id: u32, val: u32 |} / {| id, val; 1, 10; 2, 20 |}")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Table);
            let table = &*(inst.ptr as *const rtdt::Table);
            assert_eq!(table.len, rtdt::Index(2));
            assert!(table.capacity >= rtdt::Index(2));
            assert!(!table.data.is_null());

            // Check column tydescs.
            let table_tydesc_ref = rtdt::TyDescRef::from_ptr(inst.tydesc.as_ptr());
            let num_columns = table_tydesc_ref.table_num_columns();
            assert_eq!(num_columns, 2);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_table_empty() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": {| x: u32 |} / {| x |}")?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Table);
            let table = &*(inst.ptr as *const rtdt::Table);
            assert_eq!(table.len, rtdt::Index::ZERO);
            assert_eq!(table.capacity, rtdt::Index::ZERO);
            assert!(table.data.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_table_with_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, r#": {| name: string, age: u32 |} / {| name, age; "Alice", 30; "Bob", 25 |}"#)?;
        let rt = datalove_rt::rust::Runtime::new();
        let guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Table);
            let table = &*(inst.ptr as *const rtdt::Table);
            assert_eq!(table.len, rtdt::Index(2));

            // Verify table tydesc has string and u32 column types.
            let table_tydesc_ref = rtdt::TyDescRef::from_ptr(inst.tydesc.as_ptr());
            let col_infos: Vec<_> = table_tydesc_ref.table_column_tydescs().collect();
            assert_eq!(col_infos.len(), 2);
            assert_eq!(col_infos[0].tydesc().type_tag(), rtdt::TyTag::String);
            assert_eq!(col_infos[1].tydesc().type_tag(), rtdt::TyTag::U32);
        }
        Ok(())
    }
}
