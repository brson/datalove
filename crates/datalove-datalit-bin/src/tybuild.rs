//! Type descriptors made at run time, from the binary encoding of types.
//!
//! [`TyArena`] decodes a type's encoding and builds its descriptors, which it
//! owns along with the names they point at. It needs no salsa database:
//! every size and offset comes from [`datalove_rtdt::layout`], as the other
//! descriptor builders' do, so a value made with these descriptors is laid
//! out as compiled code and the runtime expect.
//!
//! Descriptors are handed out as raw pointers and live as long as the arena.
//! Keeping the arena alive while any value made with them is, is the
//! caller's to do: a `data` holds a raw pointer to its descriptor, and
//! destroying it reads it.

use std::cell::UnsafeCell;
use std::collections::BTreeMap;

use crate::read::{
    DecodeError, DecodeErrorKind, Reader, read_name, read_varint, read_varint_usize,
};
use datalove_rtdt::tyenc::{tag, type_bytes};
use datalove_rtdt::*;
use std::option::Option;
use std::result::Result;
use std::string::String;

/// How deeply a type may nest before the decoder refuses it.
pub const MAX_TYPE_DEPTH: usize = 256;

/// A type read from its encoding, before it has descriptors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TyTree {
    Scalar(TyTag),
    Tuple(Vec<TyTree>),
    Struct(Vec<(String, TyTree)>),
    Enum(Vec<(String, Option<TyTree>)>),
    Atom(String),
    Term(String, Box<TyTree>),
    List(Box<TyTree>),
    Set(Box<TyTree>),
    Map(Box<TyTree>, Box<TyTree>),
    Tensor(Box<TyTree>, u32),
    Table(Vec<(String, TyTree)>),
    Option(Box<TyTree>),
    Result(Box<TyTree>),
    /// A binder the body may refer back to.
    Rec(Box<TyTree>),
    /// A reference to an enclosing binder by de Bruijn index.
    Var(u32),
}

/// Read a type's encoding into a tree, checking everything but canonical
/// form, which needs the descriptors.
pub fn read_type_tree(r: &mut Reader<'_>) -> Result<TyTree, DecodeError> {
    // Each binder in scope, and whether a collection lies between it and here.
    let mut binders: Vec<bool> = Vec::new();
    read_tree(r, &mut binders, 0)
}

fn read_tree(r: &mut Reader<'_>, binders: &mut Vec<bool>, depth: usize) -> Result<TyTree, DecodeError> {
    if depth > MAX_TYPE_DEPTH {
        return Err(r.error(DecodeErrorKind::TooDeep));
    }
    let at = r.pos();
    let t = r.byte()?;
    let scalar = |tag: TyTag| Ok(TyTree::Scalar(tag));
    // The types inside a collection may refer to any binder in scope.
    let collection = |r: &mut Reader<'_>, binders: &mut Vec<bool>| {
        let saved = binders.clone();
        binders.iter_mut().for_each(|guarded| *guarded = true);
        let tree = read_tree(r, binders, depth + 1);
        *binders = saved;
        tree
    };
    match t {
        tag::BOOL => scalar(TyTag::Bool),
        tag::U8 => scalar(TyTag::U8),
        tag::I8 => scalar(TyTag::I8),
        tag::U16 => scalar(TyTag::U16),
        tag::I16 => scalar(TyTag::I16),
        tag::U32 => scalar(TyTag::U32),
        tag::I32 => scalar(TyTag::I32),
        tag::U64 => scalar(TyTag::U64),
        tag::I64 => scalar(TyTag::I64),
        tag::INDEX => scalar(TyTag::Index),
        tag::OFFSET => scalar(TyTag::Offset),
        tag::F32 => scalar(TyTag::F32),
        tag::F64 => scalar(TyTag::F64),
        tag::INT => scalar(TyTag::Int),
        tag::STRING => scalar(TyTag::String),
        tag::DATA => scalar(TyTag::Data),
        tag::ERROR => scalar(TyTag::Error),
        tag::ATOM => Ok(TyTree::Atom(read_name(r)?)),
        tag::TERM => {
            let name = read_name(r)?;
            Ok(TyTree::Term(name, Box::new(read_tree(r, binders, depth + 1)?)))
        }
        tag::TUPLE => {
            let n = read_count(r)?;
            // Grown as read, not reserved from the count, which the input
            // may claim is anything.
            let mut fields = Vec::with_capacity(n.min(64));
            for _ in 0..n {
                fields.push(read_tree(r, binders, depth + 1)?);
            }
            Ok(TyTree::Tuple(fields))
        }
        tag::STRUCT => Ok(TyTree::Struct(read_named(r, binders, depth)?)),
        tag::TABLE => {
            let n = read_count(r)?;
            let mut columns = Vec::with_capacity(n.min(64));
            for _ in 0..n {
                let name_at = r.pos();
                let name = read_name(r)?;
                if columns.iter().any(|(c, _): &(String, TyTree)| *c == name) {
                    return Err(DecodeError { at: name_at, kind: DecodeErrorKind::DuplicateName });
                }
                columns.push((name, collection(r, binders)?));
            }
            Ok(TyTree::Table(columns))
        }
        tag::ENUM => {
            let n = read_count(r)?;
            let mut variants: Vec<(String, Option<TyTree>)> = Vec::with_capacity(n.min(64));
            for _ in 0..n {
                let name_at = r.pos();
                let name = read_name(r)?;
                if variants.last().is_some_and(|(prev, _)| prev.as_str() >= name.as_str()) {
                    return Err(DecodeError { at: name_at, kind: DecodeErrorKind::NotCanonical });
                }
                let payload = if r.flag()? { Some(read_tree(r, binders, depth + 1)?) } else { None };
                variants.push((name, payload));
            }
            Ok(TyTree::Enum(variants))
        }
        tag::LIST => Ok(TyTree::List(Box::new(collection(r, binders)?))),
        tag::SET => Ok(TyTree::Set(Box::new(collection(r, binders)?))),
        tag::MAP => {
            let key = collection(r, binders)?;
            let value = collection(r, binders)?;
            Ok(TyTree::Map(Box::new(key), Box::new(value)))
        }
        tag::TENSOR => {
            let element = collection(r, binders)?;
            let rank_at = r.pos();
            let rank = read_varint(r)?;
            if rank == 0 || rank > u32::MAX as u64 {
                return Err(DecodeError { at: rank_at, kind: DecodeErrorKind::OutOfRange });
            }
            Ok(TyTree::Tensor(Box::new(element), rank as u32))
        }
        tag::OPTION => Ok(TyTree::Option(Box::new(read_tree(r, binders, depth + 1)?))),
        tag::RESULT => Ok(TyTree::Result(Box::new(read_tree(r, binders, depth + 1)?))),
        tag::REC => {
            binders.push(false);
            let body = read_tree(r, binders, depth + 1);
            binders.pop();
            Ok(TyTree::Rec(Box::new(body?)))
        }
        tag::VAR => {
            let index_at = r.pos();
            let index = read_varint(r)?;
            if index >= binders.len() as u64 {
                return Err(DecodeError { at: index_at, kind: DecodeErrorKind::OutOfRange });
            }
            if !binders[binders.len() - 1 - index as usize] {
                return Err(DecodeError { at, kind: DecodeErrorKind::Unguarded });
            }
            Ok(TyTree::Var(index as u32))
        }
        other => Err(DecodeError { at, kind: DecodeErrorKind::BadByte(other) }),
    }
}

/// Read a count of parts. Each part takes at least a byte, so a count
/// larger than the input fails when the input runs out.
fn read_count(r: &mut Reader<'_>) -> Result<usize, DecodeError> {
    let at = r.pos();
    let n = read_varint_usize(r)?;
    if n > u32::MAX as usize {
        return Err(DecodeError { at, kind: DecodeErrorKind::OutOfRange });
    }
    Ok(n)
}

fn read_named(
    r: &mut Reader<'_>,
    binders: &mut Vec<bool>,
    depth: usize,
) -> Result<Vec<(String, TyTree)>, DecodeError> {
    let n = read_count(r)?;
    let mut fields: Vec<(String, TyTree)> = Vec::with_capacity(n.min(64));
    for _ in 0..n {
        let name_at = r.pos();
        let name = read_name(r)?;
        if fields.iter().any(|(f, _)| *f == name) {
            return Err(DecodeError { at: name_at, kind: DecodeErrorKind::DuplicateName });
        }
        fields.push((name, read_tree(r, binders, depth + 1)?));
    }
    Ok(fields)
}

/// Descriptors made at run time, and everything they point at.
pub struct TyArena {
    tydescs: Vec<Box<TyDesc>>,
    rec_tydescs: Vec<Box<UnsafeCell<TyDesc>>>,
    names: Vec<Box<str>>,
    tuple_fields: Vec<Vec<TyInfoTupleField>>,
    struct_fields: Vec<Vec<TyInfoStructField>>,
    enum_variants: Vec<Vec<TyInfoEnumVariant>>,
    table_columns: Vec<Vec<TyInfoTableColumn>>,
    /// Every type decoded, by its encoding, which is canonical.
    decoded: BTreeMap<Vec<u8>, *const TyDesc>,
    /// The scalars made, by tag.
    scalars: BTreeMap<u8, *const TyDesc>,
    /// The tuples made, by their fields' descriptors.
    tuples: BTreeMap<Vec<usize>, *const TyDesc>,
}

impl Default for TyArena {
    fn default() -> Self {
        TyArena::new()
    }
}

impl TyArena {
    pub fn new() -> TyArena {
        TyArena {
            tydescs: Vec::new(),
            rec_tydescs: Vec::new(),
            names: Vec::new(),
            tuple_fields: Vec::new(),
            struct_fields: Vec::new(),
            enum_variants: Vec::new(),
            table_columns: Vec::new(),
            decoded: BTreeMap::new(),
            scalars: BTreeMap::new(),
            tuples: BTreeMap::new(),
        }
    }

    /// Read a type's encoding and return its descriptor.
    ///
    /// The encoding must be canonical: the one [`datalove_rtdt::tyenc::encode_type`]
    /// writes for the type. A type read before is found again rather than
    /// built twice.
    pub fn read_type(&mut self, r: &mut Reader<'_>) -> Result<*const TyDesc, DecodeError> {
        let start = r.pos();
        let tree = read_type_tree(r)?;
        let bytes = r.since(start);
        if let Some(&td) = self.decoded.get(bytes) {
            return Ok(td);
        }
        let td = self.build(&tree);
        // SAFETY: just built, and owned by this arena.
        if type_bytes(unsafe { TyDescRef::from_ptr(td) }) != bytes {
            return Err(DecodeError { at: start, kind: DecodeErrorKind::NotCanonical });
        }
        self.decoded.insert(bytes.to_vec(), td);
        Ok(td)
    }

    /// The descriptor of a type encoded in `bytes`, which hold nothing else.
    pub fn type_from_bytes(&mut self, bytes: &[u8]) -> Result<*const TyDesc, DecodeError> {
        let mut r = Reader::new(bytes);
        let td = self.read_type(&mut r)?;
        r.finish()?;
        Ok(td)
    }

    /// Build the descriptor of a type tree, which must be closed: every
    /// reference refers to a binder within it, through a collection.
    pub fn build(&mut self, tree: &TyTree) -> *const TyDesc {
        self.build_in(tree, &mut Vec::new())
    }

    /// The descriptor of a tuple of the types given.
    pub fn tuple(&mut self, fields: &[*const TyDesc]) -> *const TyDesc {
        let key: Vec<usize> = fields.iter().map(|&f| f as usize).collect();
        if let Some(&td) = self.tuples.get(&key) {
            return td;
        }
        let temp: Vec<TyInfoTupleField> = fields.iter().map(|&tydesc| TyInfoTupleField { offset: 0, tydesc }).collect();
        let probe = TyDesc {
            type_tag: TyTag::Tuple,
            flags: 0,
            size: 0,
            align: 1,
            type_info: TyInfo { tuple: TyInfoTuple { num_fields: temp.len() as u32, fields: temp.as_ptr() } },
        };
        let layout = layout::compute_tuple_layout(TyDescRef::from_ref(&probe));
        let info: Vec<TyInfoTupleField> = fields.iter().zip(&layout.field_offsets)
            .map(|(&tydesc, &offset)| TyInfoTupleField { offset, tydesc })
            .collect();
        self.tuple_fields.push(info);
        let stored = self.tuple_fields.last().expect("just pushed");
        let td = self.keep(TyDesc {
            type_tag: TyTag::Tuple,
            flags: 0,
            size: layout.size,
            align: layout.align,
            type_info: TyInfo { tuple: TyInfoTuple { num_fields: stored.len() as u32, fields: stored.as_ptr() } },
        });
        self.tuples.insert(key, td);
        td
    }

    fn keep(&mut self, mut tydesc: TyDesc) -> *const TyDesc {
        tydesc.flags = tydesc.computed_flags();
        self.tydescs.push(Box::new(tydesc));
        &**self.tydescs.last().expect("just pushed") as *const TyDesc
    }

    /// A name kept for the life of the arena.
    fn name(&mut self, name: &str) -> (*const u8, u32) {
        self.names.push(name.into());
        let kept = self.names.last().expect("just pushed");
        (kept.as_ptr(), kept.len() as u32)
    }

    /// The descriptor of a type with no parts: a number, `int`, `string`,
    /// `data` or `error`.
    pub fn scalar(&mut self, t: TyTag) -> *const TyDesc {
        if let Some(&td) = self.scalars.get(&(t as u8)) {
            return td;
        }
        let (size, align) = match t {
            TyTag::Bool | TyTag::U8 | TyTag::I8 => (1, 1),
            TyTag::U16 | TyTag::I16 => (2, 2),
            TyTag::U32 | TyTag::I32 | TyTag::F32 => (4, 4),
            TyTag::U64 | TyTag::I64 | TyTag::F64 => (8, 8),
            TyTag::Index | TyTag::Offset => (INDEX_SIZE, INDEX_ALIGN),
            TyTag::Int => (std::mem::size_of::<Int>() as u32, std::mem::align_of::<Int>() as u32),
            TyTag::String => (std::mem::size_of::<datalove_rtdt::String>() as u32, std::mem::align_of::<datalove_rtdt::String>() as u32),
            TyTag::Data => (std::mem::size_of::<Data>() as u32, std::mem::align_of::<Data>() as u32),
            TyTag::Error => (std::mem::size_of::<Error>() as u32, std::mem::align_of::<Error>() as u32),
            other => unreachable!("{other:?} is not a scalar"),
        };
        let td = self.keep(TyDesc {
            type_tag: t,
            flags: 0,
            size,
            align,
            type_info: TyInfo { nothing: TyInfoNothing { unused: 0 } },
        });
        self.scalars.insert(t as u8, td);
        td
    }

    fn build_in(&mut self, tree: &TyTree, env: &mut Vec<*const TyDesc>) -> *const TyDesc {
        match tree {
            TyTree::Scalar(t) => self.scalar(*t),
            TyTree::Var(index) => env[env.len() - 1 - *index as usize],
            TyTree::Rec(body) => self.build_rec(body, env),
            TyTree::Tuple(fields) => {
                let tds: Vec<_> = fields.iter().map(|f| self.build_in(f, env)).collect();
                self.tuple(&tds)
            }
            TyTree::Struct(fields) => {
                let tds: Vec<_> = fields.iter().map(|(_, f)| self.build_in(f, env)).collect();
                let names: Vec<_> = fields.iter().map(|(n, _)| self.name(n)).collect();
                let mut info: Vec<TyInfoStructField> = names.iter().zip(&tds)
                    .map(|(&(name, name_len), &tydesc)| TyInfoStructField { name, name_len, offset: 0, tydesc })
                    .collect();
                let probe = TyDesc {
                    type_tag: TyTag::Struct,
                    flags: 0,
                    size: 0,
                    align: 1,
                    type_info: TyInfo { struct_: TyInfoStruct { num_fields: info.len() as u32, fields: info.as_ptr() } },
                };
                let layout = layout::compute_struct_layout(TyDescRef::from_ref(&probe));
                for (field, offset) in info.iter_mut().zip(&layout.field_offsets) {
                    field.offset = *offset;
                }
                self.struct_fields.push(info);
                let stored = self.struct_fields.last().expect("just pushed");
                let type_info = TyInfo { struct_: TyInfoStruct { num_fields: stored.len() as u32, fields: stored.as_ptr() } };
                self.keep(TyDesc { type_tag: TyTag::Struct, flags: 0, size: layout.size, align: layout.align, type_info })
            }
            TyTree::Enum(variants) => {
                let payloads: Vec<*const TyDesc> = variants.iter()
                    .map(|(_, p)| p.as_ref().map_or(std::ptr::null(), |p| self.build_in(p, env)))
                    .collect();
                let names: Vec<_> = variants.iter().map(|(n, _)| self.name(n)).collect();
                let mut info: Vec<TyInfoEnumVariant> = names.iter().zip(&payloads)
                    .map(|(&(name, name_len), &payload)| TyInfoEnumVariant { name, name_len, offset: 0, payload })
                    .collect();
                let probe = TyDesc {
                    type_tag: TyTag::Enum,
                    flags: 0,
                    size: 0,
                    align: 1,
                    type_info: TyInfo { enum_: TyInfoEnum { variants: info.as_ptr(), num_variants: info.len() as u32 } },
                };
                let layout = layout::compute_enum_layout(TyDescRef::from_ref(&probe));
                for (variant, offset) in info.iter_mut().zip(&layout.variant_offsets) {
                    variant.offset = *offset;
                }
                self.enum_variants.push(info);
                let stored = self.enum_variants.last().expect("just pushed");
                let type_info = TyInfo { enum_: TyInfoEnum { variants: stored.as_ptr(), num_variants: stored.len() as u32 } };
                self.keep(TyDesc { type_tag: TyTag::Enum, flags: 0, size: layout.size, align: layout.align, type_info })
            }
            TyTree::Atom(name) => {
                let (name, name_len) = self.name(name);
                self.keep(TyDesc {
                    type_tag: TyTag::Atom,
                    flags: 0,
                    size: 0,
                    align: 1,
                    type_info: TyInfo { atom: TyInfoAtom { name, name_len } },
                })
            }
            TyTree::Term(name, payload) => {
                let payload = self.build_in(payload, env);
                // SAFETY: built by this arena.
                let (size, align) = unsafe { ((*payload).size, (*payload).align) };
                let (name, name_len) = self.name(name);
                self.keep(TyDesc {
                    type_tag: TyTag::Term,
                    flags: 0,
                    size,
                    align,
                    type_info: TyInfo { term: TyInfoTerm { name, name_len, payload } },
                })
            }
            TyTree::List(element) => {
                let element_tydesc = self.build_in(element, env);
                self.keep(TyDesc {
                    type_tag: TyTag::List,
                    flags: 0,
                    size: std::mem::size_of::<List>() as u32,
                    align: std::mem::align_of::<List>() as u32,
                    type_info: TyInfo { list: TyInfoList { element_tydesc } },
                })
            }
            TyTree::Set(element) => {
                let element = self.build_in(element, env);
                self.keep(TyDesc {
                    type_tag: TyTag::Set,
                    flags: 0,
                    size: std::mem::size_of::<Set>() as u32,
                    align: std::mem::align_of::<Set>() as u32,
                    // SAFETY: the element's descriptor is built.
                    type_info: TyInfo { set: unsafe { TyInfoSet::new(element) } },
                })
            }
            TyTree::Map(key, value) => {
                let key = self.build_in(key, env);
                let value = self.build_in(value, env);
                self.keep(TyDesc {
                    type_tag: TyTag::Map,
                    flags: 0,
                    size: std::mem::size_of::<Map>() as u32,
                    align: std::mem::align_of::<Map>() as u32,
                    // SAFETY: both descriptors are built.
                    type_info: TyInfo { map: unsafe { TyInfoMap::new(key, value) } },
                })
            }
            TyTree::Tensor(element, rank) => {
                let element_tydesc = self.build_in(element, env);
                self.keep(TyDesc {
                    type_tag: TyTag::Tensor,
                    flags: 0,
                    size: std::mem::size_of::<Tensor>() as u32,
                    align: std::mem::align_of::<Tensor>() as u32,
                    type_info: TyInfo { tensor: TyInfoTensor { element_tydesc, rank: *rank } },
                })
            }
            TyTree::Table(columns) => {
                let tds: Vec<_> = columns.iter().map(|(_, c)| self.build_in(c, env)).collect();
                let names: Vec<_> = columns.iter().map(|(n, _)| self.name(n)).collect();
                let info: Vec<TyInfoTableColumn> = names.iter().zip(&tds)
                    .map(|(&(name, name_len), &tydesc)| TyInfoTableColumn { name, name_len, tydesc })
                    .collect();
                self.table_columns.push(info);
                let stored = self.table_columns.last().expect("just pushed");
                let type_info = TyInfo { table: TyInfoTable { num_columns: stored.len() as u32, columns: stored.as_ptr() } };
                self.keep(TyDesc {
                    type_tag: TyTag::Table,
                    flags: 0,
                    size: std::mem::size_of::<Table>() as u32,
                    align: std::mem::align_of::<Table>() as u32,
                    type_info,
                })
            }
            TyTree::Option(inner) => {
                let inner_tydesc = self.build_in(inner, env);
                let type_info = || TyInfo { option: TyInfoOption { inner_tydesc } };
                let probe = TyDesc { type_tag: TyTag::Option, flags: 0, size: 0, align: 1, type_info: type_info() };
                let layout = layout::compute_option_layout(TyDescRef::from_ref(&probe));
                self.keep(TyDesc { type_tag: TyTag::Option, flags: 0, size: layout.size, align: layout.align, type_info: type_info() })
            }
            TyTree::Result(ok) => {
                let ok_tydesc = self.build_in(ok, env);
                let type_info = || TyInfo { result: TyInfoResult { ok_tydesc } };
                let probe = TyDesc { type_tag: TyTag::Result, flags: 0, size: 0, align: 1, type_info: type_info() };
                let layout = layout::compute_result_layout(TyDescRef::from_ref(&probe));
                self.keep(TyDesc { type_tag: TyTag::Result, flags: 0, size: layout.size, align: layout.align, type_info: type_info() })
            }
        }
    }

    /// The descriptor of a recursive type, which its body's descriptors point
    /// back to.
    ///
    /// As in datalit's `TyDescTable::create_rec`: the descriptor is a copy of
    /// its body's, made before the body is so that the body can point at it,
    /// and written after. In between, what is built reads its size and
    /// alignment, so it stands as the body built once already with the binder
    /// a stand-in. That first build is right about both: the type recurs only
    /// inside collections, whose size does not depend on what they hold.
    fn build_rec(&mut self, body: &TyTree, env: &mut Vec<*const TyDesc>) -> *const TyDesc {
        let stand_in = self.tuple(&[]);
        env.push(stand_in);
        let first = self.build_in(body, env);
        env.pop();

        // SAFETY: `first` is a live descriptor of this arena, and a descriptor
        // owns nothing, so a copy of its bits is a descriptor too.
        self.rec_tydescs.push(Box::new(UnsafeCell::new(unsafe { std::ptr::read(first) })));
        let cell = self.rec_tydescs.last().expect("just pushed").get();

        env.push(cell as *const TyDesc);
        let built = self.build_in(body, env);
        env.pop();
        // SAFETY: both are live descriptors of this arena, and nothing holds a
        // reference to the cell, only pointers.
        unsafe { std::ptr::copy_nonoverlapping(built, cell, 1) };
        cell as *const TyDesc
    }
}

/// Check that two descriptors of one type lay it out alike: every size,
/// alignment, offset, node layout and flag, all the way down.
///
/// For the conformance tests that hold the descriptor builders to each other,
/// since a value made with one is read with another. The error says where they
/// first differ.
pub fn layouts_agree(a: TyDescRef<'_>, b: TyDescRef<'_>) -> Result<(), String> {
    agree_in(a, b, &mut Vec::new(), "root")
}

fn agree_in(a: TyDescRef<'_>, b: TyDescRef<'_>, assumed: &mut Vec<(usize, usize)>, path: &str) -> Result<(), String> {
    if std::ptr::eq(a.as_ptr(), b.as_ptr()) {
        return Ok(());
    }
    let pair = (a.as_ptr() as usize, b.as_ptr() as usize);
    if assumed.contains(&pair) {
        return Ok(());
    }
    let (da, db) = (a.as_ref(), b.as_ref());
    let head = |what: &str, x: &dyn std::fmt::Debug, y: &dyn std::fmt::Debug| {
        format!("{path}: {what} {x:?} against {y:?}")
    };
    if da.type_tag != db.type_tag {
        return Err(head("tag", &da.type_tag, &db.type_tag));
    }
    if (da.size, da.align, da.flags) != (db.size, db.align, db.flags) {
        return Err(head("size, align and flags", &(da.size, da.align, da.flags), &(db.size, db.align, db.flags)));
    }
    assumed.push(pair);
    let result = agree_parts(a, b, assumed, path);
    assumed.pop();
    result
}

fn agree_parts(a: TyDescRef<'_>, b: TyDescRef<'_>, assumed: &mut Vec<(usize, usize)>, path: &str) -> Result<(), String> {
    let mismatch = |what: &str| Err(format!("{path}: {what} differ"));
    match a.type_tag() {
        TyTag::Tuple => {
            let (fa, fb): (Vec<_>, Vec<_>) = (a.iter_tuple_fields().collect(), b.iter_tuple_fields().collect());
            if fa.len() != fb.len() {
                return mismatch("field counts");
            }
            for (i, (x, y)) in fa.iter().zip(&fb).enumerate() {
                if x.offset() != y.offset() {
                    return mismatch(&format!("offsets of field {i}"));
                }
                agree_in(x.tydesc(), y.tydesc(), assumed, &format!("{path}.{i}"))?;
            }
            Ok(())
        }
        TyTag::Struct => {
            let (fa, fb): (Vec<_>, Vec<_>) = (a.iter_struct_fields().collect(), b.iter_struct_fields().collect());
            if fa.len() != fb.len() {
                return mismatch("field counts");
            }
            for (x, y) in fa.iter().zip(&fb) {
                if x.name() != y.name() || x.offset() != y.offset() {
                    return mismatch(&format!("fields {} and {}", x.name(), y.name()));
                }
                agree_in(x.tydesc(), y.tydesc(), assumed, &format!("{path}.{}", x.name()))?;
            }
            Ok(())
        }
        TyTag::Enum => {
            let (va, vb): (Vec<_>, Vec<_>) = (a.iter_enum_variants().collect(), b.iter_enum_variants().collect());
            if va.len() != vb.len() {
                return mismatch("variant counts");
            }
            for (x, y) in va.iter().zip(&vb) {
                if x.name() != y.name() || x.offset() != y.offset() {
                    return mismatch(&format!("variants {} and {}", x.name(), y.name()));
                }
                match (x.payload(), y.payload()) {
                    (None, None) => {}
                    (Some(px), Some(py)) => agree_in(px, py, assumed, &format!("{path}.{}", x.name()))?,
                    _ => return mismatch(&format!("payloads of {}", x.name())),
                }
            }
            Ok(())
        }
        TyTag::Atom => if a.atom_info().0 == b.atom_info().0 { Ok(()) } else { mismatch("atom names") },
        TyTag::Term => {
            if a.term_info().0 != b.term_info().0 {
                return mismatch("term names");
            }
            agree_in(a.term_info().1, b.term_info().1, assumed, &format!("{path}.term"))
        }
        TyTag::List => agree_in(a.list_element_ty(), b.list_element_ty(), assumed, &format!("{path}[]")),
        TyTag::Set => {
            if (a.set_leaf_layout(), a.set_internal_layout()) != (b.set_leaf_layout(), b.set_internal_layout()) {
                return mismatch("set node layouts");
            }
            agree_in(a.set_element_ty(), b.set_element_ty(), assumed, &format!("{path}#{{}}"))
        }
        TyTag::Map => {
            if (a.map_leaf_layout(), a.map_internal_layout()) != (b.map_leaf_layout(), b.map_internal_layout()) {
                return mismatch("map node layouts");
            }
            agree_in(a.map_key_ty(), b.map_key_ty(), assumed, &format!("{path}.key"))?;
            agree_in(a.map_value_ty(), b.map_value_ty(), assumed, &format!("{path}.value"))
        }
        TyTag::Tensor => {
            if a.tensor_rank() != b.tensor_rank() {
                return mismatch("tensor ranks");
            }
            agree_in(a.tensor_element_ty(), b.tensor_element_ty(), assumed, &format!("{path}[||]"))
        }
        TyTag::Table => {
            let (ca, cb): (Vec<_>, Vec<_>) = (a.table_column_tydescs().collect(), b.table_column_tydescs().collect());
            if ca.len() != cb.len() {
                return mismatch("column counts");
            }
            for (x, y) in ca.iter().zip(&cb) {
                if x.name() != y.name() {
                    return mismatch("column names");
                }
                agree_in(x.tydesc(), y.tydesc(), assumed, &format!("{path}.{}", x.name()))?;
            }
            Ok(())
        }
        TyTag::Option => agree_in(a.option_inner_ty(), b.option_inner_ty(), assumed, &format!("{path}?")),
        TyTag::Result => agree_in(a.result_ok_ty(), b.result_ok_ty(), assumed, &format!("{path}!")),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huge_counts_fail_without_reserving() {
        let mut arena = TyArena::new();
        for t in [tag::TUPLE, tag::STRUCT, tag::ENUM, tag::TABLE] {
            let bytes = [t, 0xFB, 0xFF, 0xFF, 0xFF, 0xFF];
            let err = arena.type_from_bytes(&bytes).unwrap_err();
            assert_eq!(err.kind, DecodeErrorKind::Truncated, "tag {t:#x}");
        }
    }
}
