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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn compile<'db>(db: &'db Database, source_text: &str) -> AnyResult<crate::tycheck::TypecheckResult<'db>> {
        let source = bct::input::Source::new(db, source_text.to_string());
        let parsed = crate::parser::parse(db, source);
        let resolved = crate::resolve::resolve_names(db, parsed);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Bool);
            assert_eq!((*tydesc).size, 1);
            assert_eq!((*tydesc).align, 1);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::U32);
            assert_eq!((*tydesc).size, 4);
            assert_eq!((*tydesc).align, 4);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::F32);
            assert_eq!((*tydesc).size, 4);
            assert_eq!((*tydesc).align, 4);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Int);
            assert_eq!((*tydesc).size, std::mem::size_of::<rtdt::Int>() as u32);
            assert_eq!((*tydesc).align, std::mem::align_of::<rtdt::Int>() as u32);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::String);
            assert_eq!((*tydesc).size, std::mem::size_of::<rtdt::String>() as u32);
            assert_eq!((*tydesc).align, std::mem::align_of::<rtdt::String>() as u32);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Data);
            assert_eq!((*tydesc).size, std::mem::size_of::<rtdt::Data>() as u32);
            assert_eq!((*tydesc).align, std::mem::align_of::<rtdt::Data>() as u32);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Error);
            assert_eq!((*tydesc).size, std::mem::size_of::<rtdt::Error>() as u32);
            assert_eq!((*tydesc).align, std::mem::align_of::<rtdt::Error>() as u32);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            assert_eq!((*fields[0].tydesc).type_tag, rtdt::TyTag::Bool);
            assert_eq!((*fields[1].tydesc).type_tag, rtdt::TyTag::U32);

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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(struct_info.fields, 2);
            assert_eq!((*fields[0].tydesc).type_tag, rtdt::TyTag::U32);
            assert_eq!((*fields[1].tydesc).type_tag, rtdt::TyTag::U32);

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
        let typechecked = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            let variants = std::slice::from_raw_parts(enum_info.variants, 2);
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
        let typechecked = compile(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            let variants = std::slice::from_raw_parts(enum_info.variants, 2);

            // Ok variant has u32 payload.
            assert!(!variants[0].payload.is_null());
            assert_eq!((*variants[0].payload).type_tag, rtdt::TyTag::U32);

            // Err variant has string payload.
            assert!(!variants[1].payload.is_null());
            assert_eq!((*variants[1].payload).type_tag, rtdt::TyTag::String);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::List);
            assert_eq!((*tydesc).size, std::mem::size_of::<rtdt::List>() as u32);
            assert_eq!((*tydesc).align, std::mem::align_of::<rtdt::List>() as u32);

            let list_info = &(*tydesc).type_info.list;
            assert!(!list_info.element_tydesc.is_null());
            assert_eq!((*list_info.element_tydesc).type_tag, rtdt::TyTag::U32);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Map);
            assert_eq!((*tydesc).size, std::mem::size_of::<rtdt::Map>() as u32);
            assert_eq!((*tydesc).align, std::mem::align_of::<rtdt::Map>() as u32);

            let map_info = &(*tydesc).type_info.map;
            assert!(!map_info.key_tydesc.is_null());
            assert!(!map_info.value_tydesc.is_null());
            assert_eq!((*map_info.key_tydesc).type_tag, rtdt::TyTag::U32);
            assert_eq!((*map_info.value_tydesc).type_tag, rtdt::TyTag::String);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Set);
            assert_eq!((*tydesc).size, std::mem::size_of::<rtdt::Set>() as u32);
            assert_eq!((*tydesc).align, std::mem::align_of::<rtdt::Set>() as u32);

            let set_info = &(*tydesc).type_info.set;
            assert!(!set_info.element_tydesc.is_null());
            assert_eq!((*set_info.element_tydesc).type_tag, rtdt::TyTag::U32);
        }
        Ok(())
    }

    #[test]
    fn test_create_option_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@u32 / @42")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Option);
            assert!((*tydesc).size > 0);
            assert!((*tydesc).align > 0);

            let option_info = &(*tydesc).type_info.option;
            assert!(!option_info.inner_tydesc.is_null());
            assert_eq!((*option_info.inner_tydesc).type_tag, rtdt::TyTag::U32);
        }
        Ok(())
    }

    #[test]
    fn test_create_result_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @!@u32 / @42")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Result);
            assert!((*tydesc).size > 0);
            assert!((*tydesc).align > 0);

            let result_info = &(*tydesc).type_info.result;
            assert!(!result_info.ok_tydesc.is_null());
            assert_eq!((*result_info.ok_tydesc).type_tag, rtdt::TyTag::U32);
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
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(tuple_info.fields, 2);

            // Both fields should be tuples.
            assert_eq!((*fields[0].tydesc).type_tag, rtdt::TyTag::Tuple);
            assert_eq!((*fields[1].tydesc).type_tag, rtdt::TyTag::Tuple);

            // Check nested tuple structure.
            let nested_info = &(*fields[0].tydesc).type_info.tuple;
            assert_eq!(nested_info.num_fields, 2);
        }
        Ok(())
    }

    #[test]
    fn test_option_of_list_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @?@[@u32] / @[@1, @2]")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::Option);
            let option_info = &(*tydesc).type_info.option;
            assert_eq!((*option_info.inner_tydesc).type_tag, rtdt::TyTag::List);

            let list_info = &(*option_info.inner_tydesc).type_info.list;
            assert_eq!((*list_info.element_tydesc).type_tag, rtdt::TyTag::U32);
        }
        Ok(())
    }

    #[test]
    fn test_list_of_option_tydesc() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @[@?@u32] / @[@42, @none]")?;
        let root_type = typechecked.root_type(&db).unwrap();

        let mut table = TyDescTable::new(&db);
        let tydesc = table.get_or_create(root_type.ty(&db));

        unsafe {
            assert_eq!((*tydesc).type_tag, rtdt::TyTag::List);
            let list_info = &(*tydesc).type_info.list;
            assert_eq!((*list_info.element_tydesc).type_tag, rtdt::TyTag::Option);

            let option_info = &(*list_info.element_tydesc).type_info.option;
            assert_eq!((*option_info.inner_tydesc).type_tag, rtdt::TyTag::U32);
        }
        Ok(())
    }
}
