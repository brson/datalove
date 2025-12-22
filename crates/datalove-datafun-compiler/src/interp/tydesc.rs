//! Type descriptor utilities for the interpreter.
//!
//! Functions for converting type hints to runtime type descriptors.

use super::InterpContext;

/// Convert a type hint to a tydesc.
pub fn type_hint_to_tydesc<'db>(
    ctx: &mut InterpContext<'db>,
    type_hint: crate::datalit::ast::TypeHintAndHeap<'db>,
) -> *const datalove_rt::rtdt::TyDesc {
    use crate::datalit::ast::TypeHint;
    use crate::datalit::tycheck::Type;

    match type_hint.type_hint(ctx.db) {
        TypeHint::Bool => ctx.tydesc_table.get_or_create(&Type::Bool),
        TypeHint::U8 => ctx.tydesc_table.get_or_create(&Type::U8),
        TypeHint::I8 => ctx.tydesc_table.get_or_create(&Type::I8),
        TypeHint::U16 => ctx.tydesc_table.get_or_create(&Type::U16),
        TypeHint::I16 => ctx.tydesc_table.get_or_create(&Type::I16),
        TypeHint::U32 => ctx.tydesc_table.get_or_create(&Type::U32),
        TypeHint::I32 => ctx.tydesc_table.get_or_create(&Type::I32),
        TypeHint::U64 => ctx.tydesc_table.get_or_create(&Type::U64),
        TypeHint::I64 => ctx.tydesc_table.get_or_create(&Type::I64),
        TypeHint::F32 => ctx.tydesc_table.get_or_create(&Type::F32),
        TypeHint::Int => ctx.tydesc_table.get_or_create(&Type::Int),
        TypeHint::String => ctx.tydesc_table.get_or_create(&Type::String),
        TypeHint::Option(opt) => {
            let inner_tydesc = type_hint_to_tydesc(ctx, opt.inner_type(ctx.db));
            ctx.tydesc_table.create_option_from_inner_tydesc(inner_tydesc)
        }
        TypeHint::Result(res) => {
            let inner_tydesc = type_hint_to_tydesc(ctx, res.inner_type(ctx.db));
            ctx.tydesc_table.create_result_from_inner_tydesc(inner_tydesc)
        }
        TypeHint::List(list) => {
            let element_tydesc = type_hint_to_tydesc(ctx, list.element_type(ctx.db));
            ctx.tydesc_table.create_list_from_element_tydesc(element_tydesc)
        }
        TypeHint::Map(map) => {
            let key_tydesc = type_hint_to_tydesc(ctx, map.key_type(ctx.db));
            let value_tydesc = type_hint_to_tydesc(ctx, map.value_type(ctx.db));
            ctx.tydesc_table.create_map_from_key_value_tydescs(key_tydesc, value_tydesc)
        }
        TypeHint::Set(set) => {
            let element_tydesc = type_hint_to_tydesc(ctx, set.element_type(ctx.db));
            ctx.tydesc_table.create_set_from_element_tydesc(element_tydesc)
        }
        TypeHint::AnonTuple(tuple) => {
            let field_tydescs: Vec<_> = tuple.fields(ctx.db)
                .iter()
                .map(|f| type_hint_to_tydesc(ctx, *f))
                .collect();
            ctx.tydesc_table.get_or_create_tuple(&field_tydescs)
        }
        TypeHint::AnonStruct(struct_hint) => {
            let field_names_and_tydescs: Vec<_> = struct_hint.fields(ctx.db)
                .iter()
                .map(|f| (f.name(ctx.db), type_hint_to_tydesc(ctx, f.type_hint(ctx.db))))
                .collect();
            ctx.tydesc_table.get_or_create_struct(&field_names_and_tydescs)
        }
        TypeHint::Data => ctx.tydesc_table.get_or_create(&Type::Data),
        TypeHint::Error => ctx.tydesc_table.get_or_create(&Type::Error),
        // Tensor, AnonEnum, ParseError not yet supported - fall back to U32.
        _ => {
            todo!()
        }
    }
}
