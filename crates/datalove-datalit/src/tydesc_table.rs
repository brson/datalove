//! Type descriptor table for runtime type information.
//!
//! This module provides `TyDescTable`, which manages the creation and
//! deduplication of runtime type descriptors (TyDescs). TyDescs are used
//! by the runtime to understand the layout and structure of values.

use rmx::prelude::*;
use std::collections::HashMap;
use crate::tycheck::*;
use datalove_rtdt as rtdt;

/// Table of type descriptors with deduplication.
///
/// TyDescs contain raw pointers to each other, so must outlive all value access.
pub struct TyDescTable<'db> {
    db: &'db dyn crate::Db,
    /// Deduplication map: Type → TyDesc pointer.
    cache: HashMap<Type<'db>, *const rtdt::TyDesc>,
    /// Runtime-constructed Option cache: inner_tydesc → Option<T> tydesc.
    runtime_option_cache: HashMap<*const rtdt::TyDesc, *const rtdt::TyDesc>,
    /// Runtime-constructed Result cache: inner_tydesc → Result<T> tydesc.
    runtime_result_cache: HashMap<*const rtdt::TyDesc, *const rtdt::TyDesc>,
    /// Runtime-constructed List cache: element_tydesc → List<T> tydesc.
    runtime_list_cache: HashMap<*const rtdt::TyDesc, *const rtdt::TyDesc>,
    /// Runtime-constructed Map cache: (key_tydesc, value_tydesc) → Map<K, V> tydesc.
    runtime_map_cache: HashMap<(*const rtdt::TyDesc, *const rtdt::TyDesc), *const rtdt::TyDesc>,
    /// Runtime-constructed Set cache: element_tydesc → Set<T> tydesc.
    runtime_set_cache: HashMap<*const rtdt::TyDesc, *const rtdt::TyDesc>,
    /// Storage for TyDesc allocations.
    tydescs: Vec<Box<rtdt::TyDesc>>,
    /// Storage for flexible array members.
    tuple_fields: Vec<Vec<rtdt::TyInfoTupleField>>,
    struct_fields: Vec<Vec<rtdt::TyInfoStructField>>,
    enum_variants: Vec<Vec<rtdt::TyInfoEnumVariant>>,
    column_tydescs: Vec<Vec<*const rtdt::TyDesc>>,
}

impl<'db> TyDescTable<'db> {
    pub fn new(db: &'db dyn crate::Db) -> Self {
        Self {
            db,
            cache: HashMap::new(),
            runtime_option_cache: HashMap::new(),
            runtime_result_cache: HashMap::new(),
            runtime_list_cache: HashMap::new(),
            runtime_map_cache: HashMap::new(),
            runtime_set_cache: HashMap::new(),
            tydescs: Vec::new(),
            tuple_fields: Vec::new(),
            struct_fields: Vec::new(),
            enum_variants: Vec::new(),
            column_tydescs: Vec::new(),
        }
    }

    /// Get or create a TyDesc for the given type.
    pub fn get_or_create(&mut self, ty: &Type<'db>) -> *const rtdt::TyDesc {
        if let Some(&ptr) = self.cache.get(ty) {
            return ptr;
        }

        let tydesc = self.create_tydesc(ty);
        self.tydescs.push(tydesc);
        let ptr = &**self.tydescs.last().unwrap() as *const rtdt::TyDesc;
        self.cache.insert(ty.clone(), ptr);
        ptr
    }

    /// Get or create a TyDesc for the given type, returning a safe TyDescRef.
    pub fn get_or_create_ref(&mut self, ty: &Type<'db>) -> rtdt::TyDescRef<'_> {
        let ptr = self.get_or_create(ty);
        unsafe { rtdt::TyDescRef::from_ptr(ptr) }
    }

    /// Create a tuple type descriptor from raw element type descriptors.
    ///
    /// This is used when building tuples from datafun expressions where we
    /// already have the runtime type descriptors for each element.
    pub fn get_or_create_tuple(&mut self, element_tydescs: &[*const rtdt::TyDesc]) -> *const rtdt::TyDesc {
        // Create temporary field info array with placeholder offsets.
        let mut temp_field_info = Vec::new();
        for &elem_tydesc in element_tydescs {
            temp_field_info.push(rtdt::TyInfoTupleField {
                offset: 0,
                tydesc: elem_tydesc,
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
                        num_fields: element_tydescs.len() as u32,
                        fields: temp_field_info.as_ptr(),
                    },
                },
            };
            rtdt::layout::compute_tuple_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc))
        };

        // Create final field info array with computed offsets.
        let mut field_info = Vec::new();
        for (i, &elem_tydesc) in element_tydescs.iter().enumerate() {
            field_info.push(rtdt::TyInfoTupleField {
                offset: layout.field_offsets[i],
                tydesc: elem_tydesc,
            });
        }

        // Store field array and get stable pointer.
        self.tuple_fields.push(field_info);
        let fields_ptr = self.tuple_fields.last().unwrap().as_ptr();

        let tydesc = Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Tuple,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                tuple: rtdt::TyInfoTuple {
                    num_fields: element_tydescs.len() as u32,
                    fields: fields_ptr,
                },
            },
        });

        self.tydescs.push(tydesc);
        let ptr = &**self.tydescs.last().unwrap() as *const rtdt::TyDesc;
        ptr
    }

    /// Create a struct type descriptor from field names and type descriptors.
    ///
    /// Fields must be provided in sorted order by name for canonical representation.
    /// This is used when building structs from datafun expressions where we
    /// already have the runtime type descriptors for each field.
    pub fn get_or_create_struct(
        &mut self,
        field_names_and_tydescs: &[(bct::text::InternedText<'db>, *const rtdt::TyDesc)],
    ) -> *const rtdt::TyDesc {
        // Create temporary field info array with placeholder offsets.
        let mut temp_field_info = Vec::new();
        for (name, tydesc) in field_names_and_tydescs {
            let name_str = name.as_str(self.db);
            temp_field_info.push(rtdt::TyInfoStructField {
                name: name_str.as_ptr(),
                name_len: name_str.len() as u32,
                offset: 0,
                tydesc: *tydesc,
            });
        }

        // Compute layout.
        let layout = unsafe {
            let temp_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::Struct,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    struct_: rtdt::TyInfoStruct {
                        num_fields: field_names_and_tydescs.len() as u32,
                        fields: temp_field_info.as_ptr(),
                    },
                },
            };
            rtdt::layout::compute_struct_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc))
        };

        // Create final field info array with computed offsets.
        let mut field_info = Vec::new();
        for (i, (name, tydesc)) in field_names_and_tydescs.iter().enumerate() {
            let name_str = name.as_str(self.db);
            field_info.push(rtdt::TyInfoStructField {
                name: name_str.as_ptr(),
                name_len: name_str.len() as u32,
                offset: layout.field_offsets[i],
                tydesc: *tydesc,
            });
        }

        // Store field array and get stable pointer.
        self.struct_fields.push(field_info);
        let fields_ptr = self.struct_fields.last().unwrap().as_ptr();

        let tydesc = Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Struct,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                struct_: rtdt::TyInfoStruct {
                    num_fields: field_names_and_tydescs.len() as u32,
                    fields: fields_ptr,
                },
            },
        });

        self.tydescs.push(tydesc);
        let ptr = &**self.tydescs.last().unwrap() as *const rtdt::TyDesc;
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
            Type::U8 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::U8,
                    size: 1,
                    align: 1,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::I8 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::I8,
                    size: 1,
                    align: 1,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::U16 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::U16,
                    size: 2,
                    align: 2,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::I16 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::I16,
                    size: 2,
                    align: 2,
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
            Type::I32 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::I32,
                    size: 4,
                    align: 4,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::U64 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::U64,
                    size: 8,
                    align: 8,
                    type_info: rtdt::TyInfo {
                        nothing: rtdt::TyInfoNothing,
                    },
                })
            }
            Type::I64 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::I64,
                    size: 8,
                    align: 8,
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
            Type::F64 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::F64,
                    size: 8,
                    align: 8,
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
            Type::AnonTuple(t) => self.create_tuple_tydesc(&t.fields),
            Type::AnonStruct(s) => self.create_struct_tydesc(&s.fields),
            Type::AnonEnum(e) => self.create_enum_tydesc(&e.variants),
            Type::List(l) => self.create_list_tydesc(l.element_type),
            Type::Map(m) => self.create_map_tydesc(m.key_type, m.value_type),
            Type::Set(s) => self.create_set_tydesc(s.element_type),
            Type::Option(o) => self.create_option_tydesc(o.inner_type),
            Type::Result(r) => self.create_result_tydesc(r.inner_type),
            Type::Tensor(t) => self.create_tensor_tydesc(t.element_type, t.rank),
            Type::Table(t) => self.create_table_tydesc(&t.columns),
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
            rtdt::layout::compute_tuple_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc))
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
            let field_ty = field.ty;
            let field_tydesc = self.get_or_create(field_ty.ty(self.db));
            field_tydescs.push(field_tydesc);
        }

        // Create temporary field info array with placeholder offsets.
        let mut temp_field_info = Vec::new();
        for (i, field) in fields.iter().enumerate() {
            let field_name = field.name.as_str(self.db);
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
            rtdt::layout::compute_struct_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc))
        };

        // Create final field info array with computed offsets.
        let mut field_info = Vec::new();
        for (i, field) in fields.iter().enumerate() {
            let field_name = field.name.as_str(self.db);
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
            let variant_name = variant.name.as_str(self.db);
            let payload_tydesc = if let Some(payload_ty) = variant.payload {
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
            rtdt::layout::compute_enum_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc))
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

    fn create_tensor_tydesc(&mut self, element_type: TypeAndHeap<'db>, rank: u32) -> Box<rtdt::TyDesc> {
        // Recursively create TyDesc for element type.
        let element_tydesc = self.get_or_create(element_type.ty(self.db));

        Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Tensor,
            size: std::mem::size_of::<rtdt::Tensor>() as u32,
            align: std::mem::align_of::<rtdt::Tensor>() as u32,
            type_info: rtdt::TyInfo {
                tensor: rtdt::TyInfoTensor {
                    element_tydesc,
                    rank,
                },
            },
        })
    }

    /// Create TyDesc for Option<T> from an existing inner tydesc pointer.
    ///
    /// This is useful for runtime construction of Option types without going through salsa.
    pub fn create_option_from_inner_tydesc(&mut self, inner_tydesc: *const rtdt::TyDesc) -> *const rtdt::TyDesc {
        // Check cache first.
        if let Some(&cached) = self.runtime_option_cache.get(&inner_tydesc) {
            return cached;
        }

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
        let layout = unsafe { rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc)) };

        // Create final TyDesc with computed layout.
        let tydesc = Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Option,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                option: rtdt::TyInfoOption { inner_tydesc },
            },
        });

        self.tydescs.push(tydesc);
        let ptr = &**self.tydescs.last().unwrap() as *const rtdt::TyDesc;
        self.runtime_option_cache.insert(inner_tydesc, ptr);
        ptr
    }

    /// Create TyDesc for Option<T> from an existing inner tydesc, returning a safe TyDescRef.
    pub fn create_option_from_inner_tydesc_ref(&mut self, inner_tydesc_ref: rtdt::TyDescRef<'_>) -> rtdt::TyDescRef<'_> {
        let inner_ptr = inner_tydesc_ref.as_ptr();
        let result_ptr = self.create_option_from_inner_tydesc(inner_ptr);
        unsafe { rtdt::TyDescRef::from_ptr(result_ptr) }
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
        let layout = unsafe { rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc)) };

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

    /// Create TyDesc for table.
    fn create_table_tydesc(&mut self, columns: &[TypeNamedField<'db>]) -> Box<rtdt::TyDesc> {
        // Recursively create TyDescs for each column type.
        let mut col_tydescs = Vec::new();
        for column in columns {
            let column_tydesc = self.get_or_create(column.ty.ty(self.db));
            col_tydescs.push(column_tydesc);
        }

        // Store column tydescs array and get stable pointer.
        self.column_tydescs.push(col_tydescs);
        let column_tydescs_ptr = self.column_tydescs.last().unwrap().as_ptr();

        Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Table,
            size: std::mem::size_of::<rtdt::Table>() as u32,
            align: std::mem::align_of::<rtdt::Table>() as u32,
            type_info: rtdt::TyInfo {
                table: rtdt::TyInfoTable {
                    num_columns: columns.len() as u32,
                    column_tydescs: column_tydescs_ptr,
                },
            },
        })
    }

    /// Create TyDesc for Result<T> from an existing inner tydesc pointer.
    ///
    /// This is useful for runtime construction of Result types without going through salsa.
    pub fn create_result_from_inner_tydesc(&mut self, inner_tydesc: *const rtdt::TyDesc) -> *const rtdt::TyDesc {
        // Check cache first.
        if let Some(&cached) = self.runtime_result_cache.get(&inner_tydesc) {
            return cached;
        }

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
        let layout = unsafe { rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc)) };

        // Create final TyDesc with computed layout.
        let tydesc = Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Result,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                result: rtdt::TyInfoResult {
                    ok_tydesc: inner_tydesc,
                },
            },
        });

        self.tydescs.push(tydesc);
        let ptr = &**self.tydescs.last().unwrap() as *const rtdt::TyDesc;
        self.runtime_result_cache.insert(inner_tydesc, ptr);
        ptr
    }

    /// Create TyDesc for Result<T> from an existing inner tydesc, returning a safe TyDescRef.
    pub fn create_result_from_inner_tydesc_ref(&mut self, inner_tydesc_ref: rtdt::TyDescRef<'_>) -> rtdt::TyDescRef<'_> {
        let inner_ptr = inner_tydesc_ref.as_ptr();
        let result_ptr = self.create_result_from_inner_tydesc(inner_ptr);
        unsafe { rtdt::TyDescRef::from_ptr(result_ptr) }
    }

    /// Create TyDesc for List<T> from an existing element tydesc.
    ///
    /// Used when building lists from datafun expressions where we already have
    /// the runtime type descriptor for the element type.
    pub fn create_list_from_element_tydesc(&mut self, element_tydesc: *const rtdt::TyDesc) -> *const rtdt::TyDesc {
        // Check cache first.
        if let Some(&cached) = self.runtime_list_cache.get(&element_tydesc) {
            return cached;
        }

        let tydesc = Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::List,
            size: std::mem::size_of::<rtdt::List>() as u32,
            align: std::mem::align_of::<rtdt::List>() as u32,
            type_info: rtdt::TyInfo {
                list: rtdt::TyInfoList { element_tydesc },
            },
        });

        self.tydescs.push(tydesc);
        let ptr = &**self.tydescs.last().unwrap() as *const rtdt::TyDesc;
        self.runtime_list_cache.insert(element_tydesc, ptr);
        ptr
    }

    /// Create TyDesc for Map<K, V> from existing key and value tydescs.
    ///
    /// Used when building maps from datafun expressions where we already have
    /// the runtime type descriptors for the key and value types.
    pub fn create_map_from_key_value_tydescs(
        &mut self,
        key_tydesc: *const rtdt::TyDesc,
        value_tydesc: *const rtdt::TyDesc,
    ) -> *const rtdt::TyDesc {
        let cache_key = (key_tydesc, value_tydesc);
        if let Some(&cached) = self.runtime_map_cache.get(&cache_key) {
            return cached;
        }

        let tydesc = Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Map,
            size: std::mem::size_of::<rtdt::Map>() as u32,
            align: std::mem::align_of::<rtdt::Map>() as u32,
            type_info: rtdt::TyInfo {
                map: rtdt::TyInfoMap {
                    key_tydesc,
                    value_tydesc,
                },
            },
        });

        self.tydescs.push(tydesc);
        let ptr = &**self.tydescs.last().unwrap() as *const rtdt::TyDesc;
        self.runtime_map_cache.insert(cache_key, ptr);
        ptr
    }

    /// Create TyDesc for Set<T> from an existing element tydesc.
    ///
    /// Used when building sets from datafun expressions where we already have
    /// the runtime type descriptor for the element type.
    pub fn create_set_from_element_tydesc(&mut self, element_tydesc: *const rtdt::TyDesc) -> *const rtdt::TyDesc {
        if let Some(&cached) = self.runtime_set_cache.get(&element_tydesc) {
            return cached;
        }

        let tydesc = Box::new(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Set,
            size: std::mem::size_of::<rtdt::Set>() as u32,
            align: std::mem::align_of::<rtdt::Set>() as u32,
            type_info: rtdt::TyInfo {
                set: rtdt::TyInfoSet { element_tydesc },
            },
        });

        self.tydescs.push(tydesc);
        let ptr = &**self.tydescs.last().unwrap() as *const rtdt::TyDesc;
        self.runtime_set_cache.insert(element_tydesc, ptr);
        ptr
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
        let layout = unsafe { rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(&temp_tydesc)) };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn compile<'db>(db: &'db Database, source_text: &str) -> AnyResult<crate::tycheck::TypecheckResult<'db>> {
        let source = bct::input::Source::new(db, source_text.to_string());
        let parsed = crate::parser::parse_for_test(db, source);
        let resolved = crate::resolve::resolve_names(db, source, parsed);
        let typechecked = crate::tycheck::type_check(db, parsed, resolved);
        Ok(typechecked)
    }

    #[test]
    fn test_create_bool_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let mut table = TyDescTable::new(&db);

        let ty_bool = Type::Bool;
        let tydesc = table.get_or_create(&ty_bool);

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Bool);
            assert_eq!(td.size(), 1);
            assert_eq!(td.align(), 1);
        }
        Ok(())
    }

    #[test]
    fn test_create_u32_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let mut table = TyDescTable::new(&db);

        let ty_u32 = Type::U32;
        let tydesc = table.get_or_create(&ty_u32);

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::U32);
            assert_eq!(td.size(), 4);
            assert_eq!(td.align(), 4);
        }
        Ok(())
    }

    #[test]
    fn test_create_f32_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let mut table = TyDescTable::new(&db);

        let ty_f32 = Type::F32;
        let tydesc = table.get_or_create(&ty_f32);

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::F32);
            assert_eq!(td.size(), 4);
            assert_eq!(td.align(), 4);
        }
        Ok(())
    }

    #[test]
    fn test_create_int_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let mut table = TyDescTable::new(&db);

        let ty_int = Type::Int;
        let tydesc = table.get_or_create(&ty_int);

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Int);
            assert_eq!(td.size(), std::mem::size_of::<rtdt::Int>() as u32);
            assert_eq!(td.align(), std::mem::align_of::<rtdt::Int>() as u32);
        }
        Ok(())
    }

    #[test]
    fn test_create_string_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let mut table = TyDescTable::new(&db);

        let ty_string = Type::String;
        let tydesc = table.get_or_create(&ty_string);

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::String);
            assert_eq!(td.size(), std::mem::size_of::<rtdt::String>() as u32);
            assert_eq!(td.align(), std::mem::align_of::<rtdt::String>() as u32);
        }
        Ok(())
    }

    #[test]
    fn test_create_data_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let mut table = TyDescTable::new(&db);

        let ty_data = Type::Data;
        let tydesc = table.get_or_create(&ty_data);

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Data);
            assert_eq!(td.size(), std::mem::size_of::<rtdt::Data>() as u32);
            assert_eq!(td.align(), std::mem::align_of::<rtdt::Data>() as u32);
        }
        Ok(())
    }

    #[test]
    fn test_create_error_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let mut table = TyDescTable::new(&db);

        let ty_error = Type::Error;
        let tydesc = table.get_or_create(&ty_error);

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Error);
            assert_eq!(td.size(), std::mem::size_of::<rtdt::Error>() as u32);
            assert_eq!(td.align(), std::mem::align_of::<rtdt::Error>() as u32);
        }
        Ok(())
    }

    #[test]
    fn test_create_tuple_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@(@true, @42)")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Tuple);
            let tuple_info = td.tuple_info();
            assert_eq!(tuple_info.num_fields(), 2);

            let fields = tuple_info.fields();
            let field0_td = rtdt::TyDescRef::from_ptr(fields[0].tydesc);
            let field1_td = rtdt::TyDescRef::from_ptr(fields[1].tydesc);
            assert_eq!(field0_td.type_tag(), rtdt::TyTag::Bool);
            assert_eq!(field1_td.type_tag(), rtdt::TyTag::U32);

            // Check layout computed offsets.
            assert_eq!(fields[0].offset, 0);
            assert_eq!(fields[1].offset, 4); // Aligned to U32
        }
        Ok(())
    }

    #[test]
    fn test_create_struct_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@{x = @1, y = @2}")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Struct);
            let struct_info = td.struct_info();
            assert_eq!(struct_info.num_fields(), 2);

            let fields = struct_info.fields();
            let field0_td = rtdt::TyDescRef::from_ptr(fields[0].tydesc);
            let field1_td = rtdt::TyDescRef::from_ptr(fields[1].tydesc);
            assert_eq!(field0_td.type_tag(), rtdt::TyTag::U32);
            assert_eq!(field1_td.type_tag(), rtdt::TyTag::U32);

            // Check field names.
            let name0 = std::slice::from_raw_parts(fields[0].name, fields[0].name_len as usize);
            let name1 = std::slice::from_raw_parts(fields[1].name, fields[1].name_len as usize);
            assert_eq!(name0, b"x");
            assert_eq!(name1, b"y");
        }
        Ok(())
    }

    #[test]
    fn test_create_enum_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum { Ok, Error } / @enum Ok")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Enum);
            let enum_info = td.enum_info();
            assert_eq!(enum_info.num_variants(), 2);

            let variants = enum_info.variants();
            let name0 = std::slice::from_raw_parts(variants[0].name, variants[0].name_len as usize);
            let name1 = std::slice::from_raw_parts(variants[1].name, variants[1].name_len as usize);
            assert_eq!(name0, b"Ok");
            assert_eq!(name1, b"Error");

            // No payload variants should have null payload tydesc.
            assert!(variants[0].payload.is_null());
            assert!(variants[1].payload.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_create_enum_with_payload_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum { Ok(@u32), Err(@string) } / @enum Ok(@42)")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Enum);
            let enum_info = td.enum_info();
            assert_eq!(enum_info.num_variants(), 2);

            let variants = enum_info.variants();

            // Ok variant has u32 payload.
            assert!(!variants[0].payload.is_null());
            let payload0_td = rtdt::TyDescRef::from_ptr(variants[0].payload);
            assert_eq!(payload0_td.type_tag(), rtdt::TyTag::U32);

            // Err variant has string payload.
            assert!(!variants[1].payload.is_null());
            let payload1_td = rtdt::TyDescRef::from_ptr(variants[1].payload);
            assert_eq!(payload1_td.type_tag(), rtdt::TyTag::String);
        }
        Ok(())
    }

    #[test]
    fn test_create_list_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@[@1, @2, @3]")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::List);
            assert_eq!(td.size(), std::mem::size_of::<rtdt::List>() as u32);
            assert_eq!(td.align(), std::mem::align_of::<rtdt::List>() as u32);

            let element_tydesc = td.list_element_ty();
            let element_td = rtdt::TyDescRef::from_ptr(element_tydesc.as_ptr());
            assert_eq!(element_td.type_tag(), rtdt::TyTag::U32);
        }
        Ok(())
    }

    #[test]
    fn test_create_map_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @map <@u32, @string> / @map { @1 = @\"one\", @2 = @\"two\" }")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Map);
            assert_eq!(td.size(), std::mem::size_of::<rtdt::Map>() as u32);
            assert_eq!(td.align(), std::mem::align_of::<rtdt::Map>() as u32);

            let key_tydesc = td.map_key_ty();
            let value_tydesc = td.map_value_ty();
            let key_td = rtdt::TyDescRef::from_ptr(key_tydesc.as_ptr());
            let value_td = rtdt::TyDescRef::from_ptr(value_tydesc.as_ptr());
            assert_eq!(key_td.type_tag(), rtdt::TyTag::U32);
            assert_eq!(value_td.type_tag(), rtdt::TyTag::String);
        }
        Ok(())
    }

    #[test]
    fn test_create_set_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @set <@u32> / @set { @1, @2, @3 }")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Set);
            assert_eq!(td.size(), std::mem::size_of::<rtdt::Set>() as u32);
            assert_eq!(td.align(), std::mem::align_of::<rtdt::Set>() as u32);

            let element_tydesc = td.set_element_ty();
            let element_td = rtdt::TyDescRef::from_ptr(element_tydesc.as_ptr());
            assert_eq!(element_td.type_tag(), rtdt::TyTag::U32);
        }
        Ok(())
    }

    #[test]
    fn test_create_option_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@u32 / some @42")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Option);
            assert!(td.size() > 0);
            assert!(td.align() > 0);

            let inner_tydesc = td.option_inner_ty();
            let inner_td = rtdt::TyDescRef::from_ptr(inner_tydesc.as_ptr());
            assert_eq!(inner_td.type_tag(), rtdt::TyTag::U32);
        }
        Ok(())
    }

    #[test]
    fn test_create_result_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @!@u32 / ok @42")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Result);
            assert!(td.size() > 0);
            assert!(td.align() > 0);

            let ok_tydesc = td.result_ok_ty();
            let ok_td = rtdt::TyDescRef::from_ptr(ok_tydesc.as_ptr());
            assert_eq!(ok_td.type_tag(), rtdt::TyTag::U32);
        }
        Ok(())
    }

    #[test]
    fn test_tydesc_deduplication() -> AnyResult<()> {
        let db = Database::default();
        let mut table = TyDescTable::new(&db);

        let ty_bool = Type::Bool;
        let ptr1 = table.get_or_create(&ty_bool);
        let ptr2 = table.get_or_create(&ty_bool);

        // Same type should return same pointer.
        assert_eq!(ptr1, ptr2);

        // Check different types get different pointers.
        let ty_u32 = Type::U32;
        let ptr3 = table.get_or_create(&ty_u32);
        assert_ne!(ptr1, ptr3);

        Ok(())
    }

    #[test]
    fn test_nested_tuple_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@(@(@1, @2), @(@3, @4))")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Tuple);
            let tuple_info = td.tuple_info();
            assert_eq!(tuple_info.num_fields(), 2);

            let fields = tuple_info.fields();

            // Both fields should be tuples.
            let field0_td = rtdt::TyDescRef::from_ptr(fields[0].tydesc);
            let field1_td = rtdt::TyDescRef::from_ptr(fields[1].tydesc);
            assert_eq!(field0_td.type_tag(), rtdt::TyTag::Tuple);
            assert_eq!(field1_td.type_tag(), rtdt::TyTag::Tuple);

            // Check nested tuple structure.
            let nested_info = field0_td.tuple_info();
            assert_eq!(nested_info.num_fields(), 2);
        }
        Ok(())
    }

    #[test]
    fn test_option_of_list_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@[@u32] / some @[@1, @2]")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::Option);
            let inner_tydesc = td.option_inner_ty();
            let inner_td = rtdt::TyDescRef::from_ptr(inner_tydesc.as_ptr());
            assert_eq!(inner_td.type_tag(), rtdt::TyTag::List);

            let element_tydesc = inner_td.list_element_ty();
            let element_td = rtdt::TyDescRef::from_ptr(element_tydesc.as_ptr());
            assert_eq!(element_td.type_tag(), rtdt::TyTag::U32);
        }
        Ok(())
    }

    #[test]
    fn test_list_of_option_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @[@?@u32] / @[some @42, @none]")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            let td = rtdt::TyDescRef::from_ptr(tydesc);
            assert_eq!(td.type_tag(), rtdt::TyTag::List);
            let element_tydesc = td.list_element_ty();
            let element_td = rtdt::TyDescRef::from_ptr(element_tydesc.as_ptr());
            assert_eq!(element_td.type_tag(), rtdt::TyTag::Option);

            let inner_tydesc = element_td.option_inner_ty();
            let inner_td = rtdt::TyDescRef::from_ptr(inner_tydesc.as_ptr());
            assert_eq!(inner_td.type_tag(), rtdt::TyTag::U32);
        }
        Ok(())
    }
}
