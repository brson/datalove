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
//! let mut rt = datalove_rt::rt_local::RtLocal::new();
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
#[derive(Debug)]
pub struct InstantiatedValue {
    pub ptr: *const u8,
    pub tydesc: *const rtdt::TyDesc,
}

/// Instantiate a value from typechecked AST using the runtime.
///
/// This is the primary entry point for runtime-integrated instantiation.
/// The caller provides a LocalRt and is responsible for cleanup.
pub fn instantiate_value<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::rt_local::RtLocal,
    tydesc_table: &mut TyDescTable<'db>,
    typechecked: TypecheckResult<'db>,
) -> AnyResult<InstantiatedValue> {
    let root_type = typechecked.root_type(db)
        .ok_or_else(|| anyhow!("No root type"))?;
    let root_expr = typechecked.root_expr(db);

    let tydesc = tydesc_table.get_or_create(root_type.ty(db));

    let value_ptr = instantiate_expr(db, rt, root_expr, root_type.ty(db), tydesc_table)?;

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
    rt: &mut datalove_rt::rt_local::RtLocal,
    expr: ExprFull<'db>,
    ty: &Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
) -> AnyResult<*const u8> {
    let tydesc = tydesc_table.get_or_create(ty);
    let dest_ptr = unsafe {
        rt.alloc.alloc((*tydesc).size, (*tydesc).align, 1)
    };
    instantiate_expr_into(db, rt, expr, ty, tydesc_table, dest_ptr)?;
    Ok(dest_ptr)
}

/// Instantiate an expression into pre-allocated memory at dest_ptr.
///
/// dest_ptr must be valid and properly aligned.
fn instantiate_expr_into<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::rt_local::RtLocal,
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

        (Expr::Data(data_expr), Type::Data) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_data(db, rt, data_expr.value(db), tydesc_table, tydesc, dest_ptr)
        }

        (Expr::Err(err_expr), Type::Error) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_error(db, rt, err_expr.value(db), tydesc_table, tydesc, dest_ptr)
        }

        _ => bail!("Unsupported expression/type combination for instantiation"),
    }
}

// ============================================================================
// Scalar type instantiation
// ============================================================================

fn instantiate_bool(_rt: &mut datalove_rt::rt_local::RtLocal, value: bool, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    unsafe {
        *dest_ptr = if value { 1 } else { 0 };
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u8(_rt: &mut datalove_rt::rt_local::RtLocal, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: u8 = value_str.parse()?;
    unsafe {
        *dest_ptr = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i8(_rt: &mut datalove_rt::rt_local::RtLocal, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: i8 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i8) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u16(_rt: &mut datalove_rt::rt_local::RtLocal, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: u16 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut u16) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i16(_rt: &mut datalove_rt::rt_local::RtLocal, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: i16 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i16) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u32(_rt: &mut datalove_rt::rt_local::RtLocal, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: u32 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut u32) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i32(_rt: &mut datalove_rt::rt_local::RtLocal, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: i32 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i32) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_u64(_rt: &mut datalove_rt::rt_local::RtLocal, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: u64 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut u64) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_i64(_rt: &mut datalove_rt::rt_local::RtLocal, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = int_expr.value(db).as_str(db);
    let value: i64 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut i64) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_f32(_rt: &mut datalove_rt::rt_local::RtLocal, db: &dyn crate::Db, float_expr: ExprFloat, dest_ptr: *mut u8) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let value_str = float_expr.value(db).as_str(db);
    let value: f32 = value_str.parse()?;
    unsafe {
        *(dest_ptr as *mut f32) = value;
        Ok(dest_ptr as *const u8)
    }
}

fn instantiate_bigint(rt: &mut datalove_rt::rt_local::RtLocal, db: &dyn crate::Db, int_expr: ExprInt, dest_ptr: *mut u8) -> AnyResult<*const u8> {
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
            let ptr = rt.alloc.alloc(4, 4, limbs.len() as u32) as *mut u32;
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
    rt: &mut datalove_rt::rt_local::RtLocal,
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
        let rt_handle = rt as *mut _ as datalove_rt::LocalRtHandle;
        let status = datalove_rt::string::string_create_local(
            rt_handle,
            string_ptr as *mut u8,
            string_tydesc,
        );

        if status != datalove_rt::RtStatus::Ok {
            bail!("Failed to create string");
        }

        // Push the string data if non-empty.
        if !value_str.is_empty() {
            let status = datalove_rt::string::string_push_bytes_local(
                rt_handle,
                string_ptr as *mut u8,
                string_tydesc,
                value_str.as_ptr(),
                value_str.len() as u32,
            );

            if status != datalove_rt::RtStatus::Ok {
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
    rt: &mut datalove_rt::rt_local::RtLocal,
    elements: &[ExprFull<'db>],
    field_types: &[TypeAndHeap<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    tuple_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_tuple_layout(tuple_tydesc) };

    for (i, (elem, field_ty)) in elements.iter().zip(field_types.iter()).enumerate() {
        let field_offset = layout.field_offsets[i];
        let field_dest = unsafe { dest_ptr.add(field_offset as usize) };
        instantiate_expr_into(db, rt, *elem, field_ty.ty(db), tydesc_table, field_dest)?;
    }

    Ok(dest_ptr as *const u8)
}

fn instantiate_struct<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::rt_local::RtLocal,
    expr_fields: &[ExprStructField<'db>],
    type_fields: &[TypeNamedField<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    struct_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_struct_layout(struct_tydesc) };

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
            instantiate_expr_into(db, rt, field_expr, field_ty.ty(db), tydesc_table, field_dest)?;
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
            instantiate_expr_into(db, rt, *field_expr, field_ty.ty(db), tydesc_table, field_dest)?;
        }
    }

    Ok(dest_ptr as *const u8)
}

fn instantiate_enum<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::rt_local::RtLocal,
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

    let layout = unsafe { rtdt::layout::compute_enum_layout(enum_tydesc) };

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
    rt: &mut datalove_rt::rt_local::RtLocal,
    elements: &[ExprFull<'db>],
    element_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _list_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let element_ty = element_type.ty(db);
    let element_tydesc = tydesc_table.get_or_create(element_ty);
    let element_size = unsafe { (*element_tydesc).size };
    let element_align = unsafe { (*element_tydesc).align };

    unsafe {
        // Allocate list data using size=element_size, align=element_align, count=len.
        let data_ptr = if !elements.is_empty() {
            let array_ptr = rt.alloc.alloc(element_size, element_align, elements.len() as u32);

            for (i, elem) in elements.iter().enumerate() {
                let elem_dest = array_ptr.add(i * element_size as usize);
                instantiate_expr_into(db, rt, *elem, element_ty, tydesc_table, elem_dest)?;
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
    rt: &mut datalove_rt::rt_local::RtLocal,
    is_some: bool,
    payload_expr: Option<ExprFull<'db>>,
    inner_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    option_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let layout = unsafe { rtdt::layout::compute_option_layout(option_tydesc) };

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

fn instantiate_data<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::rt_local::RtLocal,
    inner_expr: ExprFull<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _data_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let source = bct::input::Source::new(db, "".to_string());
    let resolved = crate::resolve::resolve_names(db, inner_expr);
    let typechecked = crate::tycheck::type_check(db, inner_expr, resolved);

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
    rt: &mut datalove_rt::rt_local::RtLocal,
    inner_expr: ExprFull<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _error_tydesc: *const rtdt::TyDesc,
    dest_ptr: *mut u8,
) -> AnyResult<*const u8> {
    debug_assert!(!dest_ptr.is_null());
    let resolved = crate::resolve::resolve_names(db, inner_expr);
    let typechecked = crate::tycheck::type_check(db, inner_expr, resolved);

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
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn compile<'db>(db: &'db Database, source_text: &str) -> AnyResult<TypecheckResult<'db>> {
        let source = bct::input::Source::new(db, source_text.to_string());
        let parsed = crate::parser::parse(db, source);
        let resolved = crate::resolve::resolve_names(db, parsed);
        let typechecked = crate::tycheck::type_check(db, parsed, resolved);
        Ok(typechecked)
    }

    #[test]
    fn test_instantiate_bool_true() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@true")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Bool);
            assert_eq!(*inst.ptr, 1);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_bool_false() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@false")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Bool);
            assert_eq!(*inst.ptr, 0);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@42")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::U32);
            assert_eq!(*(inst.ptr as *const u32), 42);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_f32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@3.14")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::F32);
            assert_eq!(*(inst.ptr as *const f32), 3.14);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, r#"@"hello""#)?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::String);
            let string = &*(inst.ptr as *const rtdt::String);
            assert_eq!(string.size, 5);
            assert!(string.capacity >= 5);
            let str_slice = std::slice::from_raw_parts(string.data, string.size as usize);
            assert_eq!(str_slice, b"hello");

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_empty_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, r#"@"""#)?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::String);
            let string = &*(inst.ptr as *const rtdt::String);
            assert_eq!(string.size, 0);
            assert_eq!(string.capacity, 0);
            assert!(string.data.is_null());

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tuple_simple() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@(@true, @42)")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*inst.tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let bool_value = *(inst.ptr.add(fields[0].offset as usize));
            assert_eq!(bool_value, 1);

            let u32_value = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(u32_value, 42);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_small() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @int / @42")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Int);
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
        let typechecked = compile(&db, ": @int / @0")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Int);
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
        let typechecked = compile(&db, "@{x = @1, y = @2}")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*inst.tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(struct_info.fields, 2);

            let field0_name = std::slice::from_raw_parts(fields[0].name, fields[0].name_len as usize);
            assert_eq!(field0_name, b"x");

            let field1_name = std::slice::from_raw_parts(fields[1].name, fields[1].name_len as usize);
            assert_eq!(field1_name, b"y");

            let x_value = *(inst.ptr.add(fields[0].offset as usize) as *const u32);
            assert_eq!(x_value, 1);

            let y_value = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(y_value, 2);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_no_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*inst.tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            let discriminant = *(inst.ptr as *const u32);
            assert_eq!(discriminant, 0);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_with_scalar_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*inst.tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            let discriminant = *(inst.ptr as *const u32);
            assert_eq!(discriminant, 0);

            let variants = std::slice::from_raw_parts(enum_info.variants, 2);
            let payload_offset = variants[0].offset;
            let payload_value = *(inst.ptr.add(payload_offset as usize) as *const u32);
            assert_eq!(payload_value, 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_list_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@[@1, @2, @3, @4, @5]")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, 5);
            assert_eq!(list.capacity, 5);

            let element_tydesc = (*inst.tydesc).type_info.list.element_tydesc;
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
        let typechecked = compile(&db, ": @[@u32] / @[]")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::List);
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
        let typechecked = compile(&db, ": @?@u32 / @none")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_some_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@u32 / @42")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
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
        let typechecked = compile(&db, r#"@[@"hello", @"world"]"#)?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, 2);

            let element_tydesc = (*inst.tydesc).type_info.list.element_tydesc;
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
        let typechecked = compile(&db, ": @?@?@u32 / @42")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let outer_tag = *inst.ptr;
            assert_eq!(outer_tag, rtdt::OptionTag::Some as u8);

            let outer_layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let outer_payload_ptr = inst.ptr.add(outer_layout.payload_offset as usize);

            let inner_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Option);
            let inner_tag = *outer_payload_ptr;
            assert_eq!(inner_tag, rtdt::OptionTag::Some as u8);

            let inner_layout = rtdt::layout::compute_option_layout(inner_tydesc);
            let inner_payload_ptr = outer_payload_ptr.add(inner_layout.payload_offset as usize);
            let value = *(inner_payload_ptr as *const u32);
            assert_eq!(value, 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_named_tuple() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @tuple Point(@u32, @u32) / @tuple Point(@1, @2)")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*inst.tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let u32_value_1 = *(inst.ptr.add(fields[0].offset as usize) as *const u32);
            assert_eq!(u32_value_1, 1);

            let u32_value_2 = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(u32_value_2, 2);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_large() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @int / @1234567890123456789")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Int);
            let int = &*(inst.ptr as *const rtdt::Int);
            assert!(int.size_and_sign > 0);
            let num_limbs = int.size_and_sign as usize;
            let limbs = std::slice::from_raw_parts(int.data, num_limbs);

            let mut reconstructed: u64 = 0;
            for (i, &limb) in limbs.iter().enumerate() {
                reconstructed |= (limb as u64) << (32 * i);
            }
            assert_eq!(reconstructed, 1234567890123456789);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_named_struct() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @struct Point {x: @u32, y: @u32} / @struct Point {x = @10, y = @20}")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*inst.tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(struct_info.fields, 2);
            let x_value = *(inst.ptr.add(fields[0].offset as usize) as *const u32);
            assert_eq!(x_value, 10);

            let y_value = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(y_value, 20);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_anon_to_named_struct() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @struct Point {x: @u32, y: @u32} / @{x = @5, y = @15}")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*inst.tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(struct_info.fields, 2);
            let x_value = *(inst.ptr.add(fields[0].offset as usize) as *const u32);
            assert_eq!(x_value, 5);

            let y_value = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(y_value, 15);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_anon_to_named_coercion() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum Status { Ok, Error } / @enum Error")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Enum);
            let discriminant = *(inst.ptr as *const u32);
            assert_eq!(discriminant, 1);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_with_tuple_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum { Ok(@(@u32, @u32)), Err(@string) } / @enum Ok(@(@10, @20))")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*inst.tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            let discriminant = *(inst.ptr as *const u32);
            assert_eq!(discriminant, 0);

            let variants = std::slice::from_raw_parts(enum_info.variants, 2);
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

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_with_struct_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum { Data(@{x: @u32, y: @u32}), None } / @enum Data(@{x = @5, y = @15})")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*inst.tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            let discriminant = *(inst.ptr as *const u32);
            assert_eq!(discriminant, 0);

            let variants = std::slice::from_raw_parts(enum_info.variants, 2);
            let payload_offset = variants[0].offset;
            let payload_ptr = inst.ptr.add(payload_offset as usize);

            let payload_tydesc = variants[0].payload;
            assert!(!payload_tydesc.is_null());
            assert_eq!((*payload_tydesc).type_tag, rtdt::TyTag::Struct);

            let struct_info = &(*payload_tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            let struct_fields = std::slice::from_raw_parts(struct_info.fields, 2);
            let x_value = *(payload_ptr.add(struct_fields[0].offset as usize) as *const u32);
            let y_value = *(payload_ptr.add(struct_fields[1].offset as usize) as *const u32);

            assert_eq!(x_value, 5);
            assert_eq!(y_value, 15);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_list_of_tuples() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@[@(@1, @2), @(@3, @4), @(@5, @6)]")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::List);
            let list = &*(inst.ptr as *const rtdt::List);
            assert_eq!(list.size, 3);

            let element_tydesc = (*inst.tydesc).type_info.list.element_tydesc;
            assert!(!element_tydesc.is_null());
            assert_eq!((*element_tydesc).type_tag, rtdt::TyTag::Tuple);

            let tuple_info = &(*element_tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let tuple_fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let tuple_size = (*element_tydesc).size as usize;

            for i in 0..3 {
                let tuple_ptr = list.data.add(i * tuple_size);
                let first = *(tuple_ptr.add(tuple_fields[0].offset as usize) as *const u32);
                let second = *(tuple_ptr.add(tuple_fields[1].offset as usize) as *const u32);

                assert_eq!(first, (i * 2 + 1) as u32);
                assert_eq!(second, (i * 2 + 2) as u32);
            }

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_tuple_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@(@u32, @u32) / @none")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*inner_tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_tuple_some() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@(@u32, @u32) / @(@10, @20)")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            let inner_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*inner_tydesc).type_info.tuple;
            let tuple_fields = std::slice::from_raw_parts(tuple_info.fields, 2);

            let first = *(payload_ptr.add(tuple_fields[0].offset as usize) as *const u32);
            let second = *(payload_ptr.add(tuple_fields[1].offset as usize) as *const u32);
            assert_eq!(first, 10);
            assert_eq!(second, 20);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_struct_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@{x: @u32, y: @u32} / @none")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*inner_tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_struct_some() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@{x: @u32, y: @u32} / @{x = @100, y = @200}")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            let inner_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*inner_tydesc).type_info.struct_;
            let struct_fields = std::slice::from_raw_parts(struct_info.fields, 2);

            let x_value = *(payload_ptr.add(struct_fields[0].offset as usize) as *const u32);
            let y_value = *(payload_ptr.add(struct_fields[1].offset as usize) as *const u32);
            assert_eq!(x_value, 100);
            assert_eq!(y_value, 200);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_list_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@[@u32] / @none")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::List);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_list_some_empty() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@[@u32] / @[]")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            let list = &*(payload_ptr as *const rtdt::List);
            assert_eq!(list.size, 0);
            assert_eq!(list.capacity, 0);
            assert!(list.data.is_null());

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_list_some_nonempty() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@[@u32] / @[@1, @2, @3]")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            let list = &*(payload_ptr as *const rtdt::List);
            assert_eq!(list.size, 3);
            assert_eq!(list.capacity, 3);

            let elements = std::slice::from_raw_parts(list.data as *const u32, list.size as usize);
            assert_eq!(elements, &[1, 2, 3]);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_string_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@string / @none")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::String);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_enum_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@enum { Ok, Error } / @none")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*inner_tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_enum_some() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@enum { Ok, Error(@string) } / @enum Error(@\"failed\")")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::Some as u8);

            let layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let payload_ptr = inst.ptr.add(layout.payload_offset as usize);

            let discriminant = *(payload_ptr as *const u32);
            assert_eq!(discriminant, 1);

            let inner_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            let enum_info = &(*inner_tydesc).type_info.enum_;
            let variants = std::slice::from_raw_parts(enum_info.variants, 2);
            let enum_layout = rtdt::layout::compute_enum_layout(inner_tydesc);

            let enum_payload_ptr = payload_ptr.add(enum_layout.variant_offsets[1] as usize);
            let string = &*(enum_payload_ptr as *const rtdt::String);
            assert_eq!(string.size, 6);
            let str_slice = std::slice::from_raw_parts(string.data, string.size as usize);
            assert_eq!(str_slice, b"failed");

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_nested_option_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@?@u32 / @none")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            let inner_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::Option);

            let innermost_tydesc = (*inner_tydesc).type_info.option.inner_tydesc;
            assert_eq!((*innermost_tydesc).type_tag, rtdt::TyTag::U32);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_nested_option_some_none() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@?@u32 / @none")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let tag = *inst.ptr;
            assert_eq!(tag, rtdt::OptionTag::None as u8);

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_option_of_option_of_tuple() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@?@(@u32, @bool) / @(@5, @true)")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Option);
            let outer_tag = *inst.ptr;
            assert_eq!(outer_tag, rtdt::OptionTag::Some as u8);

            let outer_layout = rtdt::layout::compute_option_layout(inst.tydesc);
            let outer_payload_ptr = inst.ptr.add(outer_layout.payload_offset as usize);

            let inner_opt_tydesc = (*inst.tydesc).type_info.option.inner_tydesc;
            assert_eq!((*inner_opt_tydesc).type_tag, rtdt::TyTag::Option);
            let inner_tag = *outer_payload_ptr;
            assert_eq!(inner_tag, rtdt::OptionTag::Some as u8);

            let inner_layout = rtdt::layout::compute_option_layout(inner_opt_tydesc);
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

            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);
            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_error() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@error @42")?;
        let rt = datalove_rt::rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as datalove_rt::LocalRtHandle;
        let rt_ref = unsafe { &mut *(rt_handle as *mut datalove_rt::rt_local::RtLocal) };
        let mut tydesc_table = TyDescTable::new(&db);
        let inst = instantiate_value(&db, rt_ref, &mut tydesc_table, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Error);

            let error_ptr = inst.ptr as *const rtdt::Error;
            let inner_tydesc = (*error_ptr).tydesc();
            let inner_value_ptr = (*error_ptr).value_ptr();

            assert_eq!((*inner_tydesc).type_tag, rtdt::TyTag::U32);
            let inner_value = *(inner_value_ptr as *const u32);
            assert_eq!(inner_value, 42);

            // Destroy the error contents.
            let status = datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst.ptr as *mut u8, inst.tydesc);
            assert_eq!(status, datalove_rt::RtStatus::Ok);

            // Free the error wrapper itself.
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst.tydesc, 1, inst.ptr as *mut u8);

            let rt = Box::from_raw(rt_handle as *mut datalove_rt::rt_local::RtLocal);
            rt.shutdown();
        }
        Ok(())
    }
}
