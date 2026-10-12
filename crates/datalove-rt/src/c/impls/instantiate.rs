//! Descriptors built at run time, from templates.
//!
//! A generic is compiled once whatever its type parameters are bound to, and
//! is handed one descriptor per type parameter. The descriptor of anything it
//! writes in terms of them -- a `(T, u32)`, a `[?T]` -- is not known until a
//! call, so the compiler emits a template for it: the static descriptor of
//! the type with a `TyTag::Param` wherever a parameter stood. This module
//! instantiates one against the descriptors a call was handed.
//!
//! What is built is kept, per runtime handle, until the runtime shuts down.
//! It is not allocated from the runtime's allocator, because `reset` discards
//! everything that allocator holds and a built descriptor has to outlive that:
//! a descriptor is not a value, and an error that frees values does not free
//! the types they had. Kept out of the allocator, it is out of leak checking
//! too.
//!
//! The cache is keyed by the template's address and the parameters'
//! addresses. Those are static descriptors that live as long as the code that
//! emitted them, which outlives the runtime handle the code runs on, so no key
//! outlives what it names. A miss for a type something else already has a
//! descriptor for builds a second one, which is harmless: descriptors are
//! compared by structure.

use datalove_rtdt as rtdt;
use rmx::ahash::AHashMap;
use rtdt::{TyDesc, TyDescRef, TyInfo, TyTag};

/// The descriptors built on one runtime handle, and the memory behind them.
pub struct TyDescCache {
    /// Built descriptors by `key_hash` of their template and parameters.
    /// Collisions share a bucket and are told apart by comparing the key.
    entries: AHashMap<u64, Vec<Entry>>,
    /// Everything allocated here, freed when the cache is dropped.
    owned: Vec<Owned>,
}

struct Entry {
    template: *const TyDesc,
    params: Box<[*const TyDesc]>,
    built: *const TyDesc,
}

enum Owned {
    Desc(*mut TyDesc),
    TupleFields(*mut [rtdt::TyInfoTupleField]),
    StructFields(*mut [rtdt::TyInfoStructField]),
    EnumVariants(*mut [rtdt::TyInfoEnumVariant]),
    TableColumns(*mut [rtdt::TyInfoTableColumn]),
}

impl TyDescCache {
    pub fn new() -> TyDescCache {
        TyDescCache { entries: AHashMap::new(), owned: Vec::new() }
    }

    /// The descriptor `template` describes with its parameters bound to
    /// `params`, by position.
    ///
    /// A template with no parameter in it is its own instance, and comes back
    /// unchanged.
    ///
    /// # Safety
    ///
    /// `template` and every descriptor in `params` must be valid for as long
    /// as this cache, and every parameter the template names must be in
    /// `params`.
    pub unsafe fn instantiate(&mut self, template: *const TyDesc, params: &[*const TyDesc]) -> *const TyDesc {
        let hash = key_hash(template, params);
        if let Some(bucket) = self.entries.get(&hash) {
            for entry in bucket {
                if entry.template == template && *entry.params == *params {
                    return entry.built;
                }
            }
        }
        let built = unsafe { self.build(template, params) };
        self.entries.entry(hash).or_default().push(Entry {
            template,
            params: params.into(),
            built,
        });
        built
    }

    /// Build the instance of `template`, or return the template itself if
    /// nothing in it is a parameter.
    ///
    /// The parts of a template are built here too, not looked up: only what a
    /// generic asks for is cached. A part that is also asked for on its own is
    /// then built twice, which costs a descriptor and nothing else.
    unsafe fn build(&mut self, template: *const TyDesc, params: &[*const TyDesc]) -> *const TyDesc {
        let td = unsafe { &*template };
        let part = |cache: &mut TyDescCache, p: *const TyDesc| -> *const TyDesc {
            if p.is_null() { p } else { unsafe { cache.build(p, params) } }
        };
        unsafe {
            match td.type_tag {
                TyTag::Param => {
                    let index = td.type_info.param.index as usize;
                    let Some(&bound) = params.get(index) else {
                        panic!("a template names type parameter {index} of {}", params.len());
                    };
                    bound
                }

                TyTag::Bool | TyTag::U8 | TyTag::I8 | TyTag::U16 | TyTag::I16 | TyTag::U32
                | TyTag::I32 | TyTag::U64 | TyTag::I64 | TyTag::Index | TyTag::Offset
                | TyTag::F32 | TyTag::F64 | TyTag::Int | TyTag::String | TyTag::Data
                | TyTag::Error | TyTag::Atom => template,

                TyTag::List => {
                    let element = td.type_info.list.element_tydesc;
                    let built = part(self, element);
                    if built == element {
                        return template;
                    }
                    self.keep(header_sized::<rtdt::List>(td, TyInfo {
                        list: rtdt::TyInfoList { element_tydesc: built },
                    }))
                }

                TyTag::Set => {
                    let element = td.type_info.set.element_tydesc;
                    let built = part(self, element);
                    if built == element {
                        return template;
                    }
                    self.keep(header_sized::<rtdt::Set>(td, TyInfo {
                        set: rtdt::TyInfoSet::new(built),
                    }))
                }

                TyTag::Map => {
                    let info = td.type_info.map;
                    let key = part(self, info.key_tydesc);
                    let value = part(self, info.value_tydesc);
                    if key == info.key_tydesc && value == info.value_tydesc {
                        return template;
                    }
                    self.keep(header_sized::<rtdt::Map>(td, TyInfo {
                        map: rtdt::TyInfoMap::new(key, value),
                    }))
                }

                TyTag::Tensor => {
                    let info = td.type_info.tensor;
                    let element = part(self, info.element_tydesc);
                    if element == info.element_tydesc {
                        return template;
                    }
                    self.keep(header_sized::<rtdt::Tensor>(td, TyInfo {
                        tensor: rtdt::TyInfoTensor { element_tydesc: element, rank: info.rank },
                    }))
                }

                TyTag::Table => {
                    let info = td.type_info.table;
                    let columns = std::slice::from_raw_parts(info.columns, info.num_columns as usize);
                    let built: Vec<_> = columns.iter().map(|c| rtdt::TyInfoTableColumn {
                        tydesc: part(self, c.tydesc),
                        ..*c
                    }).collect();
                    if built.iter().zip(columns).all(|(b, c)| b.tydesc == c.tydesc) {
                        return template;
                    }
                    let columns = self.keep_slice(built, Owned::TableColumns);
                    self.keep(header_sized::<rtdt::Table>(td, TyInfo {
                        table: rtdt::TyInfoTable { num_columns: info.num_columns, columns },
                    }))
                }

                TyTag::Option => {
                    let inner = td.type_info.option.inner_tydesc;
                    let built = part(self, inner);
                    if built == inner {
                        return template;
                    }
                    let mut desc = blank(td, TyInfo { option: rtdt::TyInfoOption { inner_tydesc: built } });
                    let layout = rtdt::layout::compute_option_layout(TyDescRef::from_ref(&desc));
                    desc.size = layout.size;
                    desc.align = layout.align;
                    self.keep(desc)
                }

                TyTag::Result => {
                    let ok = td.type_info.result.ok_tydesc;
                    let built = part(self, ok);
                    if built == ok {
                        return template;
                    }
                    let mut desc = blank(td, TyInfo { result: rtdt::TyInfoResult { ok_tydesc: built } });
                    let layout = rtdt::layout::compute_result_layout(TyDescRef::from_ref(&desc));
                    desc.size = layout.size;
                    desc.align = layout.align;
                    self.keep(desc)
                }

                TyTag::Term => {
                    let info = td.type_info.term;
                    let payload = part(self, info.payload);
                    if payload == info.payload {
                        return template;
                    }
                    // A term adds nothing to its payload's layout.
                    let mut desc = blank(td, TyInfo { term: rtdt::TyInfoTerm { payload, ..info } });
                    desc.size = (*payload).size;
                    desc.align = (*payload).align;
                    self.keep(desc)
                }

                TyTag::Tuple => {
                    let info = td.type_info.tuple;
                    let fields = std::slice::from_raw_parts(info.fields, info.num_fields as usize);
                    let built: Vec<_> = fields.iter().map(|f| rtdt::TyInfoTupleField {
                        tydesc: part(self, f.tydesc),
                        ..*f
                    }).collect();
                    if built.iter().zip(fields).all(|(b, f)| b.tydesc == f.tydesc) {
                        return template;
                    }
                    let fields = self.keep_slice(built, Owned::TupleFields) as *mut rtdt::TyInfoTupleField;
                    let mut desc = blank(td, TyInfo {
                        tuple: rtdt::TyInfoTuple { num_fields: info.num_fields, fields },
                    });
                    let layout = rtdt::layout::compute_tuple_layout(TyDescRef::from_ref(&desc));
                    for (i, offset) in layout.field_offsets.iter().enumerate() {
                        (*fields.add(i)).offset = *offset;
                    }
                    desc.size = layout.size;
                    desc.align = layout.align;
                    self.keep(desc)
                }

                TyTag::Struct => {
                    let info = td.type_info.struct_;
                    let fields = std::slice::from_raw_parts(info.fields, info.num_fields as usize);
                    let built: Vec<_> = fields.iter().map(|f| rtdt::TyInfoStructField {
                        tydesc: part(self, f.tydesc),
                        ..*f
                    }).collect();
                    if built.iter().zip(fields).all(|(b, f)| b.tydesc == f.tydesc) {
                        return template;
                    }
                    let fields = self.keep_slice(built, Owned::StructFields) as *mut rtdt::TyInfoStructField;
                    let mut desc = blank(td, TyInfo {
                        struct_: rtdt::TyInfoStruct { fields, num_fields: info.num_fields },
                    });
                    let layout = rtdt::layout::compute_struct_layout(TyDescRef::from_ref(&desc));
                    for (i, offset) in layout.field_offsets.iter().enumerate() {
                        (*fields.add(i)).offset = *offset;
                    }
                    desc.size = layout.size;
                    desc.align = layout.align;
                    self.keep(desc)
                }

                TyTag::Enum => {
                    let info = td.type_info.enum_;
                    let variants = std::slice::from_raw_parts(info.variants, info.num_variants as usize);
                    let built: Vec<_> = variants.iter().map(|v| rtdt::TyInfoEnumVariant {
                        payload: part(self, v.payload),
                        ..*v
                    }).collect();
                    if built.iter().zip(variants).all(|(b, v)| b.payload == v.payload) {
                        return template;
                    }
                    let variants = self.keep_slice(built, Owned::EnumVariants) as *mut rtdt::TyInfoEnumVariant;
                    let mut desc = blank(td, TyInfo {
                        enum_: rtdt::TyInfoEnum { variants, num_variants: info.num_variants },
                    });
                    let layout = rtdt::layout::compute_enum_layout(TyDescRef::from_ref(&desc));
                    for (i, offset) in layout.variant_offsets.iter().enumerate() {
                        (*variants.add(i)).offset = *offset;
                    }
                    desc.size = layout.size;
                    desc.align = layout.align;
                    self.keep(desc)
                }
            }
        }
    }

    /// Keep a descriptor whose parts are already kept, filling in its flags.
    fn keep(&mut self, mut desc: TyDesc) -> *const TyDesc {
        desc.flags = desc.computed_flags();
        let ptr = Box::into_raw(Box::new(desc));
        self.owned.push(Owned::Desc(ptr));
        ptr
    }

    fn keep_slice<T>(&mut self, items: Vec<T>, owned: fn(*mut [T]) -> Owned) -> *mut T {
        let ptr = Box::into_raw(items.into_boxed_slice());
        self.owned.push(owned(ptr));
        ptr as *mut T
    }
}

impl Drop for TyDescCache {
    fn drop(&mut self) {
        for owned in self.owned.drain(..) {
            unsafe {
                match owned {
                    Owned::Desc(p) => drop(Box::from_raw(p)),
                    Owned::TupleFields(p) => drop(Box::from_raw(p)),
                    Owned::StructFields(p) => drop(Box::from_raw(p)),
                    Owned::EnumVariants(p) => drop(Box::from_raw(p)),
                    Owned::TableColumns(p) => drop(Box::from_raw(p)),
                }
            }
        }
    }
}

/// A descriptor with the template's tag and the given arm, its size and
/// alignment to be filled in.
fn blank(template: &TyDesc, type_info: TyInfo) -> TyDesc {
    TyDesc { type_tag: template.type_tag, flags: 0, size: 0, align: 0, type_info }
}

/// A descriptor for a container whose header is the same whatever it holds.
fn header_sized<H>(template: &TyDesc, type_info: TyInfo) -> TyDesc {
    TyDesc {
        size: size_of::<H>() as u32,
        align: align_of::<H>() as u32,
        ..blank(template, type_info)
    }
}

/// The hash of a cache key: a template and the parameters it is bound to.
///
/// FxHash's step over the addresses. The keys are a handful of pointers, so a
/// general-purpose hasher's setup costs more than the mixing does.
#[inline]
fn key_hash(template: *const TyDesc, params: &[*const TyDesc]) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let step = |hash: u64, word: u64| (hash.rotate_left(5) ^ word).wrapping_mul(K);
    params.iter().fold(step(0, template as u64), |hash, p| step(hash, *p as u64))
}
