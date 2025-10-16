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
//! let mut rt = datalove_rt::alloc::LocalRt::new();
//! let result = instantiate_value(db, &mut rt, typechecked)?;
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
    rt: &mut datalove_rt::alloc::LocalRt,
    typechecked: TypecheckResult<'db>,
) -> AnyResult<(TyDescTable<'db>, InstantiatedValue)> {
    let root_type = typechecked.root_type(db)
        .ok_or_else(|| anyhow!("No root type"))?;
    let root_expr = typechecked.root_expr(db);

    let mut tydesc_table = TyDescTable::new(db);
    let tydesc = tydesc_table.get_or_create(root_type.ty(db));

    let value_ptr = instantiate_expr(db, rt, root_expr, root_type.ty(db), &mut tydesc_table)?;

    Ok((
        tydesc_table,
        InstantiatedValue {
            ptr: value_ptr,
            tydesc,
        },
    ))
}

/// Instantiate an expression into a runtime value.
fn instantiate_expr<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::alloc::LocalRt,
    expr: ExprFull<'db>,
    ty: &Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
) -> AnyResult<*const u8> {
    let expr_and_heap = expr.expr(db);
    let expr_inner = expr_and_heap.expr(db);

    match (expr_inner, ty) {
        (Expr::True, Type::Bool) => instantiate_bool(rt, true),
        (Expr::False, Type::Bool) => instantiate_bool(rt, false),

        (Expr::Int(int_expr), Type::U8) => instantiate_u8(rt, db, int_expr),
        (Expr::Int(int_expr), Type::I8) => instantiate_i8(rt, db, int_expr),
        (Expr::Int(int_expr), Type::U16) => instantiate_u16(rt, db, int_expr),
        (Expr::Int(int_expr), Type::I16) => instantiate_i16(rt, db, int_expr),
        (Expr::Int(int_expr), Type::U32) => instantiate_u32(rt, db, int_expr),
        (Expr::Int(int_expr), Type::I32) => instantiate_i32(rt, db, int_expr),
        (Expr::Int(int_expr), Type::U64) => instantiate_u64(rt, db, int_expr),
        (Expr::Int(int_expr), Type::I64) => instantiate_i64(rt, db, int_expr),
        (Expr::Int(int_expr), Type::Int) => instantiate_bigint(rt, db, int_expr),

        (Expr::Float(float_expr), Type::F32) => instantiate_f32(rt, db, float_expr),

        (Expr::String(string_expr), Type::String) => {
            instantiate_string(rt, db, string_expr)
        }

        (Expr::AnonTuple(tuple_expr), Type::AnonTuple(tuple_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_tuple(db, rt, &tuple_expr.elements(db), &tuple_ty.fields(db), tydesc_table, tydesc)
        }

        (Expr::NamedTuple(tuple_expr), Type::NamedTuple(tuple_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_tuple(db, rt, &tuple_expr.elements(db), &tuple_ty.fields(db), tydesc_table, tydesc)
        }

        (Expr::AnonStruct(struct_expr), Type::AnonStruct(struct_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_struct(db, rt, &struct_expr.fields(db), &struct_ty.fields(db), tydesc_table, tydesc)
        }

        (Expr::AnonStruct(struct_expr), Type::NamedStruct(struct_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_struct(db, rt, &struct_expr.fields(db), &struct_ty.fields(db), tydesc_table, tydesc)
        }

        (Expr::NamedStruct(struct_expr), Type::NamedStruct(struct_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_struct(db, rt, &struct_expr.fields(db), &struct_ty.fields(db), tydesc_table, tydesc)
        }

        (Expr::AnonEnum(enum_expr), Type::AnonEnum(enum_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_enum(db, rt, enum_expr.variant_name(db), enum_expr.payload(db), &enum_ty.variants(db), tydesc_table, tydesc)
        }

        (Expr::AnonEnum(enum_expr), Type::NamedEnum(enum_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_enum(db, rt, enum_expr.variant_name(db), enum_expr.payload(db), &enum_ty.variants(db), tydesc_table, tydesc)
        }

        (Expr::NamedEnum(enum_expr), Type::NamedEnum(enum_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_enum(db, rt, enum_expr.variant_name(db), enum_expr.payload(db), &enum_ty.variants(db), tydesc_table, tydesc)
        }

        (Expr::List(list_expr), Type::List(list_ty)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_list(db, rt, &list_expr.elements(db), list_ty.element_type(db), tydesc_table, tydesc)
        }

        (Expr::None, Type::Option(opt)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_option(db, rt, false, None, opt.inner_type(db), tydesc_table, tydesc)
        }

        (_, Type::Option(opt)) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_option(db, rt, true, Some(expr), opt.inner_type(db), tydesc_table, tydesc)
        }

        (Expr::Data(data_expr), Type::Data) => {
            let tydesc = tydesc_table.get_or_create(ty);
            instantiate_data(db, rt, data_expr.value(db), tydesc_table, tydesc)
        }

        _ => bail!("Unsupported expression/type combination for instantiation"),
    }
}

// ============================================================================
// Scalar type instantiation
// ============================================================================

fn instantiate_bool(rt: &mut datalove_rt::alloc::LocalRt, value: bool) -> AnyResult<*const u8> {
    unsafe {
        let ptr = rt.alloc(1, 1, 1);
        *ptr = if value { 1 } else { 0 };
        Ok(ptr as *const u8)
    }
}

fn instantiate_u8(rt: &mut datalove_rt::alloc::LocalRt, db: &dyn crate::Db, int_expr: ExprInt) -> AnyResult<*const u8> {
    let value_str = int_expr.value(db).as_str(db);
    let value: u8 = value_str.parse()?;

    unsafe {
        let ptr = rt.alloc(1, 1, 1);
        *ptr = value;
        Ok(ptr as *const u8)
    }
}

fn instantiate_i8(rt: &mut datalove_rt::alloc::LocalRt, db: &dyn crate::Db, int_expr: ExprInt) -> AnyResult<*const u8> {
    let value_str = int_expr.value(db).as_str(db);
    let value: i8 = value_str.parse()?;

    unsafe {
        let ptr = rt.alloc(1, 1, 1) as *mut i8;
        *ptr = value;
        Ok(ptr as *const u8)
    }
}

fn instantiate_u16(rt: &mut datalove_rt::alloc::LocalRt, db: &dyn crate::Db, int_expr: ExprInt) -> AnyResult<*const u8> {
    let value_str = int_expr.value(db).as_str(db);
    let value: u16 = value_str.parse()?;

    unsafe {
        let ptr = rt.alloc(2, 2, 1) as *mut u16;
        *ptr = value;
        Ok(ptr as *const u8)
    }
}

fn instantiate_i16(rt: &mut datalove_rt::alloc::LocalRt, db: &dyn crate::Db, int_expr: ExprInt) -> AnyResult<*const u8> {
    let value_str = int_expr.value(db).as_str(db);
    let value: i16 = value_str.parse()?;

    unsafe {
        let ptr = rt.alloc(2, 2, 1) as *mut i16;
        *ptr = value;
        Ok(ptr as *const u8)
    }
}

fn instantiate_u32(rt: &mut datalove_rt::alloc::LocalRt, db: &dyn crate::Db, int_expr: ExprInt) -> AnyResult<*const u8> {
    let value_str = int_expr.value(db).as_str(db);
    let value: u32 = value_str.parse()?;

    unsafe {
        let ptr = rt.alloc(4, 4, 1) as *mut u32;
        *ptr = value;
        Ok(ptr as *const u8)
    }
}

fn instantiate_i32(rt: &mut datalove_rt::alloc::LocalRt, db: &dyn crate::Db, int_expr: ExprInt) -> AnyResult<*const u8> {
    let value_str = int_expr.value(db).as_str(db);
    let value: i32 = value_str.parse()?;

    unsafe {
        let ptr = rt.alloc(4, 4, 1) as *mut i32;
        *ptr = value;
        Ok(ptr as *const u8)
    }
}

fn instantiate_u64(rt: &mut datalove_rt::alloc::LocalRt, db: &dyn crate::Db, int_expr: ExprInt) -> AnyResult<*const u8> {
    let value_str = int_expr.value(db).as_str(db);
    let value: u64 = value_str.parse()?;

    unsafe {
        let ptr = rt.alloc(8, 8, 1) as *mut u64;
        *ptr = value;
        Ok(ptr as *const u8)
    }
}

fn instantiate_i64(rt: &mut datalove_rt::alloc::LocalRt, db: &dyn crate::Db, int_expr: ExprInt) -> AnyResult<*const u8> {
    let value_str = int_expr.value(db).as_str(db);
    let value: i64 = value_str.parse()?;

    unsafe {
        let ptr = rt.alloc(8, 8, 1) as *mut i64;
        *ptr = value;
        Ok(ptr as *const u8)
    }
}

fn instantiate_f32(rt: &mut datalove_rt::alloc::LocalRt, db: &dyn crate::Db, float_expr: ExprFloat) -> AnyResult<*const u8> {
    let value_str = float_expr.value(db).as_str(db);
    let value: f32 = value_str.parse()?;

    unsafe {
        let ptr = rt.alloc(4, 4, 1) as *mut f32;
        *ptr = value;
        Ok(ptr as *const u8)
    }
}

fn instantiate_bigint(rt: &mut datalove_rt::alloc::LocalRt, db: &dyn crate::Db, int_expr: ExprInt) -> AnyResult<*const u8> {
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
        // Allocate limbs array via runtime.
        let limbs_ptr = if !limbs.is_empty() {
            let ptr = rt.alloc(std::mem::size_of::<u32>() as u32, 4, limbs.len() as u32) as *mut u32;
            for (i, &limb) in limbs.iter().enumerate() {
                *ptr.add(i) = limb;
            }
            ptr as *const u32
        } else {
            std::ptr::null()
        };

        // Allocate Int struct via runtime.
        let int_ptr = rt.alloc(
            std::mem::size_of::<rtdt::Int>() as u32,
            std::mem::align_of::<rtdt::Int>() as u32,
            1,
        ) as *mut rtdt::Int;

        (*int_ptr).data = limbs_ptr;
        (*int_ptr).size_and_sign = if is_negative {
            -(limbs.len() as i32)
        } else {
            limbs.len() as i32
        };
        (*int_ptr).capacity = limbs.len() as u32;

        Ok(int_ptr as *const u8)
    }
}

// ============================================================================
// String instantiation
// ============================================================================

fn instantiate_string(
    rt: &mut datalove_rt::alloc::LocalRt,
    db: &dyn crate::Db,
    string_expr: ExprString,
) -> AnyResult<*const u8> {
    let value_str_raw = string_expr.value(db).as_str(db);

    let value_str = if value_str_raw.starts_with('"') && value_str_raw.ends_with('"') {
        &value_str_raw[1..value_str_raw.len()-1]
    } else {
        bail!("String literal missing quotes");
    };

    unsafe {
        // Allocate data buffer if non-empty.
        let data_ptr = if !value_str.is_empty() {
            let ptr = rt.alloc(value_str.len() as u32, 1, 1);
            std::ptr::copy_nonoverlapping(
                value_str.as_ptr(),
                ptr,
                value_str.len(),
            );
            ptr as *const u8
        } else {
            std::ptr::null()
        };

        // Allocate String struct.
        let string_ptr = rt.alloc(
            std::mem::size_of::<rtdt::String>() as u32,
            std::mem::align_of::<rtdt::String>() as u32,
            1,
        ) as *mut rtdt::String;

        (*string_ptr).data = data_ptr;
        (*string_ptr).size = value_str.len() as u32;
        (*string_ptr).capacity = value_str.len() as u32;

        Ok(string_ptr as *const u8)
    }
}

// ============================================================================
// Compound type instantiation
// ============================================================================

fn instantiate_tuple<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::alloc::LocalRt,
    elements: &[ExprFull<'db>],
    field_types: &[TypeAndHeap<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    tuple_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const u8> {
    let layout = unsafe { rtdt::layout::compute_tuple_layout(tuple_tydesc) };

    let tuple_ptr = unsafe {
        rt.alloc(layout.size, layout.align, 1)
    };

    for (i, (elem, field_ty)) in elements.iter().zip(field_types.iter()).enumerate() {
        let field_value = instantiate_expr(db, rt, *elem, field_ty.ty(db), tydesc_table)?;
        let field_offset = layout.field_offsets[i];
        let field_size = unsafe { (*tydesc_table.get_or_create(field_ty.ty(db))).size };

        unsafe {
            std::ptr::copy_nonoverlapping(
                field_value,
                tuple_ptr.add(field_offset as usize),
                field_size as usize,
            );
        }
    }

    Ok(tuple_ptr as *const u8)
}

fn instantiate_struct<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::alloc::LocalRt,
    expr_fields: &[ExprStructField<'db>],
    type_fields: &[TypeNamedField<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    struct_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const u8> {
    let layout = unsafe { rtdt::layout::compute_struct_layout(struct_tydesc) };

    let struct_ptr = unsafe {
        rt.alloc(layout.size, layout.align, 1)
    };

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

        let field_value = instantiate_expr(db, rt, *field_expr, field_ty.ty(db), tydesc_table)?;
        let field_offset = layout.field_offsets[i];
        let field_size = unsafe { (*tydesc_table.get_or_create(field_ty.ty(db))).size };

        unsafe {
            std::ptr::copy_nonoverlapping(
                field_value,
                struct_ptr.add(field_offset as usize),
                field_size as usize,
            );
        }
    }

    Ok(struct_ptr as *const u8)
}

fn instantiate_enum<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::alloc::LocalRt,
    variant_name: bct::text::InternedText<'db>,
    payload_expr: Option<ExprFull<'db>>,
    type_variants: &[TypeEnumVariant<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    enum_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const u8> {
    let variant_name_str = variant_name.as_str(db);
    let (variant_index, variant_ty) = type_variants
        .iter()
        .enumerate()
        .find(|(_, v)| v.name(db).as_str(db) == variant_name_str)
        .ok_or_else(|| anyhow!("Variant not found: {}", variant_name_str))?;

    let layout = unsafe { rtdt::layout::compute_enum_layout(enum_tydesc) };

    let enum_ptr = unsafe {
        rt.alloc(layout.size, layout.align, 1)
    };

    unsafe {
        *(enum_ptr as *mut u32) = variant_index as u32;
    }

    if let (Some(payload_expr), Some(payload_ty)) = (payload_expr, variant_ty.payload(db)) {
        let payload_value = instantiate_expr(db, rt, payload_expr, payload_ty.ty(db), tydesc_table)?;
        let payload_size = unsafe { (*tydesc_table.get_or_create(payload_ty.ty(db))).size } as usize;
        let payload_offset = layout.variant_offsets[variant_index];

        unsafe {
            std::ptr::copy_nonoverlapping(
                payload_value,
                enum_ptr.add(payload_offset as usize),
                payload_size,
            );
        }
    }

    Ok(enum_ptr as *const u8)
}

fn instantiate_list<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::alloc::LocalRt,
    elements: &[ExprFull<'db>],
    element_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _list_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const u8> {
    let element_ty = element_type.ty(db);
    let element_tydesc = tydesc_table.get_or_create(element_ty);
    let element_size = unsafe { (*element_tydesc).size };
    let element_align = unsafe { (*element_tydesc).align };

    unsafe {
        let data_ptr = if !elements.is_empty() {
            let array_ptr = rt.alloc(element_size, element_align, elements.len() as u32);

            for (i, elem) in elements.iter().enumerate() {
                let elem_value = instantiate_expr(db, rt, *elem, element_ty, tydesc_table)?;

                std::ptr::copy_nonoverlapping(
                    elem_value,
                    array_ptr.add(i * element_size as usize),
                    element_size as usize,
                );
            }
            array_ptr as *const u8
        } else {
            std::ptr::null()
        };

        let list_ptr = rt.alloc(
            std::mem::size_of::<rtdt::List>() as u32,
            std::mem::align_of::<rtdt::List>() as u32,
            1,
        ) as *mut rtdt::List;

        (*list_ptr).data = data_ptr;
        (*list_ptr).size = elements.len() as u32;
        (*list_ptr).capacity = elements.len() as u32;

        Ok(list_ptr as *const u8)
    }
}

fn instantiate_option<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::alloc::LocalRt,
    is_some: bool,
    payload_expr: Option<ExprFull<'db>>,
    inner_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    option_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const u8> {
    let layout = unsafe { rtdt::layout::compute_option_layout(option_tydesc) };

    let option_ptr = unsafe {
        rt.alloc(layout.size, layout.align, 1)
    };

    if is_some {
        unsafe { *option_ptr = rtdt::OptionTag::Some as u8 };

        let payload = payload_expr.ok_or_else(|| anyhow!("Some variant missing payload"))?;
        let payload_value = instantiate_expr(db, rt, payload, inner_type.ty(db), tydesc_table)?;
        let payload_size = unsafe { (*tydesc_table.get_or_create(inner_type.ty(db))).size } as usize;

        unsafe {
            std::ptr::copy_nonoverlapping(
                payload_value,
                option_ptr.add(layout.payload_offset as usize),
                payload_size,
            );
        }
    } else {
        unsafe { *option_ptr = rtdt::OptionTag::None as u8 };
    }

    Ok(option_ptr as *const u8)
}

fn instantiate_data<'db>(
    db: &'db dyn crate::Db,
    rt: &mut datalove_rt::alloc::LocalRt,
    inner_expr: ExprFull<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    _data_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const u8> {
    let source = bct::input::Source::new(db, "".to_string());
    let resolved = crate::resolve::resolve_names(db, inner_expr);
    let typechecked = crate::tycheck::type_check(db, inner_expr, resolved);

    let inner_type = typechecked.root_type(db)
        .ok_or_else(|| anyhow!("Cannot determine type of data value"))?;

    let inner_tydesc = tydesc_table.get_or_create(inner_type.ty(db));
    let inner_value = instantiate_expr(db, rt, inner_expr, inner_type.ty(db), tydesc_table)?;

    let data_ptr = unsafe {
        rt.alloc(
            std::mem::size_of::<rtdt::Data>() as u32,
            std::mem::align_of::<rtdt::Data>() as u32,
            1,
        ) as *mut rtdt::Data
    };

    unsafe {
        std::ptr::write(
            data_ptr,
            rtdt::Data::from_pointers(inner_tydesc, inner_value)
        );
    }

    Ok(data_ptr as *const u8)
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
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Bool);
            assert_eq!(*inst.ptr, 1);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_bool_false() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@false")?;
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Bool);
            assert_eq!(*inst.ptr, 0);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@42")?;
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::U32);
            assert_eq!(*(inst.ptr as *const u32), 42);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_f32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@3.14")?;
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::F32);
            assert_eq!(*(inst.ptr as *const f32), 3.14);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, r#"@"hello""#)?;
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::String);
            let string = &*(inst.ptr as *const rtdt::String);
            assert_eq!(string.size, 5);
            assert_eq!(string.capacity, 5);
            let str_slice = std::slice::from_raw_parts(string.data, string.size as usize);
            assert_eq!(str_slice, b"hello");
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_empty_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, r#"@"""#)?;
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::String);
            let string = &*(inst.ptr as *const rtdt::String);
            assert_eq!(string.size, 0);
            assert_eq!(string.capacity, 0);
            assert!(string.data.is_null());
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tuple_simple() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@(@true, @42)")?;
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*inst.tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let bool_value = *(inst.ptr.add(fields[0].offset as usize));
            assert_eq!(bool_value, 1);

            let u32_value = *(inst.ptr.add(fields[1].offset as usize) as *const u32);
            assert_eq!(u32_value, 42);
            rt.shutdown();
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_small() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @int / @42")?;
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_no_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
        let mut rt = datalove_rt::alloc::LocalRt::with_leak_check_mode(datalove_rt::alloc::LeakCheckMode::Ignore);
        let (_tydesc_table, inst) = instantiate_value(&db, &mut rt, typechecked)?;

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
}
