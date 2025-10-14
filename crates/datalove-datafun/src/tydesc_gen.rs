//! Type descriptor generation for the interpreter.
//!
//! Converts salsa Type<'db> to rtdt TyDesc* for use in the interpreter.

use rmx::prelude::*;
use std::collections::HashMap;
use datalove_rtdt as rtdt;
use crate::datalit;
use crate::tycheck::{Type, TypeAndHeap};

/// Cache for generated type descriptors.
///
/// Manages allocation and deduplication of TyDesc instances.
pub struct TyDescCache {
    /// Map from Type hash to allocated TyDesc pointer.
    cache: HashMap<u64, *const rtdt::TyDesc>,

    /// All allocated type descriptors (for cleanup).
    allocations: Vec<Box<rtdt::TyDesc>>,

    /// Field arrays allocated for compound types.
    field_arrays: Vec<Box<[rtdt::TyInfoTupleField]>>,
    struct_field_arrays: Vec<Box<[rtdt::TyInfoStructField]>>,
    enum_variant_arrays: Vec<Box<[rtdt::TyInfoEnumVariant]>>,

    /// Field name storage.
    field_names: Vec<Box<[u8]>>,
}

impl TyDescCache {
    /// Create a new type descriptor cache.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            allocations: Vec::new(),
            field_arrays: Vec::new(),
            struct_field_arrays: Vec::new(),
            enum_variant_arrays: Vec::new(),
            field_names: Vec::new(),
        }
    }

    /// Generate a TyDesc for the given type.
    ///
    /// Returns a stable pointer that remains valid for the lifetime of this cache.
    pub fn generate<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        ty: &Type<'db>,
    ) -> *const rtdt::TyDesc {
        // Use hash for cache lookup.
        let hash = self.hash_type(db, ty);

        if let Some(&cached) = self.cache.get(&hash) {
            return cached;
        }

        // Generate the type descriptor.
        let tydesc = self.generate_uncached(db, ty);

        // Allocate and cache it.
        let boxed = Box::new(tydesc);
        let ptr = Box::as_ref(&boxed) as *const rtdt::TyDesc;
        self.allocations.push(boxed);
        self.cache.insert(hash, ptr);

        ptr
    }

    /// Generate type descriptor without caching.
    fn generate_uncached<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        ty: &Type<'db>,
    ) -> rtdt::TyDesc {
        match ty {
            Type::Datalit(datalit_ty) => self.generate_datalit(db, datalit_ty),
            Type::Function(_) => {
                // Functions cannot be represented in rtdt yet.
                // For now, treat as opaque data.
                rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Data,
                    size: std::mem::size_of::<usize>() as u32,
                    align: std::mem::align_of::<usize>() as u32,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                }
            }
        }
    }

    /// Generate type descriptor for datalit type.
    fn generate_datalit<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        ty: &datalit::tycheck::Type<'db>,
    ) -> rtdt::TyDesc {
        use datalit::tycheck::Type as DT;

        match ty {
            DT::Bool => rtdt::TyDesc {
                type_tag: rtdt::TyTag::Bool,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo {
                    nothing: rtdt::TyInfoNothing,
                },
            },

            DT::U32 => rtdt::TyDesc {
                type_tag: rtdt::TyTag::U32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo {
                    nothing: rtdt::TyInfoNothing,
                },
            },

            DT::F32 => rtdt::TyDesc {
                type_tag: rtdt::TyTag::F32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo {
                    nothing: rtdt::TyInfoNothing,
                },
            },

            DT::Int => rtdt::TyDesc {
                type_tag: rtdt::TyTag::Int,
                size: std::mem::size_of::<rtdt::Int>() as u32,
                align: std::mem::align_of::<rtdt::Int>() as u32,
                type_info: rtdt::TyInfo {
                    nothing: rtdt::TyInfoNothing,
                },
            },

            DT::String => rtdt::TyDesc {
                type_tag: rtdt::TyTag::String,
                size: std::mem::size_of::<rtdt::String>() as u32,
                align: std::mem::align_of::<rtdt::String>() as u32,
                type_info: rtdt::TyInfo {
                    nothing: rtdt::TyInfoNothing,
                },
            },

            DT::AnonTuple(tuple) => {
                let fields = tuple.fields(db);
                self.generate_tuple_tydesc(db, &fields)
            }

            DT::NamedTuple(tuple) => {
                let fields = tuple.fields(db);
                self.generate_tuple_tydesc(db, &fields)
            }

            DT::AnonStruct(struct_ty) => {
                let fields = struct_ty.fields(db);
                self.generate_struct_tydesc(db, &fields)
            }

            DT::NamedStruct(struct_ty) => {
                let fields = struct_ty.fields(db);
                self.generate_struct_tydesc(db, &fields)
            }

            DT::AnonEnum(enum_ty) => {
                let variants = enum_ty.variants(db);
                self.generate_enum_tydesc(db, &variants)
            }

            DT::NamedEnum(enum_ty) => {
                let variants = enum_ty.variants(db);
                self.generate_enum_tydesc(db, &variants)
            }

            DT::List(list) => {
                let element_ty = list.element_type(db);
                let element_tydesc = self.generate_typeandheap(db, &element_ty);

                let list_info = rtdt::TyInfoList {
                    element_tydesc,
                };

                rtdt::TyDesc {
                    type_tag: rtdt::TyTag::List,
                    size: std::mem::size_of::<rtdt::List>() as u32,
                    align: std::mem::align_of::<rtdt::List>() as u32,
                    type_info: rtdt::TyInfo { list: list_info },
                }
            }

            DT::Map(map) => {
                let key_ty = map.key_type(db);
                let value_ty = map.value_type(db);
                let key_tydesc = self.generate_typeandheap(db, &key_ty);
                let value_tydesc = self.generate_typeandheap(db, &value_ty);

                let map_info = rtdt::TyInfoMap {
                    key_tydesc,
                    value_tydesc,
                };

                rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Map,
                    size: std::mem::size_of::<rtdt::Map>() as u32,
                    align: std::mem::align_of::<rtdt::Map>() as u32,
                    type_info: rtdt::TyInfo { map: map_info },
                }
            }

            DT::Set(set) => {
                let element_ty = set.element_type(db);
                let element_tydesc = self.generate_typeandheap(db, &element_ty);

                let set_info = rtdt::TyInfoSet {
                    element_tydesc,
                };

                rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Set,
                    size: std::mem::size_of::<rtdt::Set>() as u32,
                    align: std::mem::align_of::<rtdt::Set>() as u32,
                    type_info: rtdt::TyInfo { set: set_info },
                }
            }

            DT::Option(opt) => {
                let inner_ty = opt.inner_type(db);
                let inner_tydesc = self.generate_typeandheap(db, &inner_ty);

                let opt_info = rtdt::TyInfoOption {
                    inner_tydesc,
                };

                // Compute layout using rtdt::layout.
                let layout = unsafe { rtdt::layout::compute_option_layout(inner_tydesc) };

                rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Option,
                    size: layout.size,
                    align: layout.align,
                    type_info: rtdt::TyInfo { option: opt_info },
                }
            }

            DT::Result(res) => {
                let ok_ty = res.inner_type(db);
                let ok_tydesc = self.generate_typeandheap(db, &ok_ty);

                let res_info = rtdt::TyInfoResult {
                    ok_tydesc,
                };

                // Compute layout using rtdt::layout.
                let layout = unsafe { rtdt::layout::compute_result_layout(ok_tydesc) };

                rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Result,
                    size: layout.size,
                    align: layout.align,
                    type_info: rtdt::TyInfo { result: res_info },
                }
            }

            DT::Data => rtdt::TyDesc {
                type_tag: rtdt::TyTag::Data,
                size: std::mem::size_of::<rtdt::Data>() as u32,
                align: std::mem::align_of::<rtdt::Data>() as u32,
                type_info: rtdt::TyInfo {
                    nothing: rtdt::TyInfoNothing,
                },
            },

            DT::Error => rtdt::TyDesc {
                type_tag: rtdt::TyTag::Error,
                size: std::mem::size_of::<rtdt::Error>() as u32,
                align: std::mem::align_of::<rtdt::Error>() as u32,
                type_info: rtdt::TyInfo {
                    nothing: rtdt::TyInfoNothing,
                },
            },
        }
    }

    /// Generate TyDesc for TypeAndHeap.
    fn generate_typeandheap<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        ty: &datalit::tycheck::TypeAndHeap<'db>,
    ) -> *const rtdt::TyDesc {
        let tydesc = self.generate_datalit(db, ty.ty(db));
        let boxed = Box::new(tydesc);
        let ptr = Box::as_ref(&boxed) as *const rtdt::TyDesc;
        self.allocations.push(boxed);
        ptr
    }

    /// Generate tuple type descriptor.
    fn generate_tuple_tydesc<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        fields: &[datalit::tycheck::TypeAndHeap<'db>],
    ) -> rtdt::TyDesc {
        // Generate field descriptors.
        let mut field_tydescs = Vec::with_capacity(fields.len());
        for field in fields {
            let tydesc = self.generate_typeandheap(db, field);
            field_tydescs.push(tydesc);
        }

        // Build TyInfoTupleField array.
        let mut tuple_fields = Vec::with_capacity(fields.len());
        let mut offset = 0u32;
        let mut max_align = 1u32;

        for tydesc in &field_tydescs {
            let field_size = unsafe { (**tydesc).size };
            let field_align = unsafe { (**tydesc).align };

            // Align offset.
            offset = rtdt::layout::align_up(offset, field_align);

            tuple_fields.push(rtdt::TyInfoTupleField {
                offset,
                tydesc: *tydesc,
            });

            offset += field_size;
            max_align = max_align.max(field_align);
        }

        // Total size aligned to max alignment.
        let total_size = rtdt::layout::align_up(offset, max_align);

        // Allocate field array.
        let fields_boxed = tuple_fields.into_boxed_slice();
        let fields_ptr = fields_boxed.as_ptr();
        self.field_arrays.push(fields_boxed);

        let tuple_info = rtdt::TyInfoTuple {
            num_fields: field_tydescs.len() as u32,
            fields: fields_ptr,
        };

        rtdt::TyDesc {
            type_tag: rtdt::TyTag::Tuple,
            size: total_size,
            align: max_align,
            type_info: rtdt::TyInfo { tuple: tuple_info },
        }
    }

    /// Generate struct type descriptor.
    fn generate_struct_tydesc<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        fields: &[datalit::tycheck::TypeNamedField<'db>],
    ) -> rtdt::TyDesc {
        // Generate field descriptors.
        let mut struct_fields = Vec::with_capacity(fields.len());
        let mut offset = 0u32;
        let mut max_align = 1u32;

        for field in fields {
            let field_name = field.name(db);
            let field_ty = field.ty(db);
            let field_tydesc = self.generate_typeandheap(db, &field_ty);

            let field_size = unsafe { (*field_tydesc).size };
            let field_align = unsafe { (*field_tydesc).align };

            // Align offset.
            offset = rtdt::layout::align_up(offset, field_align);

            // Store field name as bytes.
            let name_str = field_name.as_str(db);
            let name_bytes = name_str.as_bytes().to_vec().into_boxed_slice();
            let name_ptr = name_bytes.as_ptr();
            let name_len = name_bytes.len() as u32;
            self.field_names.push(name_bytes);

            struct_fields.push(rtdt::TyInfoStructField {
                name: name_ptr,
                name_len,
                offset,
                tydesc: field_tydesc,
            });

            offset += field_size;
            max_align = max_align.max(field_align);
        }

        // Total size aligned to max alignment.
        let total_size = rtdt::layout::align_up(offset, max_align);

        // Allocate field array.
        let fields_boxed = struct_fields.into_boxed_slice();
        let fields_ptr = fields_boxed.as_ptr();
        self.struct_field_arrays.push(fields_boxed);

        let struct_info = rtdt::TyInfoStruct {
            fields: fields_ptr,
            num_fields: fields.len() as u32,
        };

        rtdt::TyDesc {
            type_tag: rtdt::TyTag::Struct,
            size: total_size,
            align: max_align,
            type_info: rtdt::TyInfo { struct_: struct_info },
        }
    }

    /// Generate enum type descriptor.
    fn generate_enum_tydesc<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        variants: &[datalit::tycheck::TypeEnumVariant<'db>],
    ) -> rtdt::TyDesc {
        // Generate variant descriptors.
        let mut enum_variants = Vec::with_capacity(variants.len());
        let mut max_payload_size = 0u32;
        let mut max_payload_align = 4u32; // Discriminant is u32.

        for variant in variants {
            let variant_name = variant.name(db);
            let payload = variant.payload(db);

            // Store variant name as bytes.
            let name_str = variant_name.as_str(db);
            let name_bytes = name_str.as_bytes().to_vec().into_boxed_slice();
            let name_ptr = name_bytes.as_ptr();
            let name_len = name_bytes.len() as u32;
            self.field_names.push(name_bytes);

            let payload_tydesc = if let Some(payload_ty) = payload {
                let tydesc = self.generate_typeandheap(db, &payload_ty);
                let payload_size = unsafe { (*tydesc).size };
                let payload_align = unsafe { (*tydesc).align };

                max_payload_size = max_payload_size.max(payload_size);
                max_payload_align = max_payload_align.max(payload_align);

                tydesc
            } else {
                std::ptr::null()
            };

            let payload_offset = if payload_tydesc.is_null() {
                0
            } else {
                let payload_align = unsafe { (*payload_tydesc).align };
                rtdt::layout::align_up(4, payload_align)
            };

            enum_variants.push(rtdt::TyInfoEnumVariant {
                name: name_ptr,
                name_len,
                offset: payload_offset,
                payload: payload_tydesc,
            });
        }

        // Compute total size: discriminant + padding + max payload.
        let payload_offset = rtdt::layout::align_up(4, max_payload_align);
        let total_size = rtdt::layout::align_up(payload_offset + max_payload_size, max_payload_align);

        // Allocate variant array.
        let variants_boxed = enum_variants.into_boxed_slice();
        let variants_ptr = variants_boxed.as_ptr();
        self.enum_variant_arrays.push(variants_boxed);

        let enum_info = rtdt::TyInfoEnum {
            variants: variants_ptr,
            num_variants: variants.len() as u32,
        };

        rtdt::TyDesc {
            type_tag: rtdt::TyTag::Enum,
            size: total_size,
            align: max_payload_align,
            type_info: rtdt::TyInfo { enum_: enum_info },
        }
    }

    /// Simple hash function for types (for caching).
    fn hash_type<'db>(&self, db: &'db dyn crate::Db, ty: &Type<'db>) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Hash the type discriminant and structure.
        self.hash_type_impl(db, ty, &mut hasher);

        hasher.finish()
    }

    fn hash_type_impl<'db>(&self, db: &'db dyn crate::Db, ty: &Type<'db>, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;

        match ty {
            Type::Datalit(dt) => {
                0u8.hash(hasher);
                self.hash_datalit_type(db, dt, hasher);
            }
            Type::Function(f) => {
                1u8.hash(hasher);
                f.param_types(db).len().hash(hasher);
                // For simplicity, just hash param and return type count.
            }
        }
    }

    fn hash_datalit_type<'db>(&self, db: &'db dyn crate::Db, ty: &datalit::tycheck::Type<'db>, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        use datalit::tycheck::Type as DT;

        match ty {
            DT::Bool => 0u8.hash(hasher),
            DT::U32 => 1u8.hash(hasher),
            DT::F32 => 2u8.hash(hasher),
            DT::Int => 3u8.hash(hasher),
            DT::String => 4u8.hash(hasher),
            DT::Data => 5u8.hash(hasher),
            DT::Error => 6u8.hash(hasher),
            DT::AnonTuple(t) => {
                7u8.hash(hasher);
                t.fields(db).len().hash(hasher);
            }
            DT::NamedTuple(t) => {
                8u8.hash(hasher);
                t.name(db).as_str(db).hash(hasher);
            }
            DT::AnonStruct(s) => {
                9u8.hash(hasher);
                s.fields(db).len().hash(hasher);
            }
            DT::NamedStruct(s) => {
                10u8.hash(hasher);
                s.name(db).as_str(db).hash(hasher);
            }
            DT::AnonEnum(e) => {
                11u8.hash(hasher);
                e.variants(db).len().hash(hasher);
            }
            DT::NamedEnum(e) => {
                12u8.hash(hasher);
                e.name(db).as_str(db).hash(hasher);
            }
            DT::List(_) => 13u8.hash(hasher),
            DT::Map(_) => 14u8.hash(hasher),
            DT::Set(_) => 15u8.hash(hasher),
            DT::Option(_) => 16u8.hash(hasher),
            DT::Result(_) => 17u8.hash(hasher),
        }
    }
}

impl Default for TyDescCache {
    fn default() -> Self {
        Self::new()
    }
}
