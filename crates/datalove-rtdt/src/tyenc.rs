//! The binary encoding of datalit types, and the order of types it defines.
//!
//! A type is written as a tag byte for its constructor and then its parts:
//! names as escaped strings, counts and ranks as ordered varints, and the
//! types inside it. The encoding is canonical, so two types are the same
//! exactly when their encodings are, and the order of types is the byte order
//! of their encodings. That is the order a `data` sorts by before its value.
//!
//! A descriptor of a recursive type is a graph with a cycle in it, and the
//! runtime takes two descriptors to be one type when they unfold to the same
//! infinite tree. So the encoding is a function of that tree: walking down
//! from the root, a type equal to one of its own ancestors is written as a
//! reference back to it, and an ancestor referred to is written with a `rec`
//! binder before it. A recursive type and any unfolding of it, at any depth,
//! encode alike.

use crate::ordered::{write_str, write_varint};
use crate::{TyDescRef, TyTag};

/// The tag bytes of the type encoding.
///
/// These are the format, not the runtime's `TyTag` numbering, though they
/// happen to agree with it: changing one must not change the other.
pub mod tag {
    pub const BOOL: u8 = 0x01;
    pub const U8: u8 = 0x10;
    pub const I8: u8 = 0x11;
    pub const U16: u8 = 0x12;
    pub const I16: u8 = 0x13;
    pub const U32: u8 = 0x14;
    pub const I32: u8 = 0x15;
    pub const U64: u8 = 0x16;
    pub const I64: u8 = 0x17;
    pub const INDEX: u8 = 0x18;
    pub const OFFSET: u8 = 0x19;
    pub const F32: u8 = 0x20;
    pub const F64: u8 = 0x21;
    pub const INT: u8 = 0x30;
    pub const TUPLE: u8 = 0x40;
    pub const STRUCT: u8 = 0x41;
    pub const ENUM: u8 = 0x42;
    pub const ATOM: u8 = 0x43;
    pub const TERM: u8 = 0x44;
    pub const LIST: u8 = 0x50;
    pub const STRING: u8 = 0x51;
    pub const MAP: u8 = 0x52;
    pub const SET: u8 = 0x53;
    pub const TENSOR: u8 = 0x54;
    pub const TABLE: u8 = 0x55;
    pub const OPTION: u8 = 0x60;
    pub const RESULT: u8 = 0x61;
    pub const DATA: u8 = 0x70;
    pub const ERROR: u8 = 0x71;
    /// A binder: the type after it may refer back to it.
    pub const REC: u8 = 0x90;
    /// A reference to an enclosing binder, by de Bruijn index.
    pub const VAR: u8 = 0x91;
}

/// Append the encoding of a type.
///
/// # Panics
///
/// On a template's type parameter, which is never the type of a value.
pub fn encode_type(td: TyDescRef<'_>, out: &mut Vec<u8>) {
    let mut ancestors = Vec::new();
    let tree = fold(td, &mut ancestors);
    let mut binders = Vec::new();
    write_tree(&tree, &mut binders, out);
}

/// The encoding of a type.
pub fn type_bytes(td: TyDescRef<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    encode_type(td, &mut out);
    out
}

/// The order of two types: the byte order of their encodings.
pub fn cmp_types(a: TyDescRef<'_>, b: TyDescRef<'_>) -> std::cmp::Ordering {
    if std::ptr::eq(a.as_ptr(), b.as_ptr()) {
        return std::cmp::Ordering::Equal;
    }
    type_bytes(a).cmp(&type_bytes(b))
}

/// Whether two descriptors describe the same type: whether they unfold to the
/// same tree.
///
/// Sizes and offsets are not compared, as they follow from the structure.
pub fn same_type(a: TyDescRef<'_>, b: TyDescRef<'_>) -> bool {
    same_in(a, b, &mut Vec::new())
}

fn same_in(a: TyDescRef<'_>, b: TyDescRef<'_>, assumed: &mut Vec<(usize, usize)>) -> bool {
    if std::ptr::eq(a.as_ptr(), b.as_ptr()) {
        return true;
    }
    if a.type_tag() != b.type_tag() {
        return false;
    }
    // A pair met again inside its own comparison has matched on every step
    // since, and from here would only go round again.
    let pair = (a.as_ptr() as usize, b.as_ptr() as usize);
    if assumed.contains(&pair) {
        return true;
    }
    assumed.push(pair);
    let same = same_parts(a, b, assumed);
    assumed.pop();
    same
}

fn same_parts(a: TyDescRef<'_>, b: TyDescRef<'_>, assumed: &mut Vec<(usize, usize)>) -> bool {
    match a.type_tag() {
        TyTag::Bool | TyTag::U8 | TyTag::I8 | TyTag::U16 | TyTag::I16 | TyTag::U32
        | TyTag::I32 | TyTag::U64 | TyTag::I64 | TyTag::Index | TyTag::Offset | TyTag::F32
        | TyTag::F64 | TyTag::Int | TyTag::String | TyTag::Data | TyTag::Error => true,
        TyTag::Param => unreachable!("a template's parameter is not the type of a value"),
        TyTag::Atom => a.atom_info().0 == b.atom_info().0,
        TyTag::Term => {
            let (name_a, payload_a) = a.term_info();
            let (name_b, payload_b) = b.term_info();
            name_a == name_b && same_in(payload_a, payload_b, assumed)
        }
        TyTag::Tuple => {
            let (fa, fb) = (a.tuple_info(), b.tuple_info());
            fa.num_fields() == fb.num_fields()
                && a.iter_tuple_fields().zip(b.iter_tuple_fields())
                    .all(|(x, y)| same_in(x.tydesc(), y.tydesc(), assumed))
        }
        TyTag::Struct => {
            let (fa, fb) = (a.struct_info(), b.struct_info());
            fa.num_fields() == fb.num_fields()
                && a.iter_struct_fields().zip(b.iter_struct_fields())
                    .all(|(x, y)| x.name() == y.name() && same_in(x.tydesc(), y.tydesc(), assumed))
        }
        TyTag::Enum => {
            let (va, vb) = (sorted_variants(a), sorted_variants(b));
            va.len() == vb.len()
                && va.iter().zip(&vb).all(|((na, pa), (nb, pb))| {
                    na == nb && match (pa, pb) {
                        (None, None) => true,
                        (Some(pa), Some(pb)) => same_in(*pa, *pb, assumed),
                        _ => false,
                    }
                })
        }
        TyTag::Table => {
            a.table_num_columns() == b.table_num_columns()
                && a.table_column_tydescs().zip(b.table_column_tydescs())
                    .all(|(x, y)| x.name() == y.name() && same_in(x.tydesc(), y.tydesc(), assumed))
        }
        TyTag::List => same_in(a.list_element_ty(), b.list_element_ty(), assumed),
        TyTag::Set => same_in(a.set_element_ty(), b.set_element_ty(), assumed),
        TyTag::Map => {
            same_in(a.map_key_ty(), b.map_key_ty(), assumed)
                && same_in(a.map_value_ty(), b.map_value_ty(), assumed)
        }
        TyTag::Tensor => {
            a.tensor_rank() == b.tensor_rank()
                && same_in(a.tensor_element_ty(), b.tensor_element_ty(), assumed)
        }
        TyTag::Option => same_in(a.option_inner_ty(), b.option_inner_ty(), assumed),
        TyTag::Result => same_in(a.result_ok_ty(), b.result_ok_ty(), assumed),
    }
}

/// An enum's variants in name order, the order the encoding writes them in.
pub fn sorted_variants<'a>(td: TyDescRef<'a>) -> Vec<(&'a str, Option<TyDescRef<'a>>)> {
    let mut variants: Vec<_> = td.iter_enum_variants().map(|v| (v.name(), v.payload())).collect();
    variants.sort_by(|a, b| a.0.cmp(b.0));
    variants
}

/// A type as it will be written, with its recursion folded.
enum Folded {
    Node { parts: Vec<Part>, referred_to: bool },
    /// A reference back to the ancestor at this depth.
    Back(usize),
}

enum Part {
    Bytes(Vec<u8>),
    Type(Folded),
}

fn fold<'a>(td: TyDescRef<'a>, ancestors: &mut Vec<(TyDescRef<'a>, bool)>) -> Folded {
    // At most one ancestor can be equal: had two been, the lower would have
    // been found equal to the upper and folded already.
    if let Some(depth) = ancestors.iter().position(|(a, _)| same_type(*a, td)) {
        ancestors[depth].1 = true;
        return Folded::Back(depth);
    }
    ancestors.push((td, false));
    let mut parts = Vec::new();
    let bytes = |parts: &mut Vec<Part>, f: &dyn Fn(&mut Vec<u8>)| {
        let mut b = Vec::new();
        f(&mut b);
        parts.push(Part::Bytes(b));
    };
    let tag_byte = |t: u8| move |b: &mut Vec<u8>| b.push(t);
    match td.type_tag() {
        TyTag::Bool => bytes(&mut parts, &tag_byte(tag::BOOL)),
        TyTag::U8 => bytes(&mut parts, &tag_byte(tag::U8)),
        TyTag::I8 => bytes(&mut parts, &tag_byte(tag::I8)),
        TyTag::U16 => bytes(&mut parts, &tag_byte(tag::U16)),
        TyTag::I16 => bytes(&mut parts, &tag_byte(tag::I16)),
        TyTag::U32 => bytes(&mut parts, &tag_byte(tag::U32)),
        TyTag::I32 => bytes(&mut parts, &tag_byte(tag::I32)),
        TyTag::U64 => bytes(&mut parts, &tag_byte(tag::U64)),
        TyTag::I64 => bytes(&mut parts, &tag_byte(tag::I64)),
        TyTag::Index => bytes(&mut parts, &tag_byte(tag::INDEX)),
        TyTag::Offset => bytes(&mut parts, &tag_byte(tag::OFFSET)),
        TyTag::F32 => bytes(&mut parts, &tag_byte(tag::F32)),
        TyTag::F64 => bytes(&mut parts, &tag_byte(tag::F64)),
        TyTag::Int => bytes(&mut parts, &tag_byte(tag::INT)),
        TyTag::String => bytes(&mut parts, &tag_byte(tag::STRING)),
        TyTag::Data => bytes(&mut parts, &tag_byte(tag::DATA)),
        TyTag::Error => bytes(&mut parts, &tag_byte(tag::ERROR)),
        TyTag::Param => unreachable!("a template's parameter is not the type of a value"),
        TyTag::Atom => bytes(&mut parts, &|b| {
            b.push(tag::ATOM);
            write_str(b, td.atom_info().0);
        }),
        TyTag::Term => {
            let (name, payload) = td.term_info();
            bytes(&mut parts, &|b| {
                b.push(tag::TERM);
                write_str(b, name);
            });
            parts.push(Part::Type(fold(payload, ancestors)));
        }
        TyTag::Tuple => {
            bytes(&mut parts, &|b| {
                b.push(tag::TUPLE);
                write_varint(b, td.tuple_info().num_fields() as u64);
            });
            for field in td.iter_tuple_fields() {
                parts.push(Part::Type(fold(field.tydesc(), ancestors)));
            }
        }
        TyTag::Struct => {
            bytes(&mut parts, &|b| {
                b.push(tag::STRUCT);
                write_varint(b, td.struct_info().num_fields() as u64);
            });
            for field in td.iter_struct_fields() {
                bytes(&mut parts, &|b| write_str(b, field.name()));
                parts.push(Part::Type(fold(field.tydesc(), ancestors)));
            }
        }
        TyTag::Enum => {
            let variants = sorted_variants(td);
            bytes(&mut parts, &|b| {
                b.push(tag::ENUM);
                write_varint(b, variants.len() as u64);
            });
            for (name, payload) in variants {
                bytes(&mut parts, &|b| {
                    write_str(b, name);
                    b.push(payload.is_some() as u8);
                });
                if let Some(payload) = payload {
                    parts.push(Part::Type(fold(payload, ancestors)));
                }
            }
        }
        TyTag::Table => {
            bytes(&mut parts, &|b| {
                b.push(tag::TABLE);
                write_varint(b, td.table_num_columns() as u64);
            });
            for column in td.table_column_tydescs() {
                bytes(&mut parts, &|b| write_str(b, column.name()));
                parts.push(Part::Type(fold(column.tydesc(), ancestors)));
            }
        }
        TyTag::List => {
            bytes(&mut parts, &tag_byte(tag::LIST));
            parts.push(Part::Type(fold(td.list_element_ty(), ancestors)));
        }
        TyTag::Set => {
            bytes(&mut parts, &tag_byte(tag::SET));
            parts.push(Part::Type(fold(td.set_element_ty(), ancestors)));
        }
        TyTag::Map => {
            bytes(&mut parts, &tag_byte(tag::MAP));
            parts.push(Part::Type(fold(td.map_key_ty(), ancestors)));
            parts.push(Part::Type(fold(td.map_value_ty(), ancestors)));
        }
        TyTag::Tensor => {
            bytes(&mut parts, &tag_byte(tag::TENSOR));
            parts.push(Part::Type(fold(td.tensor_element_ty(), ancestors)));
            bytes(&mut parts, &|b| write_varint(b, td.tensor_rank() as u64));
        }
        TyTag::Option => {
            bytes(&mut parts, &tag_byte(tag::OPTION));
            parts.push(Part::Type(fold(td.option_inner_ty(), ancestors)));
        }
        TyTag::Result => {
            bytes(&mut parts, &tag_byte(tag::RESULT));
            parts.push(Part::Type(fold(td.result_ok_ty(), ancestors)));
        }
    }
    let (_, referred_to) = ancestors.pop().expect("pushed above");
    Folded::Node { parts, referred_to }
}

/// Write a folded type. `binders` holds, for each ancestor, whether it was
/// written with a binder.
fn write_tree(tree: &Folded, binders: &mut Vec<bool>, out: &mut Vec<u8>) {
    match tree {
        Folded::Back(depth) => {
            debug_assert!(binders[*depth], "a reference is to a binder");
            let index = binders[depth + 1..].iter().filter(|&&b| b).count();
            out.push(tag::VAR);
            write_varint(out, index as u64);
        }
        Folded::Node { parts, referred_to } => {
            if *referred_to {
                out.push(tag::REC);
            }
            binders.push(*referred_to);
            for part in parts {
                match part {
                    Part::Bytes(b) => out.extend_from_slice(b),
                    Part::Type(t) => write_tree(t, binders, out),
                }
            }
            binders.pop();
        }
    }
}
