//! Type descriptor table.
//!
//! Converts `IrType` to runtime `TyDesc` pointers. TyDescs provide size, alignment,
//! and type-specific info (field offsets, element types) for runtime operations.

use datalove_rtdt as rtdt;
use datalove_rtdt::{TyDesc, TyDescRef};
use datalove_datafun_ir::IrType;

/// Table for converting `IrType` to runtime `TyDesc` pointers.
pub struct IrTyDescTable {
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
        let tydesc = self.create_tydesc(ty);
        self.tydescs.push(tydesc);
        &**self.tydescs.last().unwrap() as *const TyDesc
    }

    fn create_tydesc(&mut self, ty: &IrType) -> Box<TyDesc> {
        match ty {
            IrType::Unit => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Tuple,
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
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::U8 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U8,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::U16 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U16,
                size: 2,
                align: 2,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::U32 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::U64 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::U64,
                size: 8,
                align: 8,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::I8 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I8,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::I16 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I16,
                size: 2,
                align: 2,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::I32 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::I64 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::I64,
                size: 8,
                align: 8,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::Int => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Int,
                size: std::mem::size_of::<rtdt::Int>() as u32,
                align: std::mem::align_of::<rtdt::Int>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::F32 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::F32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::F64 => Box::new(TyDesc {
                type_tag: rtdt::TyTag::F64,
                size: 8,
                align: 8,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::String => Box::new(TyDesc {
                type_tag: rtdt::TyTag::String,
                size: std::mem::size_of::<rtdt::String>() as u32,
                align: std::mem::align_of::<rtdt::String>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::Data => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Data,
                size: std::mem::size_of::<rtdt::Data>() as u32,
                align: std::mem::align_of::<rtdt::Data>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::Error => Box::new(TyDesc {
                type_tag: rtdt::TyTag::Error,
                size: std::mem::size_of::<rtdt::Error>() as u32,
                align: std::mem::align_of::<rtdt::Error>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            }),
            IrType::Tuple(fields) => self.create_tuple_tydesc(fields),
            IrType::Struct(fields) => self.create_struct_tydesc(fields),
            IrType::Enum(variants) => self.create_enum_tydesc(variants),
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
        // to allocate only 8 bytes for Ref types.
        let field_info = vec![rtdt::TyInfoTupleField {
            offset: 0,
            tydesc: inner_tydesc,
        }];
        self.tuple_fields.push(field_info);
        let fields_ptr = self.tuple_fields.last().unwrap().as_ptr();

        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Tuple,  // Use Tuple tag to store inner tydesc
            size: 8,  // Always pointer-sized
            align: 8, // Always pointer-aligned
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

    fn create_list_tydesc(&mut self, elem: &IrType) -> Box<TyDesc> {
        let element_tydesc = self.get_or_create(elem);
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::List,
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
            size: std::mem::size_of::<rtdt::Set>() as u32,
            align: std::mem::align_of::<rtdt::Set>() as u32,
            type_info: rtdt::TyInfo {
                set: rtdt::TyInfoSet { element_tydesc },
            },
        })
    }

    fn create_map_tydesc(&mut self, key: &IrType, val: &IrType) -> Box<TyDesc> {
        let key_tydesc = self.get_or_create(key);
        let value_tydesc = self.get_or_create(val);
        Box::new(TyDesc {
            type_tag: rtdt::TyTag::Map,
            size: std::mem::size_of::<rtdt::Map>() as u32,
            align: std::mem::align_of::<rtdt::Map>() as u32,
            type_info: rtdt::TyInfo {
                map: rtdt::TyInfoMap { key_tydesc, value_tydesc },
            },
        })
    }

    fn create_option_tydesc(&mut self, inner: &IrType) -> Box<TyDesc> {
        let inner_tydesc = self.get_or_create(inner);

        let temp_tydesc = TyDesc {
            type_tag: rtdt::TyTag::Option,
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
