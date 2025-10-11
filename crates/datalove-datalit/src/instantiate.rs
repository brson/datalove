//! Instantiate runtime values and type descriptors from typechecked AST.

use rmx::prelude::*;
use std::collections::HashMap;
use bct::text::InternedText;
use crate::ast::*;
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
                        tuple: rtdt::TyInfoTuple {
                            num_fields: 0,
                            fields: std::ptr::null(),
                        },
                    },
                })
            }
            Type::U32 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::U32,
                    size: 4,
                    align: 4,
                    type_info: rtdt::TyInfo {
                        tuple: rtdt::TyInfoTuple {
                            num_fields: 0,
                            fields: std::ptr::null(),
                        },
                    },
                })
            }
            Type::F32 => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::F32,
                    size: 4,
                    align: 4,
                    type_info: rtdt::TyInfo {
                        tuple: rtdt::TyInfoTuple {
                            num_fields: 0,
                            fields: std::ptr::null(),
                        },
                    },
                })
            }
            Type::Int => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Int,
                    size: std::mem::size_of::<rtdt::Int>() as u32,
                    align: std::mem::align_of::<rtdt::Int>() as u32,
                    type_info: rtdt::TyInfo {
                        tuple: rtdt::TyInfoTuple {
                            num_fields: 0,
                            fields: std::ptr::null(),
                        },
                    },
                })
            }
            Type::String => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::String,
                    size: std::mem::size_of::<rtdt::String>() as u32,
                    align: std::mem::align_of::<rtdt::String>() as u32,
                    type_info: rtdt::TyInfo {
                        tuple: rtdt::TyInfoTuple {
                            num_fields: 0,
                            fields: std::ptr::null(),
                        },
                    },
                })
            }
            Type::Error => {
                Box::new(rtdt::TyDesc {
                    type_tag: rtdt::TyTag::Error,
                    size: std::mem::size_of::<rtdt::Error>() as u32,
                    align: std::mem::align_of::<rtdt::Error>() as u32,
                    type_info: rtdt::TyInfo {
                        tuple: rtdt::TyInfoTuple {
                            num_fields: 0,
                            fields: std::ptr::null(),
                        },
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
            _ => unimplemented!("TyDesc creation for composite types"),
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
}

/// Arena for value allocations.
pub struct ValueArena {
    /// Byte storage with alignment padding.
    data: Vec<u8>,
}

impl ValueArena {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
        }
    }

    /// Allocate aligned memory for a value.
    pub fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        // Align current position.
        let offset = self.data.len();
        let aligned_offset = (offset + align - 1) & !(align - 1);
        let padding = aligned_offset - offset;

        // Add padding.
        self.data.resize(aligned_offset, 0);

        // Reserve space.
        self.data.resize(aligned_offset + size, 0);

        // Return pointer to allocated space.
        unsafe { self.data.as_mut_ptr().add(aligned_offset) }
    }
}

/// An instantiated value with its type descriptor.
///
/// Lifetime 'arena ensures value pointer doesn't outlive the arena.
pub struct InstantiatedValue<'arena> {
    pub value: *const u8,
    pub tydesc: *const rtdt::TyDesc,
    _phantom: std::marker::PhantomData<&'arena ()>,
}

/// Instantiate an expression into a runtime value.
fn instantiate_expr<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFull<'db>,
    ty: &Type<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    value_arena: &mut ValueArena,
) -> AnyResult<*const u8> {
    let expr_and_heap = expr.expr(db);
    let expr_inner = expr_and_heap.expr(db);

    match (expr_inner, ty) {
        (Expr::True, Type::Bool) => {
            let ptr = value_arena.alloc(1, 1);
            unsafe { *ptr = 1 };
            Ok(ptr as *const u8)
        }
        (Expr::False, Type::Bool) => {
            let ptr = value_arena.alloc(1, 1);
            unsafe { *ptr = 0 };
            Ok(ptr as *const u8)
        }
        (Expr::Int(int_expr), Type::U32) => {
            let value_str = int_expr.value(db).as_str(db);
            let value: u32 = value_str.parse()?;

            let ptr = value_arena.alloc(4, 4) as *mut u32;
            unsafe { *ptr = value };
            Ok(ptr as *const u8)
        }
        (Expr::Int(int_expr), Type::Int) => {
            let value_str = int_expr.value(db).as_str(db);

            // Parse as i128 for now (limited bigint support).
            let value: i128 = value_str.parse()?;

            // Convert to limbs (u32 chunks).
            let abs_value = value.unsigned_abs();
            let is_negative = value < 0;

            // Calculate number of limbs needed.
            let mut limbs = Vec::new();
            let mut remaining = abs_value;
            while remaining > 0 {
                limbs.push((remaining & 0xFFFFFFFF) as u32);
                remaining >>= 32;
            }

            // Handle zero case.
            if limbs.is_empty() {
                limbs.push(0);
            }

            // Allocate space for limbs.
            let limbs_size = limbs.len() * std::mem::size_of::<u32>();
            let limbs_ptr = if limbs_size > 0 {
                let ptr = value_arena.alloc(limbs_size, 4) as *mut u32;
                unsafe {
                    for (i, &limb) in limbs.iter().enumerate() {
                        *ptr.add(i) = limb;
                    }
                }
                ptr as *const u32
            } else {
                std::ptr::null()
            };

            // Allocate space for rtdt::Int struct.
            let int_size = std::mem::size_of::<rtdt::Int>();
            let int_align = std::mem::align_of::<rtdt::Int>();
            let int_ptr = value_arena.alloc(int_size, int_align) as *mut rtdt::Int;

            unsafe {
                (*int_ptr).data = limbs_ptr;
                (*int_ptr).size_and_sign = if is_negative {
                    -(limbs.len() as i32)
                } else {
                    limbs.len() as i32
                };
                (*int_ptr).capacity = limbs.len() as u32;
            }

            Ok(int_ptr as *const u8)
        }
        (Expr::Float(float_expr), Type::F32) => {
            let value_str = float_expr.value(db).as_str(db);
            let value: f32 = value_str.parse()?;

            let ptr = value_arena.alloc(4, 4) as *mut f32;
            unsafe { *ptr = value };
            Ok(ptr as *const u8)
        }
        (Expr::String(string_expr), Type::String) => {
            let value_str_raw = string_expr.value(db).as_str(db);

            // Strip surrounding quotes if present.
            //
            // The string tokens include the quotes. Hm.
            let value_str = if value_str_raw.starts_with('"') && value_str_raw.ends_with('"') {
                &value_str_raw[1..value_str_raw.len()-1]
            } else {
                bug!();
            };

            // Allocate space for the string data.
            let data_len = value_str.len();
            let data_ptr = if data_len > 0 {
                let ptr = value_arena.alloc(data_len, 1);
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        value_str.as_ptr(),
                        ptr,
                        data_len,
                    );
                }
                ptr as *const u8
            } else {
                std::ptr::null()
            };

            // Allocate space for rtdt::String struct.
            let string_size = std::mem::size_of::<rtdt::String>();
            let string_align = std::mem::align_of::<rtdt::String>();
            let string_ptr = value_arena.alloc(string_size, string_align) as *mut rtdt::String;

            unsafe {
                (*string_ptr).data = data_ptr;
                (*string_ptr).size = data_len as u32;
                (*string_ptr).capacity = data_len as u32;
            }

            Ok(string_ptr as *const u8)
        }
        (Expr::AnonTuple(tuple_expr), Type::AnonTuple(tuple_ty)) => {
            let tuple_tydesc = tydesc_table.get_or_create(ty);
            instantiate_tuple(db, &tuple_expr.elements(db), &tuple_ty.fields(db), tydesc_table, value_arena, tuple_tydesc)
        }
        (Expr::NamedTuple(tuple_expr), Type::NamedTuple(tuple_ty)) => {
            let tuple_tydesc = tydesc_table.get_or_create(ty);
            instantiate_tuple(db, &tuple_expr.elements(db), &tuple_ty.fields(db), tydesc_table, value_arena, tuple_tydesc)
        }
        (Expr::AnonStruct(struct_expr), Type::AnonStruct(struct_ty)) => {
            let struct_tydesc = tydesc_table.get_or_create(ty);
            instantiate_struct(db, &struct_expr.fields(db), &struct_ty.fields(db), tydesc_table, value_arena, struct_tydesc)
        }
        (Expr::AnonStruct(struct_expr), Type::NamedStruct(struct_ty)) => {
            // Anon struct can be coerced to named struct.
            let struct_tydesc = tydesc_table.get_or_create(ty);
            instantiate_struct(db, &struct_expr.fields(db), &struct_ty.fields(db), tydesc_table, value_arena, struct_tydesc)
        }
        (Expr::NamedStruct(struct_expr), Type::NamedStruct(struct_ty)) => {
            let struct_tydesc = tydesc_table.get_or_create(ty);
            instantiate_struct(db, &struct_expr.fields(db), &struct_ty.fields(db), tydesc_table, value_arena, struct_tydesc)
        }
        (Expr::AnonEnum(enum_expr), Type::AnonEnum(enum_ty)) => {
            let enum_tydesc = tydesc_table.get_or_create(ty);
            instantiate_enum(db, enum_expr.variant_name(db), enum_expr.payload(db), &enum_ty.variants(db), tydesc_table, value_arena, enum_tydesc)
        }
        (Expr::AnonEnum(enum_expr), Type::NamedEnum(enum_ty)) => {
            // Anon enum can be coerced to named enum.
            let enum_tydesc = tydesc_table.get_or_create(ty);
            instantiate_enum(db, enum_expr.variant_name(db), enum_expr.payload(db), &enum_ty.variants(db), tydesc_table, value_arena, enum_tydesc)
        }
        (Expr::NamedEnum(enum_expr), Type::NamedEnum(enum_ty)) => {
            let enum_tydesc = tydesc_table.get_or_create(ty);
            instantiate_enum(db, enum_expr.variant_name(db), enum_expr.payload(db), &enum_ty.variants(db), tydesc_table, value_arena, enum_tydesc)
        }
        (Expr::List(list_expr), Type::List(list_ty)) => {
            let list_tydesc = tydesc_table.get_or_create(ty);
            instantiate_list(db, &list_expr.elements(db), list_ty.element_type(db), tydesc_table, value_arena, list_tydesc)
        }
        _ => bail!("Unsupported expression/type combination for instantiation"),
    }
}

/// Instantiate a tuple value.
fn instantiate_tuple<'db>(
    db: &'db dyn crate::Db,
    elements: &[ExprFull<'db>],
    field_types: &[TypeAndHeap<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    value_arena: &mut ValueArena,
    tuple_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const u8> {
    // Use the provided tuple tydesc and compute layout.
    let layout = unsafe { rtdt::layout::compute_tuple_layout(tuple_tydesc) };

    // Allocate space for tuple.
    let tuple_ptr = value_arena.alloc(layout.size as usize, layout.align as usize);

    // Instantiate and write each field.
    for (i, (elem, field_ty)) in elements.iter().zip(field_types.iter()).enumerate() {
        let field_value = instantiate_expr(db, *elem, field_ty.ty(db), tydesc_table, value_arena)?;
        let field_offset = layout.field_offsets[i];
        let field_size = unsafe { (*tydesc_table.get_or_create(field_ty.ty(db))).size };

        unsafe {
            std::ptr::copy_nonoverlapping(
                field_value,
                tuple_ptr.add(field_offset as usize),
                field_size as usize,
            );
        }
    }

    Ok(tuple_ptr as *const u8)
}

/// Instantiate a struct value.
fn instantiate_struct<'db>(
    db: &'db dyn crate::Db,
    expr_fields: &[ExprStructField<'db>],
    type_fields: &[TypeNamedField<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    value_arena: &mut ValueArena,
    struct_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const u8> {
    // Use the provided struct tydesc and compute layout.
    let layout = unsafe { rtdt::layout::compute_struct_layout(struct_tydesc) };

    // Allocate space for struct.
    let struct_ptr = value_arena.alloc(layout.size as usize, layout.align as usize);

    // Create a map of field names to expr values for lookup.
    let mut field_map: std::collections::HashMap<&str, ExprFull<'db>> = std::collections::HashMap::new();
    for expr_field in expr_fields {
        let name = expr_field.name(db).as_str(db);
        field_map.insert(name, expr_field.value(db));
    }

    // Instantiate and write each field according to the type's field order.
    for (i, type_field) in type_fields.iter().enumerate() {
        let field_name = type_field.name(db).as_str(db);
        let field_ty = type_field.ty(db);

        // Find the corresponding expression field.
        let field_expr = field_map.get(field_name)
            .ok_or_else(|| anyhow!("Missing field: {}", field_name))?;

        let field_value = instantiate_expr(db, *field_expr, field_ty.ty(db), tydesc_table, value_arena)?;
        let field_offset = layout.field_offsets[i];
        let field_size = unsafe { (*tydesc_table.get_or_create(field_ty.ty(db))).size };

        unsafe {
            std::ptr::copy_nonoverlapping(
                field_value,
                struct_ptr.add(field_offset as usize),
                field_size as usize,
            );
        }
    }

    Ok(struct_ptr as *const u8)
}

/// Instantiate an enum value.
fn instantiate_enum<'db>(
    db: &'db dyn crate::Db,
    variant_name: InternedText<'db>,
    payload_expr: Option<ExprFull<'db>>,
    type_variants: &[TypeEnumVariant<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    value_arena: &mut ValueArena,
    enum_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const u8> {
    // Find the variant index and type.
    let variant_name_str = variant_name.as_str(db);
    let (variant_index, variant_ty) = type_variants
        .iter()
        .enumerate()
        .find(|(_, v)| v.name(db).as_str(db) == variant_name_str)
        .ok_or_else(|| anyhow!("Variant not found: {}", variant_name_str))?;

    // Get layout.
    let layout = unsafe { rtdt::layout::compute_enum_layout(enum_tydesc) };

    // IMPORTANT: Instantiate payload BEFORE allocating enum memory,
    // and copy its data immediately to avoid pointer invalidation issues.
    let payload_bytes = if let (Some(payload_expr), Some(payload_ty)) = (payload_expr, variant_ty.payload(db)) {
        let payload_value = instantiate_expr(db, payload_expr, payload_ty.ty(db), tydesc_table, value_arena)?;
        let payload_size = unsafe { (*tydesc_table.get_or_create(payload_ty.ty(db))).size } as usize;

        // Copy payload data to a temporary buffer before allocating enum space.
        let mut bytes = vec![0u8; payload_size];
        unsafe {
            std::ptr::copy_nonoverlapping(
                payload_value,
                bytes.as_mut_ptr(),
                payload_size,
            );
        }
        Some(bytes)
    } else {
        None
    };

    // Now allocate space for enum (after all nested allocations are done).
    let enum_ptr = value_arena.alloc(layout.size as usize, layout.align as usize);

    // Write discriminant (u32).
    unsafe {
        *(enum_ptr as *mut u32) = variant_index as u32;
    }

    // Write payload if present.
    if let Some(payload_bytes) = payload_bytes {
        let payload_offset = layout.variant_offsets[variant_index];
        unsafe {
            std::ptr::copy_nonoverlapping(
                payload_bytes.as_ptr(),
                enum_ptr.add(payload_offset as usize),
                payload_bytes.len(),
            );
        }
    }

    Ok(enum_ptr as *const u8)
}

/// Instantiate a list value.
///
/// Note: This implementation has a limitation where arena reallocation can
/// invalidate pointers. For types containing pointers (like String), we need
/// to ensure the arena doesn't resize during element instantiation.
fn instantiate_list<'db>(
    db: &'db dyn crate::Db,
    elements: &[ExprFull<'db>],
    element_type: TypeAndHeap<'db>,
    tydesc_table: &mut TyDescTable<'db>,
    value_arena: &mut ValueArena,
    list_tydesc: *const rtdt::TyDesc,
) -> AnyResult<*const u8> {
    let element_ty = element_type.ty(db);
    let element_tydesc = tydesc_table.get_or_create(element_ty);
    let element_size = unsafe { (*element_tydesc).size } as usize;
    let element_align = unsafe { (*element_tydesc).align } as usize;

    // Allocate array for element data if not empty.
    let data_ptr = if !elements.is_empty() {
        let total_size = element_size * elements.len();

        // Reserve space for the array upfront to minimize arena resizes.
        // This helps avoid pointer invalidation during element instantiation.
        let array_ptr = value_arena.alloc(total_size, element_align);

        // Instantiate each element and copy it into the array.
        // Note: If element instantiation causes arena reallocation, array_ptr
        // could be invalidated. This is a known limitation that will be
        // addressed when we implement a proper bump allocator.
        for (i, elem) in elements.iter().enumerate() {
            let elem_value = instantiate_expr(db, *elem, element_ty, tydesc_table, value_arena)?;

            unsafe {
                std::ptr::copy_nonoverlapping(
                    elem_value,
                    array_ptr.add(i * element_size),
                    element_size,
                );
            }
        }
        array_ptr as *const u8
    } else {
        std::ptr::null()
    };

    // Now allocate space for List struct.
    let list_size = std::mem::size_of::<rtdt::List>();
    let list_align = std::mem::align_of::<rtdt::List>();
    let list_ptr = value_arena.alloc(list_size, list_align) as *mut rtdt::List;

    unsafe {
        (*list_ptr).data = data_ptr;
        (*list_ptr).size = elements.len() as u32;
        (*list_ptr).capacity = elements.len() as u32;
    }

    Ok(list_ptr as *const u8)
}

/// Instantiate a value from a typechecked AST.
pub fn instantiate_value<'db>(
    db: &'db dyn crate::Db,
    typechecked: TypecheckResult<'db>,
) -> AnyResult<(TyDescTable<'db>, ValueArena, InstantiatedValue<'static>)> {
    let root_type = typechecked.root_type(db)
        .ok_or_else(|| anyhow!("No root type"))?;
    let root_expr = typechecked.root_expr(db);

    let mut tydesc_table = TyDescTable::new(db);
    let mut value_arena = ValueArena::new();

    // Build TyDesc for root type.
    let tydesc = tydesc_table.get_or_create(root_type.ty(db));

    // Instantiate value.
    let value_ptr = instantiate_expr(db, root_expr, root_type.ty(db), &mut tydesc_table, &mut value_arena)?;

    Ok((
        tydesc_table,
        value_arena,
        InstantiatedValue {
            value: value_ptr,
            tydesc,
            _phantom: std::marker::PhantomData,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn compile<'db>(db: &'db Database, source_text: &str) -> AnyResult<TypecheckResult<'db>> {
        let source = bct::input::Source::new(db, source_text.to_string());
        let parsed = crate::parser::parse(db, source);
        let resolved = crate::resolve::resolve_names(db, parsed);
        let typechecked = crate::tycheck::type_check(db, parsed, resolved);
        Ok(typechecked)
    }

    #[test]
    fn test_instantiate_bool_true() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@true")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Bool);
            assert_eq!(*inst.value, 1);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_bool_false() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@false")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Bool);
            assert_eq!(*inst.value, 0);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@42")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::U32);
            assert_eq!(*(inst.value as *const u32), 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_f32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@3.14")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::F32);
            assert_eq!(*(inst.value as *const f32), 3.14);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, r#"@"hello""#)?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::String);
            let string = &*(inst.value as *const rtdt::String);
            assert_eq!(string.size, 5);
            assert_eq!(string.capacity, 5);
            let str_slice = std::slice::from_raw_parts(string.data, string.size as usize);
            assert_eq!(str_slice, b"hello");
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_empty_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, r#"@"""#)?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::String);
            let string = &*(inst.value as *const rtdt::String);
            assert_eq!(string.size, 0);
            assert_eq!(string.capacity, 0);
            assert!(string.data.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_tuple_simple() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@(@true, @42)")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*inst.tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let bool_value = *(inst.value.add(fields[0].offset as usize));
            assert_eq!(bool_value, 1);

            let u32_value = *(inst.value.add(fields[1].offset as usize) as *const u32);
            assert_eq!(u32_value, 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_named_tuple() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @tuple Point(@u32, @u32) / @tuple Point(@1, @2)")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Tuple);
            let tuple_info = &(*inst.tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let u32_value_1 = *(inst.value.add(fields[0].offset as usize) as *const u32);
            assert_eq!(u32_value_1, 1);

            let u32_value_2 = *(inst.value.add(fields[1].offset as usize) as *const u32);
            assert_eq!(u32_value_2, 2);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_small() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @int / @42")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Int);
            let int = &*(inst.value as *const rtdt::Int);
            assert_eq!(int.size_and_sign, 1);
            assert_eq!(int.capacity, 1);
            let limbs = std::slice::from_raw_parts(int.data, 1);
            assert_eq!(limbs[0], 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_zero() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @int / @0")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Int);
            let int = &*(inst.value as *const rtdt::Int);
            assert_eq!(int.size_and_sign, 1);
            assert_eq!(int.capacity, 1);
            let limbs = std::slice::from_raw_parts(int.data, 1);
            assert_eq!(limbs[0], 0);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_int_large() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @int / @1234567890123456789")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Int);
            let int = &*(inst.value as *const rtdt::Int);
            assert!(int.size_and_sign > 0);
            let num_limbs = int.size_and_sign as usize;
            let limbs = std::slice::from_raw_parts(int.data, num_limbs);

            // Reconstruct the value to verify.
            let mut reconstructed: u64 = 0;
            for (i, &limb) in limbs.iter().enumerate() {
                reconstructed |= (limb as u64) << (32 * i);
            }
            assert_eq!(reconstructed, 1234567890123456789);
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

        assert_eq!(ptr1, ptr2);
        assert_eq!(table.tydescs.len(), 1);
        Ok(())
    }

    #[test]
    fn test_value_arena_alignment() {
        let mut arena = ValueArena::new();

        let ptr1 = arena.alloc(1, 1);
        let ptr2 = arena.alloc(4, 4);
        let ptr3 = arena.alloc(1, 1);
        let ptr4 = arena.alloc(8, 8);

        assert_eq!(ptr2 as usize % 4, 0);
        assert_eq!(ptr4 as usize % 8, 0);
    }

    #[test]
    fn test_instantiate_anon_struct() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@{x = @1, y = @2}")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*inst.tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(struct_info.fields, 2);

            // Check field names.
            let field0_name = std::slice::from_raw_parts(fields[0].name, fields[0].name_len as usize);
            assert_eq!(field0_name, b"x");

            let field1_name = std::slice::from_raw_parts(fields[1].name, fields[1].name_len as usize);
            assert_eq!(field1_name, b"y");

            // Check field values.
            let x_value = *(inst.value.add(fields[0].offset as usize) as *const u32);
            assert_eq!(x_value, 1);

            let y_value = *(inst.value.add(fields[1].offset as usize) as *const u32);
            assert_eq!(y_value, 2);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_named_struct() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @struct Point {x: @u32, y: @u32} / @struct Point {x = @10, y = @20}")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*inst.tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(struct_info.fields, 2);

            // Check field values.
            let x_value = *(inst.value.add(fields[0].offset as usize) as *const u32);
            assert_eq!(x_value, 10);

            let y_value = *(inst.value.add(fields[1].offset as usize) as *const u32);
            assert_eq!(y_value, 20);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_anon_to_named_struct() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @struct Point {x: @u32, y: @u32} / @{x = @5, y = @15}")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Struct);
            let struct_info = &(*inst.tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            let fields = std::slice::from_raw_parts(struct_info.fields, 2);

            let x_value = *(inst.value.add(fields[0].offset as usize) as *const u32);
            assert_eq!(x_value, 5);

            let y_value = *(inst.value.add(fields[1].offset as usize) as *const u32);
            assert_eq!(y_value, 15);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_no_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*inst.tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            // Check discriminant.
            let discriminant = *(inst.value as *const u32);
            assert_eq!(discriminant, 0); // "Ok" is first variant
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_with_scalar_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*inst.tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            // Check discriminant.
            let discriminant = *(inst.value as *const u32);
            assert_eq!(discriminant, 0); // "Ok" is first variant

            // Check payload value.
            let variants = std::slice::from_raw_parts(enum_info.variants, 2);
            let payload_offset = variants[0].offset;
            let payload_value = *(inst.value.add(payload_offset as usize) as *const u32);
            assert_eq!(payload_value, 42);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_anon_to_named_coercion() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum Status { Ok, Error } / @enum Error")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Enum);

            // Check discriminant.
            let discriminant = *(inst.value as *const u32);
            assert_eq!(discriminant, 1); // "Error" is second variant
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_with_tuple_payload() -> AnyResult<()> {
        let db = Database::default();
        // Test enum variant with tuple payload - using same syntax as 13_anon_enum_with_multi_payload
        let typechecked = compile(&db, ": @enum { Ok(@(@u32, @u32)), Err(@string) } / @enum Ok(@(@10, @20))")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*inst.tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            // Check discriminant.
            let discriminant = *(inst.value as *const u32);
            assert_eq!(discriminant, 0); // "Ok" is first variant

            // Check tuple payload.
            let variants = std::slice::from_raw_parts(enum_info.variants, 2);
            let payload_offset = variants[0].offset;
            let payload_ptr = inst.value.add(payload_offset as usize);

            // The payload is a tuple, get its tydesc.
            let payload_tydesc = variants[0].payload;
            assert!(!payload_tydesc.is_null());
            assert_eq!((*payload_tydesc).type_tag, rtdt::TyTag::Tuple);

            let tuple_info = &(*payload_tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            let tuple_fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let first_value = *(payload_ptr.add(tuple_fields[0].offset as usize) as *const u32);
            let second_value = *(payload_ptr.add(tuple_fields[1].offset as usize) as *const u32);

            assert_eq!(first_value, 10);
            assert_eq!(second_value, 20);
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_enum_with_struct_payload() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, ": @enum { Data(@{x: @u32, y: @u32}), None } / @enum Data(@{x = @5, y = @15})")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::Enum);
            let enum_info = &(*inst.tydesc).type_info.enum_;
            assert_eq!(enum_info.num_variants, 2);

            // Check discriminant.
            let discriminant = *(inst.value as *const u32);
            assert_eq!(discriminant, 0); // "Data" is first variant

            // Check struct payload.
            let variants = std::slice::from_raw_parts(enum_info.variants, 2);
            let payload_offset = variants[0].offset;
            let payload_ptr = inst.value.add(payload_offset as usize);

            // The payload is a struct, get its tydesc.
            let payload_tydesc = variants[0].payload;
            assert!(!payload_tydesc.is_null());
            assert_eq!((*payload_tydesc).type_tag, rtdt::TyTag::Struct);

            let struct_info = &(*payload_tydesc).type_info.struct_;
            assert_eq!(struct_info.num_fields, 2);

            let struct_fields = std::slice::from_raw_parts(struct_info.fields, 2);
            let x_value = *(payload_ptr.add(struct_fields[0].offset as usize) as *const u32);
            let y_value = *(payload_ptr.add(struct_fields[1].offset as usize) as *const u32);

            assert_eq!(x_value, 5);
            assert_eq!(y_value, 15);
        }
        Ok(())
    }

    #[test]
    #[ignore] // TODO: Fix arena reallocation issue - array_ptr gets invalidated
    fn test_instantiate_list_u32() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@[@1, @2, @3, @4, @5]")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::List);
            let list = &*(inst.value as *const rtdt::List);
            assert_eq!(list.size, 5);
            assert_eq!(list.capacity, 5);

            // Check list elements.
            let element_tydesc = (*inst.tydesc).type_info.list.element_tydesc;
            assert!(!element_tydesc.is_null());
            assert_eq!((*element_tydesc).type_tag, rtdt::TyTag::U32);

            let elements = std::slice::from_raw_parts(list.data as *const u32, list.size as usize);
            assert_eq!(elements, &[1, 2, 3, 4, 5]);
        }
        Ok(())
    }

    #[test]
    #[ignore] // TODO: Requires parser support for list type hint syntax
    fn test_instantiate_empty_list() -> AnyResult<()> {
        let db = Database::default();
        // Empty lists need type hint.
        let typechecked = compile(&db, ": @list [@u32] / @[]")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::List);
            let list = &*(inst.value as *const rtdt::List);
            assert_eq!(list.size, 0);
            assert_eq!(list.capacity, 0);
            assert!(list.data.is_null());
        }
        Ok(())
    }

    #[test]
    fn test_instantiate_list_string() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, r#"@[@"hello", @"world", @"test"]"#)?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::List);
            let list = &*(inst.value as *const rtdt::List);
            assert_eq!(list.size, 3);
            assert_eq!(list.capacity, 3);

            // Check element type.
            let element_tydesc = (*inst.tydesc).type_info.list.element_tydesc;
            assert!(!element_tydesc.is_null());
            assert_eq!((*element_tydesc).type_tag, rtdt::TyTag::String);

            // Check each string element.
            let element_size = (*element_tydesc).size as usize;
            let strings = std::slice::from_raw_parts(list.data as *const rtdt::String, list.size as usize);

            assert_eq!(strings[0].size, 5);
            let str0 = std::slice::from_raw_parts(strings[0].data, strings[0].size as usize);
            assert_eq!(str0, b"hello");

            assert_eq!(strings[1].size, 5);
            let str1 = std::slice::from_raw_parts(strings[1].data, strings[1].size as usize);
            assert_eq!(str1, b"world");

            assert_eq!(strings[2].size, 4);
            let str2 = std::slice::from_raw_parts(strings[2].data, strings[2].size as usize);
            assert_eq!(str2, b"test");
        }
        Ok(())
    }

    #[test]
    #[ignore] // TODO: Fix arena reallocation issue for complex nested types
    fn test_instantiate_list_of_tuples() -> AnyResult<()> {
        let db = Database::default();
        let typechecked = compile(&db, "@[@(@1, @2), @(@3, @4), @(@5, @6)]")?;
        let (tydesc_table, value_arena, inst) = instantiate_value(&db, typechecked)?;

        unsafe {
            assert_eq!((*inst.tydesc).type_tag, rtdt::TyTag::List);
            let list = &*(inst.value as *const rtdt::List);
            assert_eq!(list.size, 3);

            // Check element type is tuple.
            let element_tydesc = (*inst.tydesc).type_info.list.element_tydesc;
            assert!(!element_tydesc.is_null());
            assert_eq!((*element_tydesc).type_tag, rtdt::TyTag::Tuple);

            let tuple_info = &(*element_tydesc).type_info.tuple;
            assert_eq!(tuple_info.num_fields, 2);

            // Get tuple field offsets.
            let tuple_fields = std::slice::from_raw_parts(tuple_info.fields, 2);
            let tuple_size = (*element_tydesc).size as usize;

            // Check each tuple.
            for i in 0..3 {
                let tuple_ptr = list.data.add(i * tuple_size);
                let first = *(tuple_ptr.add(tuple_fields[0].offset as usize) as *const u32);
                let second = *(tuple_ptr.add(tuple_fields[1].offset as usize) as *const u32);

                assert_eq!(first, (i * 2 + 1) as u32);
                assert_eq!(second, (i * 2 + 2) as u32);
            }
        }
        Ok(())
    }
}
