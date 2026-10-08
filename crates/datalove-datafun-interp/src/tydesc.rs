//! Type descriptor table.
//!
//! Converts `IrType` to runtime `TyDesc` pointers. TyDescs provide size, alignment,
//! and type-specific info (field offsets, element types) for runtime operations.

use rustc_hash::FxHashMap;

use datalove_rtdt as rtdt;
use datalove_rtdt::{TyDesc, TyDescRef};
use datalove_datafun_ir::IrType;

/// Table for converting `IrType` to runtime `TyDesc` pointers.
pub struct IrTyDescTable {
    /// Cache mapping IrType to existing TyDesc pointer.
    ///
    /// `FxHashMap` rather than the default: an `IrType` is a tree, the default
    /// hasher walks all of it with SipHash, and this lookup is reached once per
    /// value in a frame layout and once per argument at a jit call boundary. It
    /// was the largest single entry in both profiles.
    cache: FxHashMap<IrType, *const TyDesc>,
    /// Storage for TyDesc allocations.
    tydescs: Vec<Box<TyDesc>>,
    /// Storage for tuple field arrays.
    tuple_fields: Vec<Vec<rtdt::TyInfoTupleField>>,
    /// Storage for struct field arrays.
    struct_fields: Vec<Vec<rtdt::TyInfoStructField>>,
    /// Storage for enum variant arrays.
    enum_variants: Vec<Vec<rtdt::TyInfoEnumVariant>>,
    /// Storage for table column arrays.
    table_columns: Vec<Vec<rtdt::TyInfoTableColumn>>,
    /// Storage for field name strings (to keep them alive).
    field_names: Vec<String>,
}

impl IrTyDescTable {
    pub fn new() -> Self {
        Self {
            cache: FxHashMap::default(),
            tydescs: Vec::new(),
            tuple_fields: Vec::new(),
            struct_fields: Vec::new(),
            enum_variants: Vec::new(),
            table_columns: Vec::new(),
            field_names: Vec::new(),
        }
    }

    /// Get or create a TyDesc for the given IrType.
    pub fn get_or_create(&mut self, ty: &IrType) -> *const TyDesc {
        if let Some(&ptr) = self.cache.get(ty) {
            return ptr;
        }
        let mut tydesc = self.create_tydesc(ty);
        // The descriptors it points to were made first, so its own flags can
        // be read off them.
        tydesc.flags = tydesc.computed_flags();
        self.tydescs.push(tydesc);
        let ptr = &**self.tydescs.last().unwrap() as *const TyDesc;
        self.cache.insert(ty.clone(), ptr);
        ptr
    }

    fn create_tydesc(&mut self, ty: &IrType) -> Box<TyDesc> {
        match ty {
            IrType::Unit => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Tuple,
                flags: 0,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    tuple: rtdt::TyInfoTuple {
                        num_fields: 0,
                        // Use dangling pointer for empty slice (Rust requires non-null).
                        fields: std::ptr::NonNull::dangling().as_ptr(),
                    },
                },
            }),
            IrType::Bool => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Bool,
                flags: 0,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::U8 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U8,
                flags: 0,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::U16 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U16,
                flags: 0,
                size: 2,
                align: 2,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::U32 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U32,
                flags: 0,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::U64 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U64,
                flags: 0,
                size: 8,
                align: 8,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::I8 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I8,
                flags: 0,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::I16 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I16,
                flags: 0,
                size: 2,
                align: 2,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::I32 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I32,
                flags: 0,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::I64 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I64,
                flags: 0,
                size: 8,
                align: 8,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::Index => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Index,
                flags: 0,
                size: rtdt::INDEX_SIZE,
                align: rtdt::INDEX_ALIGN,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::Offset => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Offset,
                flags: 0,
                size: rtdt::INDEX_SIZE,
                align: rtdt::INDEX_ALIGN,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::Int => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Int,
                flags: 0,
                size: std::mem::size_of::<rtdt::Int>() as u32,
                align: std::mem::align_of::<rtdt::Int>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::F32 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::F32,
                flags: 0,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::F64 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::F64,
                flags: 0,
                size: 8,
                align: 8,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::String => Box::new(TyDesc {
                type_tag: rtdt::TyTag::String,
                flags: 0,
                size: std::mem::size_of::<rtdt::String>() as u32,
                align: std::mem::align_of::<rtdt::String>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::Data => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Data,
                flags: 0,
                size: std::mem::size_of::<rtdt::Data>() as u32,
                align: std::mem::align_of::<rtdt::Data>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::Error => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Error,
                flags: 0,
                size: std::mem::size_of::<rtdt::Error>() as u32,
                align: std::mem::align_of::<rtdt::Error>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            }),
            IrType::Tuple(fields) => self.create_tuple_tydesc(fields),
            IrType::Struct(fields) => self.create_struct_tydesc(fields),
            IrType::Enum(variants) => self.create_enum_tydesc(variants),
            IrType::Atom(name) => self.create_atom_tydesc(name),
            IrType::Term(name, payload) => self.create_term_tydesc(name, payload),
            IrType::List(elem) => self.create_list_tydesc(elem),
            IrType::Set(elem) => self.create_set_tydesc(elem),
            IrType::Map(key, val) => self.create_map_tydesc(key, val),
            IrType::Option(inner) => self.create_option_tydesc(inner),
            IrType::Result(inner) => self.create_result_tydesc(inner),
            IrType::Tensor(elem, rank) => self.create_tensor_tydesc(elem, *rank),
            IrType::Ref(inner) => self.create_ref_tydesc(inner),
            IrType::Table(columns) => self.create_table_tydesc(columns),
        }
    }

    fn create_ref_tydesc(&mut self, inner: &IrType) -> Box<TyDesc> {
        // Create inner tydesc (stored in type_info for reading through the ref).
        let inner_tydesc = self.get_or_create(inner);

        // Ref is pointer-sized. We use a 1-element tuple containing the inner type
        // as a hack to store the inner tydesc. The layout uses special handling
        // to allocate only a pointer for Ref types.
        let field_info = vec![rtdt::TyInfoTupleField {
            offset: 0,
            tydesc: inner_tydesc,
        }];
        self.tuple_fields.push(field_info);
        let fields_ptr = self.tuple_fields.last().unwrap().as_ptr();

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Tuple, flags: 0,  // Use Tuple tag to store inner tydesc
            size: std::mem::size_of::<*const u8>() as u32,
            align: std::mem::align_of::<*const u8>() as u32,
            type_info: rtdt::TyInfo {
                tuple: rtdt::TyInfoTuple {
                    num_fields: 1,
                    fields: fields_ptr,
                },
            },
        })
    }

    fn create_tuple_tydesc(&mut self, fields: &[IrType]) -> Box<TyDesc> {
        let field_tydescs: Vec<_> = fields.iter()
            .map(|f| self.get_or_create(f))
            .collect();

        // Create field info with placeholder offsets.
        let mut field_info: Vec<_> = field_tydescs.iter()
            .map(|&tydesc| rtdt::TyInfoTupleField { offset: 0, tydesc })
            .collect();

        // Compute layout.
        let layout = unsafe {
            let temp_tydesc = TyDesc {
                type_tag: rtdt::TyTag::Tuple,
                flags: 0,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    tuple: rtdt::TyInfoTuple {
                        num_fields: fields.len() as u32,
                        fields: field_info.as_ptr(),
                    },
                },
            };
            rtdt::layout::compute_tuple_layout(TyDescRef::from_ptr(&temp_tydesc))
        };

        // Update offsets.
        for (i, fi) in field_info.iter_mut().enumerate() {
            fi.offset = layout.field_offsets[i];
        }

        self.tuple_fields.push(field_info);
        let fields_ptr = self.tuple_fields.last().unwrap().as_ptr();

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Tuple,
            flags: 0,
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

    fn create_struct_tydesc(&mut self, fields: &[(String, IrType)]) -> Box<TyDesc> {
        let field_tydescs: Vec<_> = fields.iter()
            .map(|(_, ty)| self.get_or_create(ty))
            .collect();

        // Store field names.
        let name_start = self.field_names.len();
        for (name, _) in fields {
            self.field_names.push(name.clone());
        }

        // Create field info with placeholder offsets.
        let mut field_info: Vec<_> = fields.iter().enumerate()
            .map(|(i, _)| {
                let name_ref = &self.field_names[name_start + i];
                rtdt::TyInfoStructField {
                    name: name_ref.as_ptr(),
                    name_len: name_ref.len() as u32,
                    offset: 0,
                    tydesc: field_tydescs[i],
                }
            })
            .collect();

        // Compute layout.
        let layout = unsafe {
            let temp_tydesc = TyDesc {
                type_tag: rtdt::TyTag::Struct,
                flags: 0,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    struct_: rtdt::TyInfoStruct {
                        num_fields: fields.len() as u32,
                        fields: field_info.as_ptr(),
                    },
                },
            };
            rtdt::layout::compute_struct_layout(TyDescRef::from_ptr(&temp_tydesc))
        };

        // Update offsets.
        for (i, fi) in field_info.iter_mut().enumerate() {
            fi.offset = layout.field_offsets[i];
        }

        self.struct_fields.push(field_info);
        let fields_ptr = self.struct_fields.last().unwrap().as_ptr();

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Struct,
            flags: 0,
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

    fn create_enum_tydesc(&mut self, variants: &[(String, Option<IrType>)]) -> Box<TyDesc> {
        // Create tydescs for each variant payload.
        let payload_tydescs: Vec<Option<*const TyDesc>> = variants.iter()
            .map(|(_, payload)| payload.as_ref().map(|p| self.get_or_create(p)))
            .collect();

        // Store variant names.
        let name_start = self.field_names.len();
        for (name, _) in variants {
            self.field_names.push(name.clone());
        }

        // Create variant info with placeholder offsets.
        let mut variant_info: Vec<_> = variants.iter().enumerate()
            .map(|(i, _)| {
                let name_ref = &self.field_names[name_start + i];
                rtdt::TyInfoEnumVariant {
                    name: name_ref.as_ptr(),
                    name_len: name_ref.len() as u32,
                    offset: 0,
                    payload: payload_tydescs[i].unwrap_or(std::ptr::null()),
                }
            })
            .collect();

        // Compute layout.
        let layout = unsafe {
            let temp_tydesc = TyDesc {
                type_tag: rtdt::TyTag::Enum,
                flags: 0,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    enum_: rtdt::TyInfoEnum {
                        num_variants: variants.len() as u32,
                        variants: variant_info.as_ptr(),
                    },
                },
            };
            rtdt::layout::compute_enum_layout(TyDescRef::from_ptr(&temp_tydesc))
        };

        // Update offsets.
        for (i, vi) in variant_info.iter_mut().enumerate() {
            vi.offset = layout.variant_offsets[i];
        }

        self.enum_variants.push(variant_info);
        let variants_ptr = self.enum_variants.last().unwrap().as_ptr();

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Enum,
            flags: 0,
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

    fn create_atom_tydesc(&mut self, name: &str) -> Box<TyDesc> {
        self.field_names.push(name.to_string());
        let name_ref = self.field_names.last().unwrap();
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Atom,
            flags: 0,
            size: 0,
            align: 1,
            type_info: rtdt::TyInfo {
                atom: rtdt::TyInfoAtom {
                    name: name_ref.as_ptr(),
                    name_len: name_ref.len() as u32,
                },
            },
        })
    }

    fn create_term_tydesc(&mut self, name: &str, payload: &IrType) -> Box<TyDesc> {
        let payload_tydesc = self.get_or_create(payload);
        let payload_layout = unsafe { &*payload_tydesc };
        let size = payload_layout.size;
        let align = payload_layout.align;

        self.field_names.push(name.to_string());
        let name_ref = self.field_names.last().unwrap();
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Term,
            flags: 0,
            size,
            align,
            type_info: rtdt::TyInfo {
                term: rtdt::TyInfoTerm {
                    name: name_ref.as_ptr(),
                    name_len: name_ref.len() as u32,
                    payload: payload_tydesc,
                },
            },
        })
    }

    fn create_list_tydesc(&mut self, elem: &IrType) -> Box<TyDesc> {
        let element_tydesc = self.get_or_create(elem);
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::List,
            flags: 0,
            size: std::mem::size_of::<rtdt::List>() as u32,
            align: std::mem::align_of::<rtdt::List>() as u32,
            type_info: rtdt::TyInfo {
                list: rtdt::TyInfoList { element_tydesc },
            },
        })
    }

    fn create_set_tydesc(&mut self, elem: &IrType) -> Box<TyDesc> {
        let element_tydesc = self.get_or_create(elem);
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Set,
            flags: 0,
            size: std::mem::size_of::<rtdt::Set>() as u32,
            align: std::mem::align_of::<rtdt::Set>() as u32,
            type_info: rtdt::TyInfo {
                set: unsafe { rtdt::TyInfoSet::new(element_tydesc) },
            },
        })
    }

    fn create_map_tydesc(&mut self, key: &IrType, val: &IrType) -> Box<TyDesc> {
        let key_tydesc = self.get_or_create(key);
        let value_tydesc = self.get_or_create(val);
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Map,
            flags: 0,
            size: std::mem::size_of::<rtdt::Map>() as u32,
            align: std::mem::align_of::<rtdt::Map>() as u32,
            type_info: rtdt::TyInfo {
                map: unsafe { rtdt::TyInfoMap::new(key_tydesc, value_tydesc) },
            },
        })
    }

    fn create_option_tydesc(&mut self, inner: &IrType) -> Box<TyDesc> {
        let inner_tydesc = self.get_or_create(inner);

        let temp_tydesc = TyDesc {
            type_tag: rtdt::TyTag::Option,
            flags: 0,
            size: 0,
            align: 1,
            type_info: rtdt::TyInfo {
                option: rtdt::TyInfoOption { inner_tydesc },
            },
        };

        let layout = unsafe {
            rtdt::layout::compute_option_layout(TyDescRef::from_ptr(&temp_tydesc))
        };

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Option,
            flags: 0,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                option: rtdt::TyInfoOption { inner_tydesc },
            },
        })
    }

    fn create_result_tydesc(&mut self, inner: &IrType) -> Box<TyDesc> {
        let ok_tydesc = self.get_or_create(inner);

        let temp_tydesc = TyDesc {
            type_tag: rtdt::TyTag::Result,
            flags: 0,
            size: 0,
            align: 1,
            type_info: rtdt::TyInfo {
                result: rtdt::TyInfoResult { ok_tydesc },
            },
        };

        let layout = unsafe {
            rtdt::layout::compute_result_layout(TyDescRef::from_ptr(&temp_tydesc))
        };

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Result,
            flags: 0,
            size: layout.size,
            align: layout.align,
            type_info: rtdt::TyInfo {
                result: rtdt::TyInfoResult { ok_tydesc },
            },
        })
    }

    fn create_tensor_tydesc(&mut self, elem: &IrType, rank: u32) -> Box<TyDesc> {
        let element_tydesc = self.get_or_create(elem);
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Tensor,
            flags: 0,
            size: std::mem::size_of::<rtdt::Tensor>() as u32,
            align: std::mem::align_of::<rtdt::Tensor>() as u32,
            type_info: rtdt::TyInfo {
                tensor: rtdt::TyInfoTensor { element_tydesc, rank },
            },
        })
    }

    fn create_table_tydesc(&mut self, columns: &[(String, Box<IrType>)]) -> Box<TyDesc> {
        // Create tydescs for each column type.
        let column_tydescs: Vec<_> = columns.iter()
            .map(|(_, ty)| self.get_or_create(ty))
            .collect();

        // Store column names.
        let name_start = self.field_names.len();
        for (name, _) in columns {
            self.field_names.push(name.clone());
        }

        // Create column info.
        let column_info: Vec<_> = columns.iter().enumerate()
            .map(|(i, _)| {
                let name_ref = &self.field_names[name_start + i];
                rtdt::TyInfoTableColumn {
                    name: name_ref.as_ptr(),
                    name_len: name_ref.len() as u32,
                    tydesc: column_tydescs[i],
                }
            })
            .collect();

        self.table_columns.push(column_info);
        let columns_ptr = self.table_columns.last().unwrap().as_ptr();

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Table,
            flags: 0,
            size: std::mem::size_of::<rtdt::Table>() as u32,
            align: std::mem::align_of::<rtdt::Table>() as u32,
            type_info: rtdt::TyInfo {
                table: rtdt::TyInfoTable {
                    num_columns: columns.len() as u32,
                    columns: columns_ptr,
                },
            },
        })
    }
}

impl Default for IrTyDescTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_caching() {
        let mut table = IrTyDescTable::new();

        let td1 = table.get_or_create(&IrType::I64);
        let td2 = table.get_or_create(&IrType::I64);

        assert_eq!(td1, td2, "Same type should return same pointer");
        assert_eq!(table.tydescs.len(), 1, "Should only allocate once");
    }

    #[test]
    fn test_different_types_not_shared() {
        let mut table = IrTyDescTable::new();

        let td1 = table.get_or_create(&IrType::I64);
        let td2 = table.get_or_create(&IrType::I32);

        assert_ne!(td1, td2, "Different types should return different pointers");
        assert_eq!(table.tydescs.len(), 2);
    }

    #[test]
    fn test_tuple_caching() {
        let mut table = IrTyDescTable::new();

        let tuple_ty = IrType::Tuple(vec![IrType::I64, IrType::Bool]);
        let td1 = table.get_or_create(&tuple_ty);
        let td2 = table.get_or_create(&tuple_ty);

        assert_eq!(td1, td2, "Same tuple type should return same pointer");
    }

    #[test]
    fn test_result_caching() {
        let mut table = IrTyDescTable::new();

        let result_ty = IrType::Result(Box::new(IrType::U32));
        let td1 = table.get_or_create(&result_ty);
        let td2 = table.get_or_create(&result_ty);

        assert_eq!(td1, td2, "Same Result type should return same pointer");
    }

    #[test]
    fn test_repeated_calls_no_growth() {
        let mut table = IrTyDescTable::new();

        let types = [IrType::I64, IrType::Bool, IrType::U32];

        // First pass: create all types.
        for ty in &types {
            table.get_or_create(ty);
        }
        let count_after_first = table.tydescs.len();

        // Many repeated calls should not grow the table.
        for _ in 0..1000 {
            for ty in &types {
                table.get_or_create(ty);
            }
        }

        assert_eq!(
            table.tydescs.len(),
            count_after_first,
            "Repeated calls should not allocate new tydescs"
        );
    }
}
