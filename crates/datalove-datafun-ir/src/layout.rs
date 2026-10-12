//! Memory layout of IR types.
//!
//! This is the single authority on how an `IrType` is laid out. Both AOT
//! backends and the interpreter's type descriptor table must agree with it,
//! and with `datalove_rtdt::layout`, which computes the same layouts from
//! runtime type descriptors: generated code writes values that the runtime
//! reads back through a `TyDesc`, so a disagreement is a wrong read, not a
//! failed compile.
//!
//! The two are kept in step by `layout_conformance_tests`, which walks a
//! corpus of types and checks that both agree field by field.

use datalove_rtdt as rtdt;

use crate::IrType;

/// Size and alignment of a type, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeLayout {
    pub size: u32,
    pub align: u32,
}

/// Round `value` up to a multiple of `align`, which must be a power of two.
#[inline]
pub fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Discriminant size and alignment for an enum.
const ENUM_DISCRIMINANT_SIZE: u32 = 4;
const ENUM_DISCRIMINANT_ALIGN: u32 = 4;

/// Tag size for `?T` and `!T`.
const OPTION_TAG_SIZE: u32 = 1;
const RESULT_TAG_SIZE: u32 = 1;

/// Layout of the error payload carried by the `Err` arm of a `!T`.
fn error_layout() -> TypeLayout {
    TypeLayout {
        size: std::mem::size_of::<rtdt::Error>() as u32,
        align: std::mem::align_of::<rtdt::Error>() as u32,
    }
}

/// The layout of a fat pointer: a value's address and its descriptor.
///
/// What a dynamic type is laid out as, and a reference to one.
pub fn fat_pointer_layout() -> TypeLayout {
    TypeLayout {
        size: 2 * std::mem::size_of::<*const u8>() as u32,
        align: std::mem::align_of::<*const u8>() as u32,
    }
}

/// The layout of an IR type.
///
/// A dynamic type -- one that mentions a type parameter -- is laid out as a
/// fat pointer, whatever its form: its own layout is not known until a call,
/// so a frame holds where the value is and what describes it. The value's
/// own layout is the descriptor's to say, and the helpers below that give an
/// offset within a type refuse a dynamic one rather than give an answer for
/// the fat pointer.
pub fn layout_of(ty: &IrType) -> TypeLayout {
    if ty.is_dynamic() {
        return fat_pointer_layout();
    }
    match ty {
        IrType::Param(_) => unreachable!("a type parameter is dynamic"),

        // Zero-sized types.
        IrType::Unit => TypeLayout { size: 0, align: 1 },
        IrType::Atom(_) => TypeLayout { size: 0, align: 1 },

        // Scalars.
        IrType::Bool | IrType::U8 | IrType::I8 => TypeLayout { size: 1, align: 1 },
        IrType::U16 | IrType::I16 => TypeLayout { size: 2, align: 2 },
        IrType::U32 | IrType::I32 | IrType::F32 => TypeLayout { size: 4, align: 4 },
        IrType::U64 | IrType::I64 | IrType::F64 => TypeLayout { size: 8, align: 8 },
        IrType::Index | IrType::Offset => TypeLayout {
            size: rtdt::INDEX_SIZE,
            align: rtdt::INDEX_ALIGN,
        },

        // Runtime structs with a fixed shape.
        IrType::Int => struct_layout::<rtdt::Int>(),
        IrType::String => struct_layout::<rtdt::String>(),
        IrType::Data => struct_layout::<rtdt::Data>(),
        IrType::Error => struct_layout::<rtdt::Error>(),
        IrType::List(_) => struct_layout::<rtdt::List>(),
        IrType::Set(_) => struct_layout::<rtdt::Set>(),
        IrType::Map(_, _) => struct_layout::<rtdt::Map>(),
        IrType::Tensor(_, _) => struct_layout::<rtdt::Tensor>(),
        IrType::Table(_) => struct_layout::<rtdt::Table>(),

        // A reference is a pointer to the referent.
        IrType::Ref(_) => TypeLayout {
            size: std::mem::size_of::<*const u8>() as u32,
            align: std::mem::align_of::<*const u8>() as u32,
        },

        // Composites.
        IrType::Tuple(fields) => aggregate_layout(fields),
        IrType::Struct(fields) => {
            let field_types: Vec<IrType> = fields.iter().map(|(_, ty)| ty.clone()).collect();
            aggregate_layout(&field_types)
        }
        // A term is transparent over its payload.
        IrType::Term(_, payload) => layout_of(payload),
        IrType::Enum(variants) => enum_layout(variants),
        IrType::Option(inner) => option_layout(inner),
        IrType::Result(ok_ty) => result_layout(ok_ty),
    }
}

/// Refuse a part whose layout is not known until a call.
///
/// Only the descriptor can say where things are in a dynamic type; an answer
/// worked out from its fat pointer's layout would be wrong without saying so.
fn assert_static<'a>(mut parts: impl Iterator<Item = &'a IrType>, what: &str) {
    if let Some(part) = parts.find(|p| p.is_dynamic()) {
        panic!("the layout of {what} holding {part}, which is dynamic, was asked of its static type");
    }
}

fn struct_layout<T>() -> TypeLayout {
    TypeLayout {
        size: std::mem::size_of::<T>() as u32,
        align: std::mem::align_of::<T>() as u32,
    }
}

/// Layout of a tuple or struct: fields in order, each at its own alignment.
pub fn aggregate_layout(fields: &[IrType]) -> TypeLayout {
    assert_static(fields.iter(), "a tuple or struct");
    let mut offset = 0u32;
    let mut max_align = 1u32;

    for field in fields {
        let field_layout = layout_of(field);
        offset = align_up(offset, field_layout.align);
        offset += field_layout.size;
        max_align = max_align.max(field_layout.align);
    }

    TypeLayout {
        size: align_up(offset, max_align),
        align: max_align,
    }
}

/// Byte offset of each field of a tuple or struct.
pub fn aggregate_field_offsets(fields: &[IrType]) -> Vec<u32> {
    assert_static(fields.iter(), "a tuple or struct");
    let mut offsets = Vec::with_capacity(fields.len());
    let mut offset = 0u32;

    for field in fields {
        let field_layout = layout_of(field);
        offset = align_up(offset, field_layout.align);
        offsets.push(offset);
        offset += field_layout.size;
    }

    offsets
}

/// Layout of an enum: a u32 discriminant followed by the largest payload.
pub fn enum_layout(variants: &[(String, Option<IrType>)]) -> TypeLayout {
    assert_static(variants.iter().filter_map(|(_, p)| p.as_ref()), "an enum");
    let mut max_payload_size = 0u32;
    let mut max_payload_align = ENUM_DISCRIMINANT_ALIGN;

    for (_, payload) in variants {
        if let Some(payload_ty) = payload {
            let payload_layout = layout_of(payload_ty);
            max_payload_size = max_payload_size.max(payload_layout.size);
            max_payload_align = max_payload_align.max(payload_layout.align);
        }
    }

    let payload_offset = align_up(ENUM_DISCRIMINANT_SIZE, max_payload_align);
    TypeLayout {
        size: align_up(payload_offset + max_payload_size, max_payload_align),
        align: max_payload_align,
    }
}

/// Byte offset of each enum variant's payload.
///
/// A payload sits at the first offset past the discriminant that satisfies
/// that variant's own alignment, so variants of differing alignment have
/// differing offsets. Variants without a payload get 0.
pub fn enum_variant_offsets(variants: &[(String, Option<IrType>)]) -> Vec<u32> {
    assert_static(variants.iter().filter_map(|(_, p)| p.as_ref()), "an enum");
    variants.iter().map(|(_, payload)| {
        match payload {
            Some(payload_ty) => enum_payload_offset(payload_ty),
            None => 0,
        }
    }).collect()
}

/// Byte offset of an enum variant's payload, given its payload type.
pub fn enum_payload_offset(payload_ty: &IrType) -> u32 {
    assert_static(std::iter::once(payload_ty), "an enum payload");
    align_up(ENUM_DISCRIMINANT_SIZE, layout_of(payload_ty).align)
}

/// Byte offset of the payload of a `?T`.
pub fn option_payload_offset(inner: &IrType) -> u32 {
    assert_static(std::iter::once(inner), "an option");
    align_up(OPTION_TAG_SIZE, layout_of(inner).align)
}

/// Layout of a `?T`: a tag byte followed by the payload.
pub fn option_layout(inner: &IrType) -> TypeLayout {
    assert_static(std::iter::once(inner), "an option");
    let inner_layout = layout_of(inner);
    let payload_offset = align_up(OPTION_TAG_SIZE, inner_layout.align);
    let align = inner_layout.align.max(1);
    TypeLayout {
        size: align_up(payload_offset + inner_layout.size, align),
        align,
    }
}

/// Byte offset of the payload of a `!T`.
///
/// Both arms share the offset, so it accounts for the error payload as well
/// as the ok payload.
pub fn result_payload_offset(ok_ty: &IrType) -> u32 {
    assert_static(std::iter::once(ok_ty), "a result");
    let align = layout_of(ok_ty).align.max(error_layout().align);
    align_up(RESULT_TAG_SIZE, align)
}

/// Layout of a `!T`: a tag byte followed by the larger of ok and error.
pub fn result_layout(ok_ty: &IrType) -> TypeLayout {
    assert_static(std::iter::once(ok_ty), "a result");
    let ok_layout = layout_of(ok_ty);
    let err_layout = error_layout();

    let max_payload_size = ok_layout.size.max(err_layout.size);
    let max_payload_align = ok_layout.align.max(err_layout.align);

    let payload_offset = align_up(RESULT_TAG_SIZE, max_payload_align);
    let align = max_payload_align.max(1);
    TypeLayout {
        size: align_up(payload_offset + max_payload_size, align),
        align,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A container's layout does not depend on its element type.
    ///
    /// A container of a type parameter is held behind a fat pointer like any
    /// dynamic type, but its header is the same few words whatever it holds;
    /// this is what would let one live in the frame with only its descriptor
    /// beside it (the plan's "inline containers").
    #[test]
    fn a_container_is_the_same_shape_whatever_it_holds() {
        let cases: Vec<(IrType, IrType)> = vec![
            (IrType::List(Box::new(IrType::U32)), IrType::List(Box::new(IrType::String))),
            (IrType::Set(Box::new(IrType::String)), IrType::Set(Box::new(IrType::U8))),
            (
                IrType::Map(Box::new(IrType::U32), Box::new(IrType::String)),
                IrType::Map(Box::new(IrType::String), Box::new(IrType::U64)),
            ),
            (
                IrType::Table(vec![("c".to_string(), Box::new(IrType::U8))]),
                IrType::Table(vec![("c".to_string(), Box::new(IrType::String))]),
            ),
        ];
        for (a, b) in cases {
            assert_eq!(
                layout_of(&a), layout_of(&b),
                "{:?} and {:?} should occupy the same slot", a, b,
            );
        }
    }
}
