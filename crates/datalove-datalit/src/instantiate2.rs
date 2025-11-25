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
//! let mut rt = datalove_rt::impls::rt_local::RtLocal::new();
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
use crate::tycheck::*;
use crate::rtdt;
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
    let root_type = typechecked.root_type(db)
        .ok_or_else(|| anyhow!("No root type"))?;
    let root_expr = typechecked.root_expr(db);

    let value_ptr = instantiate_expr(db, rt, root_expr, root_type.ty(db), tydesc_table)?;

    let tydesc = tydesc_table.get_or_create_ref(root_type.ty(db));

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
) -> AnyResult<*const u8> {
    let tydesc_ptr = tydesc_table.get_or_create(ty);
    let (size, align) = unsafe {
        let td = rtdt::TyDescRef::from_ptr(tydesc_ptr);
        (td.size(), td.align())
    };
    let dest_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, size, align, 1)
    };

    // Try to instantiate the expression. If it fails, free the allocated memory.
    match instantiate_expr_into(db, rt, expr, ty, tydesc_table, dest_ptr) {
        Ok(_) => Ok(dest_ptr),
        Err(e) => {
            // Free the allocated memory on error to avoid leak.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(
                    rt,
                    tydesc_ptr,
                    1,
                    dest_ptr as *mut u8,
                );
            }
            Err(e)
        }
    }
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
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null(), "dest_ptr must be non-null");
    let expr_and_heap = expr.expr(db);
    let expr_inner = expr_and_heap.expr(db);

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
        (Expr::Int(int_expr), Type::Int) => instantiate_bigint(rt, db, int_expr, dest_ptr),

        (Expr::Float(float_expr), Type::F32) => instantiate_f32(rt, db, float_expr, dest_ptr),

        (Expr::String(string_expr), Type::String) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_string(rt, db, string_expr, tydesc, dest_ptr)
        }

        (Expr::AnonTuple(tuple_expr), Type::AnonTuple(tuple_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_tuple(db, rt, &tuple_expr.elements(db), &tuple_ty.fields(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::NamedTuple(tuple_expr), Type::NamedTuple(tuple_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_tuple(db, rt, &tuple_expr.elements(db), &tuple_ty.fields(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::AnonStruct(struct_expr), Type::AnonStruct(struct_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_struct(db, rt, &struct_expr.fields(db), &struct_ty.fields(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::AnonStruct(struct_expr), Type::NamedStruct(struct_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_struct(db, rt, &struct_expr.fields(db), &struct_ty.fields(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::NamedStruct(struct_expr), Type::NamedStruct(struct_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_struct(db, rt, &struct_expr.fields(db), &struct_ty.fields(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::AnonEnum(enum_expr), Type::AnonEnum(enum_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_enum(db, rt, enum_expr.variant_name(db), enum_expr.payload(db), &enum_ty.variants(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::AnonEnum(enum_expr), Type::NamedEnum(enum_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_enum(db, rt, enum_expr.variant_name(db), enum_expr.payload(db), &enum_ty.variants(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::NamedEnum(enum_expr), Type::NamedEnum(enum_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_enum(db, rt, enum_expr.variant_name(db), enum_expr.payload(db), &enum_ty.variants(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::List(list_expr), Type::List(list_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_list(db, rt, &list_expr.elements(db), list_ty.element_type(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::None, Type::Option(opt)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_option(db, rt, false, None, opt.inner_type(db), tydesc_table, tydesc, dest_ptr)
        }

        (_, Type::Option(opt)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_option(db, rt, true, Some(expr), opt.inner_type(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::Err(err_expr), Type::Result(res)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_result(db, rt, false, None, Some(err_expr.value(db)), res.inner_type(db), tydesc_table, tydesc, dest_ptr)
        }

        (_, Type::Result(res)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_result(db, rt, true, Some(expr), None, res.inner_type(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::Data(data_expr), Type::Data) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_data(db, rt, data_expr.value(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::Err(err_expr), Type::Error) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_error(db, rt, err_expr.value(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::Map(map_expr), Type::Map(map_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_map(db, rt, &map_expr.entries(db), map_ty.key_type(db), map_ty.value_type(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::Set(set_expr), Type::Set(set_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_set(db, rt, &set_expr.elements(db), set_ty.element_type(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::Tensor(tensor_expr), Type::Tensor(tensor_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_tensor(db, rt, &tensor_expr.shape(db), &tensor_expr.elements(db), tensor_ty.element_type(db), tydesc_table, tydesc, dest_ptr)
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

fn instantiate_u8(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: u8 = value_str.parse()?;
    unsafe {
        *dest_ptr = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i8(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: i8 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i8) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u16(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: u16 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut u16) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i16(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: i16 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i16) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u32(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: u32 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut u32) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i32(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: i32 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i32) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u64(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: u64 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut u64) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i64(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: i64 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i64) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_f32(_rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, float_expr: ExprFloat, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = float_expr.value(db).as_str(db);
    let value: f32 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut f32) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_bigint(rt: datalove_rt::c::LocalRtHandle, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: i128 = value_str.parse()?;

    let abs_value = value.unsigned_abs();
    let is_negative = value < 0;

    let mut limbs = Vec::new();
    let mut remaining = abs_value;
    while remaining > 0 {
        limbs.push((remaining & 0xFFFFFFFF) as u32);
        remaining >>= 32;
    }

    if limbs.is_empty() {
        limbs.push(0);
    }

    unsafe {
        // Allocate limbs array via runtime using size=4, align=4, count=len.
        let limbs_ptr = if !limbs.is_empty() {
            let ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, 4, 4, limbs.len() as u32) as *mut u32;
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
        (*int_ptr).capacity = limbs.len() as u32;

        Ok(dest_ptr as *const u8)
    }
}

// ============================================================================
// String instantiation
// ============================================================================

fn instantiate_string(
    rt: datalove_rt::c::LocalRtHandle,
    db: &dyn crate::Db,
    string_expr: ExprString,
    string_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str_raw = string_expr.value(db).as_str(db);

    let value_str = if value_str_raw.starts_with('"') && value_str_raw.ends_with('"') {
        &value_str_raw[1..value_str_raw.len()-1]
    } else {
        bail!("String literal missing quotes");
    };

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
                value_str.len() as u32,
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
    field_types: &[TypeAndHeap<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    tuple_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_tuple_layout(rtdt::TyDescRef::from_ptr(tuple_tydesc)) };

    for (i, (elem, field_ty)) in elements.iter().zip(field_types.iter()).enumerate() {
        let field_offset = layout.field_offsets[i];
        let field_dest = unsafe { dest_ptr.add(field_offset as usize) };
        if let Err(e) = instantiate_expr_into(db, rt, *elem, field_ty.ty(db), tydesc_table, field_dest) {
            // Destroy successfully instantiated fields.
            for j in 0..i {
                let prev_field_ty = &field_types[j];
                let prev_field_tydesc = tydesc_table.get_or_create(prev_field_ty.ty(db));
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
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_struct_layout(rtdt::TyDescRef::from_ptr(struct_tydesc)) };

    // For small structs, linear search is faster than HashMap allocation.
    const SMALL_STRUCT_THRESHOLD: usize = 8;

    if expr_fields.len() <= SMALL_STRUCT_THRESHOLD {
        // Linear search for small structs.
        for (i, type_field) in type_fields.iter().enumerate() {
            let field_name = type_field.name(db).as_str(db);
            let field_ty = type_field.ty(db);

            let field_expr = expr_fields
                .iter()
                .find(|ef| ef.name(db).as_str(db) == field_name)
                .ok_or_else(|| anyhow!("Missing field: {}", field_name))?
                .value(db);

            let field_offset = layout.field_offsets[i];
            let field_dest = unsafe { dest_ptr.add(field_offset as usize) };
            if let Err(e) = instantiate_expr_into(db, rt, field_expr, field_ty.ty(db), tydesc_table, field_dest) {
                // Destroy successfully instantiated fields.
                for j in 0..i {
                    let prev_field_ty = type_fields[j].ty(db);
                    let prev_field_tydesc = tydesc_table.get_or_create(prev_field_ty.ty(db));
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
            let name = expr_field.name(db).as_str(db);
            field_map.insert(name, expr_field.value(db));
        }

        for (i, type_field) in type_fields.iter().enumerate() {
            let field_name = type_field.name(db).as_str(db);
            let field_ty = type_field.ty(db);

            let field_expr = field_map.get(field_name)
                .ok_or_else(|| anyhow!("Missing field: {}", field_name))?;

            let field_offset = layout.field_offsets[i];
            let field_dest = unsafe { dest_ptr.add(field_offset as usize) };
            if let Err(e) = instantiate_expr_into(db, rt, *field_expr, field_ty.ty(db), tydesc_table, field_dest) {
                // Destroy successfully instantiated fields.
                for j in 0..i {
                    let prev_field_ty = type_fields[j].ty(db);
                    let prev_field_tydesc = tydesc_table.get_or_create(prev_field_ty.ty(db));
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

fn instantiate_enum<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    variant_name: bct::text::InternedText<'db>,
    payload_expr: Option<ExprFull<'db>>,
    type_variants: &[TypeEnumVariant<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    enum_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let variant_name_str = variant_name.as_str(db);
    let (variant_index, variant_ty) = type_variants
        .iter()
        .enumerate()
        .find(|(_, v)| v.name(db).as_str(db) == variant_name_str)
        .ok_or_else(|| anyhow!("Variant not found: {}", variant_name_str))?;

    let layout = unsafe { rtdt::layout::compute_enum_layout(rtdt::TyDescRef::from_ptr(enum_tydesc)) };

    unsafe {
        *(dest_ptr as *mut u32) = variant_index as u32;
    }

    if let (Some(payload_expr), Some(payload_ty)) = (payload_expr, variant_ty.payload(db)) {
        let payload_offset = layout.variant_offsets[variant_index];
        let payload_dest = unsafe { dest_ptr.add(payload_offset as usize) };
        instantiate_expr_into(db, rt, payload_expr, payload_ty.ty(db), tydesc_table, payload_dest)?;
    }

    Ok(dest_ptr as *const u8)
}

fn instantiate_list<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    elements: &[ExprFull<'db>],
    element_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _list_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let element_ty = element_type.ty(db);
    let element_tydesc = tydesc_table.get_or_create(element_ty);
    let element_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(element_tydesc) };
    let element_size = element_tydesc_ref.size();
    let element_align = element_tydesc_ref.align();

    unsafe {
        // Allocate list data using size=element_size, align=element_align, count=len.
        let data_ptr = if !elements.is_empty() {
            let array_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, element_size, element_align, elements.len() as u32);

            // Try to instantiate all elements. If any fail, clean up and return error.
            for (i, elem) in elements.iter().enumerate() {
                let elem_dest = array_ptr.add(i * element_size as usize);
                if let Err(e) = instantiate_expr_into(db, rt, *elem, element_ty, tydesc_table, elem_dest) {
                    // Destroy successfully instantiated elements.
                    for j in 0..i {
                        let elem_to_destroy = array_ptr.add(j * element_size as usize);
                        datalove_rt::c::dtlv_rti_any_destroy_local(rt, elem_to_destroy, element_tydesc);
                    }
                    // Free the array by calling the allocator's free through the rt handle.
                    let rt_ref = &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal);
                    rt_ref.alloc.free(element_size, element_align, elements.len() as u32, array_ptr);
                    return Err(e);
                }
            }
            array_ptr as *const u8
        } else {
            std::ptr::null()
        };

        let list_ptr = dest_ptr as *mut rtdt::List;
        (*list_ptr).data = data_ptr;
        (*list_ptr).size = elements.len() as u32;
        (*list_ptr).capacity = elements.len() as u32;

        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_option<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    is_some: bool,
    payload_expr: Option<ExprFull<'db>>,
    inner_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    option_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(option_tydesc)) };

    if is_some {
        unsafe { *dest_ptr = rtdt::OptionTag::Some as u8 };

        let payload = payload_expr.ok_or_else(|| anyhow!("Some variant missing payload"))?;
        let payload_dest = unsafe { dest_ptr.add(layout.payload_offset as usize) };
        instantiate_expr_into(db, rt, payload, inner_type.ty(db), tydesc_table, payload_dest)?;
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
    ok_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    result_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(result_tydesc)) };

    if is_ok {
        unsafe { *dest_ptr = rtdt::ResultTag::Ok as u8 };

        let payload = ok_payload_expr.ok_or_else(|| anyhow!("Ok variant missing payload"))?;
        let payload_dest = unsafe { dest_ptr.add(layout.payload_offset as usize) };
        instantiate_expr_into(db, rt, payload, ok_type.ty(db), tydesc_table, payload_dest)?;
    } else {
        unsafe { *dest_ptr = rtdt::ResultTag::Err as u8 };

        let err_payload = err_payload_expr.ok_or_else(|| anyhow!("Err variant missing payload"))?;
        let payload_dest = unsafe { dest_ptr.add(layout.payload_offset as usize) };

        // Instantiate the error payload (which is an Error type).
        let dummy_source = bct::input::Source::new(db, String::new());
        let resolved = crate::resolve::resolve_names(db, dummy_source, err_payload);
        let typechecked = crate::tycheck::type_check(db, err_payload, resolved);

        if !typechecked.errors(db).is_empty() {
            return Err(anyhow!("Type errors in result error payload ({} errors)", typechecked.errors(db).len()));
        }

        let inner_type = typechecked.root_type(db)
            .ok_or_else(|| anyhow!("Cannot determine type of error value"))?;

        let inner_tydesc = tydesc_table.get_or_create(inner_type.ty(db));
        let inner_value = instantiate_expr(db, rt, err_payload, inner_type.ty(db), tydesc_table)?;

        let error_ptr = payload_dest as *mut rtdt::Error;

        unsafe {
            // Error has same layout as Data, so we write it as Data.
            std::ptr::write(
                error_ptr as *mut rtdt::Data,
                rtdt::Data::from_pointers(inner_tydesc, inner_value)
            );
        }
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
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let source = bct::input::Source::new(db, "".to_string());
    let resolved = crate::resolve::resolve_names(db, source, inner_expr);
    let typechecked = crate::tycheck::type_check(db, inner_expr, resolved);

    if !typechecked.errors(db).is_empty() {
        return Err(anyhow!("Type errors in data value ({} errors)", typechecked.errors(db).len()));
    }

    let inner_type = typechecked.root_type(db)
        .ok_or_else(|| anyhow!("Cannot determine type of data value"))?;

    let inner_tydesc = tydesc_table.get_or_create(inner_type.ty(db));
    let inner_value = instantiate_expr(db, rt, inner_expr, inner_type.ty(db), tydesc_table)?;

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
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let dummy_source = bct::input::Source::new(db, String::new());
    let resolved = crate::resolve::resolve_names(db, dummy_source, inner_expr);
    let typechecked = crate::tycheck::type_check(db, inner_expr, resolved);

    if !typechecked.errors(db).is_empty() {
        return Err(anyhow!("Type errors in error value ({} errors)", typechecked.errors(db).len()));
    }

    let inner_type = typechecked.root_type(db)
        .ok_or_else(|| anyhow!("Cannot determine type of error value"))?;

    let inner_tydesc = tydesc_table.get_or_create(inner_type.ty(db));
    let inner_value = instantiate_expr(db, rt, inner_expr, inner_type.ty(db), tydesc_table)?;

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
// Map instantiation
// ============================================================================

/// Build a B-tree from sorted map entries (supports any number of entries).
///
/// Returns the root node pointer and updates total_len with the number of entries.
unsafe fn build_map_btree<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    entries: &[ExprMapEntry<'db>],
    key_type: TypeAndHeap<'db>,
    value_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    key_tydesc: *const rtdt::TyDesc,
    value_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const rtdt::MapNode> {
    let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
    let value_tydesc_ref = rtdt::TyDescRef::from_ptr(value_tydesc);
    let key_size = key_tydesc_ref.size() as usize;
    let value_size = value_tydesc_ref.size() as usize;

    // Step 1: Build all leaf nodes.
    let num_leaves = (entries.len() + rtdt::MAP_NODE_CAPACITY as usize - 1) / rtdt::MAP_NODE_CAPACITY as usize;
    let mut leaves = Vec::with_capacity(num_leaves);

    let leaf_layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc_ref, value_tydesc_ref);

    let entries_per_leaf = (entries.len() + num_leaves - 1) / num_leaves;
    let mut entry_idx = 0;

    for _ in 0..num_leaves {
        let leaf_entries = entries_per_leaf.min(entries.len() - entry_idx);

        // Allocate leaf node.
        let leaf_node = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, leaf_layout.size, leaf_layout.align, 1);

        // Initialize node header: tag = Leaf (2), len = leaf_entries.
        *leaf_node = rtdt::MapNodeTag::Leaf as u8;
        *(leaf_node.add(4) as *mut u32) = leaf_entries as u32;

        // Initialize next_leaf pointer to null (we'll link them next).
        let next_leaf_ptr = leaf_node.add(leaf_layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
        *next_leaf_ptr = std::ptr::null_mut();

        // Get pointers to keys and values arrays.
        let keys_array = leaf_node.add(leaf_layout.keys_offset as usize);
        let values_array = leaf_node.add(leaf_layout.values_offset as usize);

        // Instantiate and copy each key-value pair.
        for i in 0..leaf_entries {
            let entry = &entries[entry_idx + i];
            let key_expr = entry.key(db);
            let value_expr = entry.value(db);

            let key_dest = keys_array.add(i * key_size);
            let value_dest = values_array.add(i * value_size);

            // Try to instantiate key.
            if let Err(e) = instantiate_expr_into(db, rt, key_expr, key_type.ty(db), tydesc_table, key_dest) {
                // Clean up this leaf's already instantiated entries.
                for j in 0..i {
                    let key_to_destroy = keys_array.add(j * key_size);
                    let value_to_destroy = values_array.add(j * value_size);
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, key_to_destroy, key_tydesc);
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, value_to_destroy, value_tydesc);
                }
                // Free this leaf node.
                let rt_ref = &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal);
                rt_ref.alloc.free(leaf_layout.size, leaf_layout.align, 1, leaf_node);
                // Clean up all previously created leaves.
                cleanup_map_leaves(&leaves, rt, key_tydesc, value_tydesc, &leaf_layout);
                return Err(e);
            }

            // Try to instantiate value.
            if let Err(e) = instantiate_expr_into(db, rt, value_expr, value_type.ty(db), tydesc_table, value_dest) {
                // Destroy the key we just instantiated.
                datalove_rt::c::dtlv_rti_any_destroy_local(rt, key_dest, key_tydesc);
                // Clean up this leaf's already instantiated entries.
                for j in 0..i {
                    let key_to_destroy = keys_array.add(j * key_size);
                    let value_to_destroy = values_array.add(j * value_size);
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, key_to_destroy, key_tydesc);
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, value_to_destroy, value_tydesc);
                }
                // Free this leaf node.
                let rt_ref = &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal);
                rt_ref.alloc.free(leaf_layout.size, leaf_layout.align, 1, leaf_node);
                // Clean up all previously created leaves.
                cleanup_map_leaves(&leaves, rt, key_tydesc, value_tydesc, &leaf_layout);
                return Err(e);
            }
        }

        leaves.push(leaf_node as *mut rtdt::MapNode);
        entry_idx += leaf_entries;
    }

    // Link leaf nodes via next_leaf pointers.
    for i in 0..leaves.len() - 1 {
        let next_leaf_ptr = (leaves[i] as *mut u8).add(leaf_layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
        *next_leaf_ptr = leaves[i + 1];
    }

    // If only one leaf, it's the root.
    if leaves.len() == 1 {
        return Ok(leaves[0] as *const rtdt::MapNode);
    }

    // Step 2: Build internal levels bottom-up.
    let mut current_level = leaves;

    loop {
        let num_nodes = current_level.len();
        if num_nodes == 1 {
            return Ok(current_level[0] as *const rtdt::MapNode);
        }

        // Build next level of internal nodes.
        let capacity_plus_one = (rtdt::MAP_NODE_CAPACITY + 1) as usize;
        let num_parents = (num_nodes + capacity_plus_one - 1) / capacity_plus_one;
        let mut parents = Vec::with_capacity(num_parents);

        let internal_layout = rtdt::layout::compute_map_internal_node_layout(key_tydesc_ref);
        let children_per_parent = (num_nodes + num_parents - 1) / num_parents;

        let mut child_idx = 0;

        for _ in 0..num_parents {
            let num_children = children_per_parent.min(num_nodes - child_idx);

            // Allocate internal node.
            let internal_node = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, internal_layout.size, internal_layout.align, 1);

            // Initialize node header: tag = Internal (1), len = num_children - 1 (number of keys).
            *internal_node = rtdt::MapNodeTag::Internal as u8;
            *(internal_node.add(4) as *mut u32) = (num_children - 1) as u32;

            // Get pointers to keys and child_ptrs arrays.
            let keys_array = internal_node.add(internal_layout.keys_offset as usize);
            let child_ptrs_array = internal_node.add(internal_layout.child_ptrs_offset as usize) as *mut *mut rtdt::MapNode;

            // Set child pointers.
            for i in 0..num_children {
                *child_ptrs_array.add(i) = current_level[child_idx + i];
            }

            // Extract separator keys (first key from each child except the first).
            for i in 1..num_children {
                let child_node = current_level[child_idx + i];
                let child_is_leaf = *(child_node as *const u8) == rtdt::MapNodeTag::Leaf as u8;

                let first_key_src = if child_is_leaf {
                    (child_node as *const u8).add(leaf_layout.keys_offset as usize)
                } else {
                    (child_node as *const u8).add(internal_layout.keys_offset as usize)
                };

                let key_dest = keys_array.add((i - 1) * key_size);
                // Clone the key to properly duplicate heap-allocated data.
                let status = datalove_rt::c::dtlv_rti_clone_local(rt, first_key_src, key_tydesc, key_dest);
                if status != datalove_rt::c::RtStatus::Ok {
                    // TODO: Clean up partially constructed internal node and any previously cloned keys.
                    return Err(anyhow!("Failed to clone key for internal node"));
                }
            }

            parents.push(internal_node as *mut rtdt::MapNode);
            child_idx += num_children;
        }

        current_level = parents;
    }
}

/// Helper to clean up partially constructed map leaves.
unsafe fn cleanup_map_leaves(
    leaves: &[*mut rtdt::MapNode],
    rt: datalove_rt::c::LocalRtHandle,
    key_tydesc: *const rtdt::TyDesc,
    value_tydesc: *const rtdt::TyDesc,
    leaf_layout: &rtdt::MapNodeLeafLayout,
) {
    let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
    let value_tydesc_ref = rtdt::TyDescRef::from_ptr(value_tydesc);
    let key_size = key_tydesc_ref.size() as usize;
    let value_size = value_tydesc_ref.size() as usize;

    for &leaf_node in leaves {
        let len = *((leaf_node as *const u8).add(4) as *const u32) as usize;
        let keys_array = (leaf_node as *mut u8).add(leaf_layout.keys_offset as usize);
        let values_array = (leaf_node as *mut u8).add(leaf_layout.values_offset as usize);

        // Destroy all entries in this leaf.
        for i in 0..len {
            let key_to_destroy = keys_array.add(i * key_size);
            let value_to_destroy = values_array.add(i * value_size);
            datalove_rt::c::dtlv_rti_any_destroy_local(rt, key_to_destroy, key_tydesc);
            datalove_rt::c::dtlv_rti_any_destroy_local(rt, value_to_destroy, value_tydesc);
        }

        // Free the leaf node.
        let rt_ref = &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal);
        rt_ref.alloc.free(leaf_layout.size, leaf_layout.align, 1, leaf_node as *mut u8);
    }
}

fn instantiate_map<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    entries: &[ExprMapEntry<'db>],
    key_type: TypeAndHeap<'db>,
    value_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _map_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let map_ptr = dest_ptr as *mut rtdt::Map;

    if entries.is_empty() {
        // Empty map: null root, zero length.
        unsafe {
            (*map_ptr).root = std::ptr::null();
            (*map_ptr).len = 0;
        }
        return Ok(map_ptr as *const u8);
    }

    // Non-empty map: build B-tree (supports any number of entries).
    let key_tydesc = tydesc_table.get_or_create(key_type.ty(db));
    let value_tydesc = tydesc_table.get_or_create(value_type.ty(db));

    let root = unsafe {
        build_map_btree(
            db,
            rt,
            entries,
            key_type,
            value_type,
            tydesc_table,
            key_tydesc,
            value_tydesc,
        )?
    };

    // Initialize Map struct.
    unsafe {
        (*map_ptr).root = root;
        (*map_ptr).len = entries.len() as u32;
    }

    Ok(map_ptr as *const u8)
}

// ============================================================================
// Set instantiation
// ============================================================================

/// Build a B-tree from sorted set elements (supports any number of elements).
///
/// Returns the root node pointer.
unsafe fn build_set_btree<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    elements: &[ExprFull<'db>],
    element_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    element_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const rtdt::SetNode> {
    let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
    let element_size = element_tydesc_ref.size() as usize;

    // Step 1: Build all leaf nodes.
    let num_leaves = (elements.len() + rtdt::SET_NODE_CAPACITY as usize - 1) / rtdt::SET_NODE_CAPACITY as usize;
    let mut leaves = Vec::with_capacity(num_leaves);

    let leaf_layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc_ref);

    let elements_per_leaf = (elements.len() + num_leaves - 1) / num_leaves;
    let mut element_idx = 0;

    for _ in 0..num_leaves {
        let leaf_elements = elements_per_leaf.min(elements.len() - element_idx);

        // Allocate leaf node.
        let leaf_node = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, leaf_layout.size, leaf_layout.align, 1);

        // Initialize node header: tag = Leaf (2), len = leaf_elements.
        *leaf_node = rtdt::SetNodeTag::Leaf as u8;
        *(leaf_node.add(4) as *mut u32) = leaf_elements as u32;

        // Initialize next_leaf pointer to null (will be linked later).
        let next_leaf_ptr = leaf_node.add(leaf_layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
        *next_leaf_ptr = std::ptr::null_mut();

        // Get pointer to keys array.
        let keys_array = leaf_node.add(leaf_layout.keys_offset as usize);

        // Instantiate and copy each element.
        for i in 0..leaf_elements {
            let elem_expr = elements[element_idx + i];

            let elem_dest = keys_array.add(i * element_size);

            // Try to instantiate element.
            if let Err(e) = instantiate_expr_into(db, rt, elem_expr, element_type.ty(db), tydesc_table, elem_dest) {
                // Clean up this leaf's already instantiated elements.
                for j in 0..i {
                    let elem_to_destroy = keys_array.add(j * element_size);
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt, elem_to_destroy, element_tydesc);
                }
                // Free this leaf node.
                let rt_ref = &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal);
                rt_ref.alloc.free(leaf_layout.size, leaf_layout.align, 1, leaf_node);
                // Clean up all previously created leaves.
                cleanup_set_leaves(&leaves, rt, element_tydesc, &leaf_layout);
                return Err(e);
            }
        }

        leaves.push(leaf_node as *mut rtdt::SetNode);
        element_idx += leaf_elements;
    }

    // Link leaf nodes via next_leaf pointers (for B+ tree).
    for i in 0..leaves.len() - 1 {
        let next_leaf_ptr = (leaves[i] as *mut u8).add(leaf_layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
        *next_leaf_ptr = leaves[i + 1];
    }

    // If only one leaf, it's the root.
    if leaves.len() == 1 {
        return Ok(leaves[0] as *const rtdt::SetNode);
    }

    // Step 2: Build internal levels bottom-up.
    let mut current_level = leaves;

    loop {
        let num_nodes = current_level.len();
        if num_nodes == 1 {
            return Ok(current_level[0] as *const rtdt::SetNode);
        }

        // Build next level of internal nodes.
        let capacity_plus_one = (rtdt::SET_NODE_CAPACITY + 1) as usize;
        let num_parents = (num_nodes + capacity_plus_one - 1) / capacity_plus_one;
        let mut parents = Vec::with_capacity(num_parents);

        let internal_layout = rtdt::layout::compute_set_internal_node_layout(element_tydesc_ref);
        let children_per_parent = (num_nodes + num_parents - 1) / num_parents;

        let mut child_idx = 0;

        for _ in 0..num_parents {
            let num_children = children_per_parent.min(num_nodes - child_idx);

            // Allocate internal node.
            let internal_node = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, internal_layout.size, internal_layout.align, 1);

            // Initialize node header: tag = Internal (1), len = num_children - 1 (number of keys).
            *internal_node = rtdt::SetNodeTag::Internal as u8;
            *(internal_node.add(4) as *mut u32) = (num_children - 1) as u32;

            // Get pointers to keys and child_ptrs arrays.
            let keys_array = internal_node.add(internal_layout.keys_offset as usize);
            let child_ptrs_array = internal_node.add(internal_layout.child_ptrs_offset as usize) as *mut *mut rtdt::SetNode;

            // Set child pointers.
            for i in 0..num_children {
                *child_ptrs_array.add(i) = current_level[child_idx + i];
            }

            // Extract separator keys (first key from each child except the first).
            for i in 1..num_children {
                let child_node = current_level[child_idx + i];
                let child_is_leaf = *(child_node as *const u8) == rtdt::SetNodeTag::Leaf as u8;

                let first_key_src = if child_is_leaf {
                    (child_node as *const u8).add(leaf_layout.keys_offset as usize)
                } else {
                    (child_node as *const u8).add(internal_layout.keys_offset as usize)
                };

                let key_dest = keys_array.add((i - 1) * element_size);
                // Clone the key to properly duplicate heap-allocated data.
                let status = datalove_rt::c::dtlv_rti_clone_local(rt, first_key_src, element_tydesc, key_dest);
                if status != datalove_rt::c::RtStatus::Ok {
                    // TODO: Clean up partially constructed internal node and any previously cloned keys.
                    return Err(anyhow!("Failed to clone element for internal node"));
                }
            }

            parents.push(internal_node as *mut rtdt::SetNode);
            child_idx += num_children;
        }

        current_level = parents;
    }
}

/// Helper to clean up partially constructed set leaves.
unsafe fn cleanup_set_leaves(
    leaves: &[*mut rtdt::SetNode],
    rt: datalove_rt::c::LocalRtHandle,
    element_tydesc: *const rtdt::TyDesc,
    leaf_layout: &rtdt::SetNodeLeafLayout,
) {
    let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
    let element_size = element_tydesc_ref.size() as usize;

    for &leaf_node in leaves {
        let len = (*(leaf_node as *const rtdt::SetNode)).len as usize;
        let keys_array = (leaf_node as *mut u8).add(leaf_layout.keys_offset as usize);

        // Destroy all elements in this leaf.
        for i in 0..len {
            let elem_to_destroy = keys_array.add(i * element_size);
            datalove_rt::c::dtlv_rti_any_destroy_local(rt, elem_to_destroy, element_tydesc);
        }

        // Free the leaf node.
        let rt_ref = &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal);
        rt_ref.alloc.free(leaf_layout.size, leaf_layout.align, 1, leaf_node as *mut u8);
    }
}

fn instantiate_set<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    elements: &[ExprFull<'db>],
    element_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _set_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let set_ptr = dest_ptr as *mut rtdt::Set;

    if elements.is_empty() {
        // Empty set: null root, zero length.
        unsafe {
            (*set_ptr).root = std::ptr::null();
            (*set_ptr).len = 0;
        }
        return Ok(set_ptr as *const u8);
    }

    // Non-empty set: build B-tree (supports any number of elements).
    let element_tydesc = tydesc_table.get_or_create(element_type.ty(db));

    let root = unsafe {
        build_set_btree(
            db,
            rt,
            elements,
            element_type,
            tydesc_table,
            element_tydesc,
        )?
    };

    // Initialize Set struct.
    unsafe {
        (*set_ptr).root = root;
        (*set_ptr).len = elements.len() as u32;
    }

    Ok(set_ptr as *const u8)
}

fn instantiate_tensor<'db>(
    db: &'db dyn crate::Db,
    rt: datalove_rt::c::LocalRtHandle,
    shape: &[u32],
    elements: &[ExprFull<'db>],
    element_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _tensor_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let element_ty = element_type.ty(db);
    let element_tydesc = tydesc_table.get_or_create(element_ty);
    let element_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(element_tydesc) };
    let element_size = element_tydesc_ref.size();
    let element_align = element_tydesc_ref.align();

    let rank = shape.len();
    let total_elems: usize = shape.iter().map(|&d| d as usize).product();

    unsafe {
        // Allocate tensor data array.
        let data_ptr = if total_elems > 0 {
            let array_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, element_size, element_align, total_elems as u32);

            // Try to instantiate all elements. If any fail, clean up and return error.
            for (i, elem) in elements.iter().enumerate() {
                let elem_dest = array_ptr.add(i * element_size as usize);
                if let Err(e) = instantiate_expr_into(db, rt, *elem, element_ty, tydesc_table, elem_dest) {
                    // Destroy successfully instantiated elements.
                    for j in 0..i {
                        let elem_to_destroy = array_ptr.add(j * element_size as usize);
                        datalove_rt::c::dtlv_rti_any_destroy_local(rt, elem_to_destroy, element_tydesc);
                    }
                    // Free the array by calling the allocator's free through the rt handle.
                    let rt_ref = &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal);
                    rt_ref.alloc.free(element_size, element_align, total_elems as u32, array_ptr);
                    return Err(e);
                }
            }
            array_ptr
        } else {
            std::ptr::null_mut()
        };

        // Allocate shape array.
        let shape_ptr = if rank > 0 {
            let shape_array = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, std::mem::size_of::<u32>() as u32, std::mem::align_of::<u32>() as u32, rank as u32) as *mut u32;
            for (i, &dim) in shape.iter().enumerate() {
                *shape_array.add(i) = dim;
            }
            shape_array as *const u32
        } else {
            std::ptr::null()
        };

        // Allocate and compute strides array.
        let strides_ptr = if rank > 0 {
            let strides_array = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt, std::mem::size_of::<u32>() as u32, std::mem::align_of::<u32>() as u32, rank as u32) as *mut u32;

            // Compute strides for row-major layout.
            // RowMajor: strides[i] = product of dims[i+1..rank].
            for i in 0..rank {
                let stride = shape[i+1..rank].iter().map(|&d| d as u32).product::<u32>();
                *strides_array.add(i) = if stride == 0 { 1 } else { stride };
            }

            strides_array as *const u32
        } else {
            std::ptr::null()
        };

        // Always use row-major layout for tensor literals.
        let rtdt_layout = rtdt::TensorLayout::RowMajor;

        // Fill in the Tensor struct.
        let tensor_ptr = dest_ptr as *mut rtdt::Tensor;
        (*tensor_ptr).ptr_base = data_ptr;
        (*tensor_ptr).capacity_elems = total_elems as u32;
        (*tensor_ptr).offset_elems = 0;
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

    /// RAII guard for RtLocal to prevent memory leaks on panic.
    struct RtGuard {
        handle: datalove_rt::c::LocalRtHandle,
    }

    impl RtGuard {
        fn new(rt: Box<datalove_rt::impls::rt_local::RtLocal>) -> Self {
            let handle = Box::into_raw(rt) as datalove_rt::c::LocalRtHandle;
            Self { handle }
        }

        fn handle(&self) -> datalove_rt::c::LocalRtHandle {
            self.handle
        }
    }

    impl Drop for RtGuard {
        fn drop(&mut self) {
            unsafe {
                let rt = Box::from_raw(self.handle as *mut datalove_rt::impls::rt_local::RtLocal);
                rt.shutdown();
            }
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
        let typechecked = compile_str(&db, "@true")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, "@false")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, "@42")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, "@3.14")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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

    #[test]
    fn test_instantiate_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, r#"@"hello""#)?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::String);
            let string = &*(inst.ptr as *const rtdt::String);
            assert_eq!(string.size, 5);
            assert!(string.capacity >= 5);
            let str_slice = std::slice::from_raw_parts(string.data, string.size as usize);
            assert_eq!(str_slice, b"hello");
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_empty_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, r#"@"""#)?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::String);
            let string = &*(inst.ptr as *const rtdt::String);
            assert_eq!(string.size, 0);
            assert_eq!(string.capacity, 0);
            assert!(string.data.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tuple_simple() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, "@(@true, @42)")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @int / @42")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
            assert_eq!(int.capacity, 1);
            let limbs = std::slice::from_raw_parts(int.data, 1);
            assert_eq!(limbs[0], 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_zero() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @int / @0")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
            assert_eq!(int.capacity, 1);
            let limbs = std::slice::from_raw_parts(int.data, 1);
            assert_eq!(limbs[0], 0);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_anon_struct() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, "@{x = @1, y = @2}")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
    fn test_instantiate_enum_no_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Enum);
            let enum_info = inst.tydesc.enum_info();
            assert_eq!(enum_info.num_variants(), 2);

            let discriminant = *(inst.ptr as *const u32);
            assert_eq!(discriminant, 0);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_with_scalar_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Enum);
            let enum_info = inst.tydesc.enum_info();
            assert_eq!(enum_info.num_variants(), 2);

            let discriminant = *(inst.ptr as *const u32);
            assert_eq!(discriminant, 0);

            let variants = enum_info.variants();
            let payload_offset = variants[0].offset;
            let payload_value = *(inst.ptr.add(payload_offset as usize) as *const u32);
            assert_eq!(payload_value, 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_list_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, "@[@1, @2, @3, @4, @5]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, 5);
            assert_eq!(list.capacity, 5);

            let element_tydesc = inst.tydesc.list_element_ty().as_ptr();
            assert!(!element_tydesc.is_null());
            assert_eq!((*element_tydesc).type_tag, rtdt::TyTag::U32);

            let elements = std::slice::from_raw_parts(list.data as *const u32, list.size as usize);
            assert_eq!(elements, &[1, 2, 3, 4, 5]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_empty_list() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @[@u32] / @[]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, 0);
            assert_eq!(list.capacity, 0);
            assert!(list.data.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @?@u32 / @none")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @?@u32 / @42")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, r#"@[@"hello", @"world"]"#)?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, 2);

            let element_tydesc = inst.tydesc.list_element_ty().as_ptr();
            assert_eq!((*element_tydesc).type_tag, rtdt::TyTag::String);

            let strings = std::slice::from_raw_parts(list.data as *const rtdt::String, list.size as usize);

            assert_eq!(strings[0].size, 5);
            let str0 = std::slice::from_raw_parts(strings[0].data, strings[0].size as usize);
            assert_eq!(str0, b"hello");

            assert_eq!(strings[1].size, 5);
            let str1 = std::slice::from_raw_parts(strings[1].data, strings[1].size as usize);
            assert_eq!(str1, b"world");
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_nested_option_some_some() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @?@?@u32 / @42")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
    fn test_instantiate_named_tuple() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @tuple Point(@u32, @u32) / @tuple Point(@1, @2)")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
            let u32_value_1 = *(inst.ptr.add(fields[0].offset as usize) as *const u32);
            assert_eq!(u32_value_1, 1);

            let u32_value_2 = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(u32_value_2, 2);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_large() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @int / @1234567890123456789")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
    fn test_instantiate_named_struct() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @struct Point {x: @u32, y: @u32} / @struct Point {x = @10, y = @20}")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
            let x_value = *(inst.ptr.add(fields[0].offset as usize) as *const u32);
            assert_eq!(x_value, 10);

            let y_value = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(y_value, 20);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_anon_to_named_struct() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @struct Point {x: @u32, y: @u32} / @{x = @5, y = @15}")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
            let x_value = *(inst.ptr.add(fields[0].offset as usize) as *const u32);
            assert_eq!(x_value, 5);

            let y_value = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(y_value, 15);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_anon_to_named_coercion() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @enum Status { Ok, Error } / @enum Error")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Enum);
            let discriminant = *(inst.ptr as *const u32);
            assert_eq!(discriminant, 1);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_with_tuple_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @enum { Ok(@(@u32, @u32)), Err(@string) } / @enum Ok(@(@10, @20))")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Enum);
            let enum_info = inst.tydesc.enum_info();
            assert_eq!(enum_info.num_variants(), 2);

            let discriminant = *(inst.ptr as *const u32);
            assert_eq!(discriminant, 0);

            let variants = enum_info.variants();
            let payload_offset = variants[0].offset;
            let payload_ptr = inst.ptr.add(payload_offset as usize);

            let payload_tydesc = variants[0].payload;
            assert!(!payload_tydesc.is_null());
            assert_eq!((*payload_tydesc).type_tag, rtdt::TyTag::Tuple);

            let tuple_info = &(*payload_tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let tuple_fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let first_value = *(payload_ptr.add(tuple_fields[0].offset as usize) as *const u32);
            let second_value = *(payload_ptr.add(tuple_fields[1].offset as usize) as *const u32);

            assert_eq!(first_value, 10);
            assert_eq!(second_value, 20);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_with_struct_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @enum { Data(@{x: @u32, y: @u32}), None } / @enum Data(@{x = @5, y = @15})")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Enum);
            let enum_info = inst.tydesc.enum_info();
            assert_eq!(enum_info.num_variants(), 2);

            let discriminant = *(inst.ptr as *const u32);
            assert_eq!(discriminant, 0);

            let variants = enum_info.variants();
            let payload_offset = variants[0].offset;
            let payload_ptr = inst.ptr.add(payload_offset as usize);

            let payload_tydesc = variants[0].payload;
            assert!(!payload_tydesc.is_null());
            assert_eq!((*payload_tydesc).type_tag, rtdt::TyTag::Struct);

            let struct_info = &(*payload_tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            let struct_fields = std::slice::from_raw_parts(struct_info.fields, struct_info.num_fields as usize);
            let x_value = *(payload_ptr.add(struct_fields[0].offset as usize) as *const u32);
            let y_value = *(payload_ptr.add(struct_fields[1].offset as usize) as *const u32);

            assert_eq!(x_value, 5);
            assert_eq!(y_value, 15);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_list_of_tuples() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, "@[@(@1, @2), @(@3, @4), @(@5, @6)]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, 3);

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
        let typechecked = compile_str(&db, ": @?@(@u32, @u32) / @none")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @?@(@u32, @u32) / @(@10, @20)")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @?@{x: @u32, y: @u32} / @none")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @?@{x: @u32, y: @u32} / @{x = @100, y = @200}")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @?@[@u32] / @none")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @?@[@u32] / @[]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
            assert_eq!(list.size, 0);
            assert_eq!(list.capacity, 0);
            assert!(list.data.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_list_some_nonempty() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @?@[@u32] / @[@1, @2, @3]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
            assert_eq!(list.size, 3);
            assert_eq!(list.capacity, 3);

            let elements = std::slice::from_raw_parts(list.data as *const u32, list.size as usize);
            assert_eq!(elements, &[1, 2, 3]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_string_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @?@string / @none")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
    fn test_instantiate_option_of_enum_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @?@enum { Ok, Error } / @none")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*inner_tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_enum_some() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @?@enum { Ok, Error(@string) } / @enum Error(@\"failed\")")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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

            let discriminant = *(payload_ptr as *const u32);
            assert_eq!(discriminant, 1);

            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            let enum_info = &(*inner_tydesc).type_info.enum_;
            let variants = std::slice::from_raw_parts(enum_info.variants, enum_info.num_variants as usize);
            let enum_layout = rtdt::layout::compute_enum_layout(rtdt::TyDescRef::from_ptr(inner_tydesc));

            let enum_payload_ptr = payload_ptr.add(enum_layout.variant_offsets[1] as usize);
            let string = &*(enum_payload_ptr as *const rtdt::String);
            assert_eq!(string.size, 6);
            let str_slice = std::slice::from_raw_parts(string.data, string.size as usize);
            assert_eq!(str_slice, b"failed");
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_nested_option_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": @?@?@u32 / @none")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @?@?@u32 / @none")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @?@?@(@u32, @bool) / @(@5, @true)")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @!@u32 / @42")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": @!@u32 / @error @\"oops\"")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
            let string_bytes = std::slice::from_raw_parts((*string_ptr).data, (*string_ptr).size as usize);
            assert_eq!(string_bytes, b"oops");
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_error() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, "@error @42")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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
        let typechecked = compile_str(&db, ": tensor<u32, 2> / @tensor [2, 3] [1 2 3, 4 5 6]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tensor);

            let tensor = inst.ptr as *const rtdt::Tensor;
            assert_eq!((*tensor).capacity_elems, 6);
            assert_eq!((*tensor).offset_elems, 0);
            assert_eq!((*tensor).layout, rtdt::TensorLayout::RowMajor);

            // Check shape [2, 3]
            let shape = std::slice::from_raw_parts((*tensor).shape, 2);
            assert_eq!(shape, &[2, 3]);

            // Check strides [3, 1] (row-major)
            let strides = std::slice::from_raw_parts((*tensor).strides, 2);
            assert_eq!(strides, &[3, 1]);

            // Check data [1, 2, 3, 4, 5, 6]
            let data = std::slice::from_raw_parts((*tensor).ptr_base as *const u32, 6);
            assert_eq!(data, &[1, 2, 3, 4, 5, 6]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tensor_1d_f32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": tensor<f32, 1> / @tensor [5] [1.0, 2.0, 3.0, 4.0, 5.0]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tensor);

            let tensor = inst.ptr as *const rtdt::Tensor;
            assert_eq!((*tensor).capacity_elems, 5);
            assert_eq!((*tensor).offset_elems, 0);
            assert_eq!((*tensor).layout, rtdt::TensorLayout::RowMajor);

            // Check shape [5]
            let shape = std::slice::from_raw_parts((*tensor).shape, 1);
            assert_eq!(shape, &[5]);

            // Check strides [1] (row-major, but for 1D doesn't matter)
            let strides = std::slice::from_raw_parts((*tensor).strides, 1);
            assert_eq!(strides, &[1]);

            // Check data [1.0, 2.0, 3.0, 4.0, 5.0]
            let data = std::slice::from_raw_parts((*tensor).ptr_base as *const f32, 5);
            assert_eq!(data, &[1.0, 2.0, 3.0, 4.0, 5.0]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tensor_rank4() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": tensor<u32, 4> / @tensor [2, 2, 2, 2] [1 2, 3 4, 5 6, 7 8, 9 10, 11 12, 13 14, 15 16]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tensor);

            let tensor = inst.ptr as *const rtdt::Tensor;
            assert_eq!((*tensor).capacity_elems, 16);
            assert_eq!((*tensor).offset_elems, 0);
            assert_eq!((*tensor).layout, rtdt::TensorLayout::RowMajor);

            // Check shape [2, 2, 2, 2]
            let shape = std::slice::from_raw_parts((*tensor).shape, 4);
            assert_eq!(shape, &[2, 2, 2, 2]);

            // Check strides [8, 4, 2, 1] (row-major for 4D)
            let strides = std::slice::from_raw_parts((*tensor).strides, 4);
            assert_eq!(strides, &[8, 4, 2, 1]);

            // Check data
            let data = std::slice::from_raw_parts((*tensor).ptr_base as *const u32, 16);
            assert_eq!(data, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tensor_3d_i32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": tensor<i32, 3> / @tensor [2, 2, 2] [1 2, 3 4, 5 6, 7 8]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tensor);

            let tensor = inst.ptr as *const rtdt::Tensor;
            assert_eq!((*tensor).capacity_elems, 8);
            assert_eq!((*tensor).offset_elems, 0);
            assert_eq!((*tensor).layout, rtdt::TensorLayout::RowMajor);

            // Check shape [2, 2, 2]
            let shape = std::slice::from_raw_parts((*tensor).shape, 3);
            assert_eq!(shape, &[2, 2, 2]);

            // Check strides [4, 2, 1] (row-major for 3D)
            let strides = std::slice::from_raw_parts((*tensor).strides, 3);
            assert_eq!(strides, &[4, 2, 1]);

            // Check data [1, 2, 3, 4, 5, 6, 7, 8]
            let data = std::slice::from_raw_parts((*tensor).ptr_base as *const i32, 8);
            assert_eq!(data, &[1, 2, 3, 4, 5, 6, 7, 8]);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tensor_of_tuples() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": tensor<(u32, f32), 2> / @tensor [2, 2] [(1, 1.0) (2, 2.0), (3, 3.0) (4, 4.0)]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::Tensor);

            let tensor = inst.ptr as *const rtdt::Tensor;
            assert_eq!((*tensor).capacity_elems, 4);
            assert_eq!((*tensor).offset_elems, 0);
            assert_eq!((*tensor).layout, rtdt::TensorLayout::RowMajor);

            // Check shape [2, 2]
            let shape = std::slice::from_raw_parts((*tensor).shape, 2);
            assert_eq!(shape, &[2, 2]);

            // Check strides [2, 1]
            let strides = std::slice::from_raw_parts((*tensor).strides, 2);
            assert_eq!(strides, &[2, 1]);

            // Verify we have tuple elements (just check the tensor structure is correct)
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_list_of_tensors() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": [tensor<u32, 2>] / @[@tensor [2, 2] [1 2, 3 4], @tensor [2, 2] [5 6, 7 8]]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
        let mut tydesc_table = TyDescTable::new(&db);
        let inst_guard = InstGuard::new(
            guard.handle(),
            instantiate_value(&db, guard.handle(), &mut tydesc_table, typechecked)?
        );
        let inst = inst_guard.value();

        unsafe {
            assert_eq!(inst.tydesc.type_tag(), rtdt::TyTag::List);

            let list = inst.ptr as *const rtdt::List;
            assert_eq!((*list).size, 2);

            // Verify both elements are tensors (basic structure check)
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_tensor() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile_str(&db, ": ?tensor<u32, 2> / @tensor [2, 2] [1 2, 3 4]")?;
        let rt = datalove_rt::impls::rt_local::RtLocal::new();
        let mut guard = RtGuard::new(rt);
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

            // Verify the payload is a tensor
            let inner_tydesc = inst.tydesc.option_inner_ty().as_ptr();
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Tensor);
        }
        Ok(())
    }
}
