//! Type descriptor table for runtime type information.
//!
//! This module provides `TyDescTable`, which manages the creation and
//! deduplication of runtime type descriptors (TyDescs). TyDescs are used
//! by the runtime to understand the layout and structure of values.

use rmx::prelude::*;
use std::collections::HashMap;
use bct::text::InternedText;
use crate::tycheck::*;
use crate::rtdt;

/// Table of type descriptors with deduplication.
///
/// TyDescs contain raw pointers to each other, so must outlive all value access.
pub struct TyDescTable<'db> {
    db: &'db dyn crate::Db,
    /// Deduplication map: Type → TyDesc pointer.
    cache: HashMap<Type<'db>, *const rtdt::TyDesc>,
    /// Storage for TyDesc allocations.
    tydescs: Vec<Box<rtdt::TyDesc>>,
    /// Storage for flexible array members.
    tuple_fields: Vec<Vec<rtdt::TyInfoTupleField>>,
    struct_fields: Vec<Vec<rtdt::TyInfoStructField>>,
    enum_variants: Vec<Vec<rtdt::TyInfoEnumVariant>>,
}

impl<'db> TyDescTable<'db> {
    pub fn new(db: &'db dyn crate::Db) -> Self {
        Self {
            db,
            cache: HashMap::new(),
            tydescs: Vec::new(),
            tuple_fields: Vec::new(),
            struct_fields: Vec::new(),
            enum_variants: Vec::new(),
        }
    }

    /// Get or create a TyDesc for the given type.
    pub fn get_or_create(&mut self, ty: &Type<'db>) -> *const rtdt::TyDesc {
        if let Some(&ptr) = self.cache.get(ty) {
            return ptr;
        }

        let tydesc = self.create_tydesc(ty);
        let ptr = &*tydesc as *const rtdt::TyDesc;
        self.tydescs.push(tydesc);
        self.cache.insert(ty.clone(), ptr);
        ptr
    }

    /// Create a new TyDesc for the given type.
    fn create_tydesc(&mut self, ty: &Type<'db>) -> Box<rtdt::TyDesc> {
        match ty {
            Type::Bool => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Bool,
                    size: 1,
                    align: 1,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::U32 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::U32,
                    size: 4,
                    align: 4,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::F32 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::F32,
                    size: 4,
                    align: 4,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::Int => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Int,
                    size: std::mem::size_of::<rtdt::Int>() as u32,
                    align: std::mem::align_of::<rtdt::Int>() as u32,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::String => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::String,
                    size: std::mem::size_of::<rtdt::String>() as u32,
                    align: std::mem::align_of::<rtdt::String>() as u32,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::Data => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Data,
                    size: std::mem::size_of::<rtdt::Data>() as u32,
                    align: std::mem::align_of::<rtdt::Data>() as u32,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::Error => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Error,
                    size: std::mem::size_of::<rtdt::Error>() as u32,
                    align: std::mem::align_of::<rtdt::Error>() as u32,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::AnonTuple(t) => self.create_tuple_tydesc(&t.fields(self.db)),
            Type::NamedTuple(t) => self.create_tuple_tydesc(&t.fields(self.db)),
            Type::AnonStruct(s) => self.create_struct_tydesc(&s.fields(self.db)),
            Type::NamedStruct(s) => self.create_struct_tydesc(&s.fields(self.db)),
            Type::AnonEnum(e) => self.create_enum_tydesc(&e.variants(self.db)),
            Type::NamedEnum(e) => self.create_enum_tydesc(&e.variants(self.db)),
            Type::List(l) => self.create_list_tydesc(l.element_type(self.db)),
            Type::Map(m) => self.create_map_tydesc(m.key_type(self.db), m.value_type(self.db)),
            Type::Set(s) => self.create_set_tydesc(s.element_type(self.db)),
            Type::Option(o) => self.create_option_tydesc(o.inner_type(self.db)),
            Type::Result(r) => self.create_result_tydesc(r.inner_type(self.db)),
        }
    }

    /// Create TyDesc for tuple (anon or named).
    fn create_tuple_tydesc(&mut self, fields: &[TypeAndHeap<'db>]) -> Box<rtdt::TyDesc> {
        // Recursively create TyDescs for field types.
        let mut field_tydescs = Vec::new();
        for field in fields {
            let field_ty = field.ty(self.db);
            let field_tydesc = self.get_or_create(field_ty);
            field_tydescs.push(field_tydesc);
        }

        // Create temporary field info array with placeholder offsets.
        let mut temp_field_info = Vec::new();
        for &field_tydesc in &field_tydescs {
            temp_field_info.push(rtdt::TyInfoTupleField {
                offset: 0,
                tydesc: field_tydesc,
            });
        }

        // Compute layout.
        let layout = unsafe {
            let temp_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::Tuple,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    tuple: rtdt::TyInfoTuple {
                        num_fields: fields.len() as u32,
                        fields: temp_field_info.as_ptr(),
                    },
                },
            };
            rtdt::layout::compute_tuple_layout(&temp_tydesc)
        };

        // Create final field info array with computed offsets.
        let mut field_info = Vec::new();
        for (i, &field_tydesc) in field_tydescs.iter().enumerate() {
            field_info.push(rtdt::TyInfoTupleField {
                offset: layout.field_offsets[i],
                tydesc: field_tydesc,
            });
        }

        // Store field array and get stable pointer.
        self.tuple_fields.push(field_info);
        let fields_ptr = self.tuple_fields.last().unwrap().as_ptr();

        Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Tuple,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                tuple: rtdt::TyInfoTuple {
                    num_fields: fields.len() as u32,
                    fields: fields_ptr,
                },
            },
        })
    }

    /// Create TyDesc for struct (anon or named).
    fn create_struct_tydesc(&mut self, fields: &[TypeNamedField<'db>]) -> Box<rtdt::TyDesc> {
        // Recursively create TyDescs for field types.
        let mut field_tydescs = Vec::new();
        for field in fields {
            let field_ty = field.ty(self.db);
            let field_tydesc = self.get_or_create(field_ty.ty(self.db));
            field_tydescs.push(field_tydesc);
        }

        // Create temporary field info array with placeholder offsets.
        let mut temp_field_info = Vec::new();
        for (i, field) in fields.iter().enumerate() {
            let field_name = field.name(self.db).as_str(self.db);
            temp_field_info.push(rtdt::TyInfoStructField {
                name: field_name.as_ptr(),
                name_len: field_name.len() as u32,
                offset: 0,
                tydesc: field_tydescs[i],
            });
        }

        // Compute layout using struct layout function.
        let layout = unsafe {
            let temp_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::Struct,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    struct_: rtdt::TyInfoStruct {
                        num_fields: fields.len() as u32,
                        fields: temp_field_info.as_ptr(),
                    },
                },
            };
            rtdt::layout::compute_struct_layout(&temp_tydesc)
        };

        // Create final field info array with computed offsets.
        let mut field_info = Vec::new();
        for (i, field) in fields.iter().enumerate() {
            let field_name = field.name(self.db).as_str(self.db);
            field_info.push(rtdt::TyInfoStructField {
                name: field_name.as_ptr(),
                name_len: field_name.len() as u32,
                offset: layout.field_offsets[i],
                tydesc: field_tydescs[i],
            });
        }

        // Store field array and get stable pointer.
        self.struct_fields.push(field_info);
        let fields_ptr = self.struct_fields.last().unwrap().as_ptr();

        Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Struct,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                struct_: rtdt::TyInfoStruct {
                    num_fields: fields.len() as u32,
                    fields: fields_ptr,
                },
            },
        })
    }

    /// Create TyDesc for enum (anon or named).
    fn create_enum_tydesc(&mut self, variants: &[TypeEnumVariant<'db>]) -> Box<rtdt::TyDesc> {
        // Create variant info array with TyDescs for payloads.
        let mut variant_info = Vec::new();
        for variant in variants {
            let variant_name = variant.name(self.db).as_str(self.db);
            let payload_tydesc = if let Some(payload_ty) = variant.payload(self.db) {
                self.get_or_create(payload_ty.ty(self.db))
            } else {
                std::ptr::null()
            };

            variant_info.push(rtdt::TyInfoEnumVariant {
                name: variant_name.as_ptr(),
                name_len: variant_name.len() as u32,
                offset: 0, // Will be computed by layout
                payload: payload_tydesc,
            });
        }

        // Compute layout.
        let layout = unsafe {
            let temp_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::Enum,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    enum_: rtdt::TyInfoEnum {
                        num_variants: variants.len() as u32,
                        variants: variant_info.as_ptr(),
                    },
                },
            };
            rtdt::layout::compute_enum_layout(&temp_tydesc)
        };

        // Update variant offsets from layout.
        for (i, variant_entry) in variant_info.iter_mut().enumerate() {
            variant_entry.offset = layout.variant_offsets[i];
        }

        // Store variant array and get stable pointer.
        self.enum_variants.push(variant_info);
        let variants_ptr = self.enum_variants.last().unwrap().as_ptr();

        Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Enum,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                enum_: rtdt::TyInfoEnum {
                    num_variants: variants.len() as u32,
                    variants: variants_ptr,
                },
            },
        })
    }

    /// Create TyDesc for list.
    fn create_list_tydesc(&mut self, element_type: TypeAndHeap<'db>) -> Box<rtdt::TyDesc> {
        // Recursively create TyDesc for element type.
        let element_tydesc = self.get_or_create(element_type.ty(self.db));

        Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::List,
            size: std::mem::size_of::<rtdt::List>() as u32,
            align: std::mem::align_of::<rtdt::List>() as u32,
            type_info: rtdt::TyInfo {
                list: rtdt::TyInfoList {
                    element_tydesc,
                },
            },
        })
    }

    /// Create TyDesc for option.
    fn create_option_tydesc(&mut self, inner_type: TypeAndHeap<'db>) -> Box<rtdt::TyDesc> {
        // Recursively create TyDesc for inner type.
        let inner_tydesc = self.get_or_create(inner_type.ty(self.db));

        // Create temporary TyDesc to compute layout.
        let temp_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Option,
            size: 0,
            align: 1,
            type_info: rtdt::TyInfo {
                option: rtdt::TyInfoOption { inner_tydesc },
            },
        };

        // Compute layout.
        let layout = unsafe { rtdt::layout::compute_option_layout(&temp_tydesc) };

        // Create final TyDesc with computed layout.
        Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Option,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                option: rtdt::TyInfoOption { inner_tydesc },
            },
        })
    }

    /// Create TyDesc for map.
    fn create_map_tydesc(
        &mut self,
        key_type: TypeAndHeap<'db>,
        value_type: TypeAndHeap<'db>,
    ) -> Box<rtdt::TyDesc> {
        // Recursively create TyDescs for key and value types.
        let key_tydesc = self.get_or_create(key_type.ty(self.db));
        let value_tydesc = self.get_or_create(value_type.ty(self.db));

        Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Map,
            size: std::mem::size_of::<rtdt::Map>() as u32,
            align: std::mem::align_of::<rtdt::Map>() as u32,
            type_info: rtdt::TyInfo {
                map: rtdt::TyInfoMap {
                    key_tydesc,
                    value_tydesc,
                },
            },
        })
    }

    /// Create TyDesc for set.
    fn create_set_tydesc(&mut self, element_type: TypeAndHeap<'db>) -> Box<rtdt::TyDesc> {
        // Recursively create TyDesc for element type.
        let element_tydesc = self.get_or_create(element_type.ty(self.db));

        Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Set,
            size: std::mem::size_of::<rtdt::Set>() as u32,
            align: std::mem::align_of::<rtdt::Set>() as u32,
            type_info: rtdt::TyInfo {
                set: rtdt::TyInfoSet {
                    element_tydesc,
                },
            },
        })
    }

    /// Create TyDesc for result.
    fn create_result_tydesc(&mut self, inner_type: TypeAndHeap<'db>) -> Box<rtdt::TyDesc> {
        // Recursively create TyDesc for inner type.
        let inner_tydesc = self.get_or_create(inner_type.ty(self.db));

        // Create temporary TyDesc to compute layout.
        let temp_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Result,
            size: 0,
            align: 1,
            type_info: rtdt::TyInfo {
                result: rtdt::TyInfoResult {
                    ok_tydesc: inner_tydesc,
                },
            },
        };

        // Compute layout.
        let layout = unsafe { rtdt::layout::compute_result_layout(&temp_tydesc) };

        // Create final TyDesc with computed layout.
        Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Result,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                result: rtdt::TyInfoResult {
                    ok_tydesc: inner_tydesc,
                },
            },
        })
    }
}
