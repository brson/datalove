//! Instantiate runtime values and type descriptors from typechecked AST.

use rmx::prelude::*;
use std::collections::HashMap;
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

        // Compute layout.
        let layout = unsafe {
            let temp_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::Tuple,
                size: 0,
                align: 1,
                type_info: rtdt::TyInfo {
                    tuple: rtdt::TyInfoTuple {
                        num_fields: fields.len() as u32,
                        fields: field_tydescs.as_ptr() as *const rtdt::TyInfoTupleField,
                    },
                },
            };
            rtdt::layout::compute_tuple_layout(&temp_tydesc)
        };

        // Create field info array.
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
) -> Result<*const u8, String> {
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
            let value: u32 = value_str.parse()
                .map_err(|_| format!("Failed to parse u32: {}", value_str))?;

            let ptr = value_arena.alloc(4, 4) as *mut u32;
            unsafe { *ptr = value };
            Ok(ptr as *const u8)
        }
        (Expr::Float(float_expr), Type::F32) => {
            let value_str = float_expr.value(db).as_str(db);
            let value: f32 = value_str.parse()
                .map_err(|_| format!("Failed to parse f32: {}", value_str))?;

            let ptr = value_arena.alloc(4, 4) as *mut f32;
            unsafe { *ptr = value };
            Ok(ptr as *const u8)
        }
        (Expr::String(string_expr), Type::String) => {
            let value_str = string_expr.value(db).as_str(db);

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
            instantiate_tuple(db, &tuple_expr.elements(db), &tuple_ty.fields(db), tydesc_table, value_arena)
        }
        _ => Err(format!("Unsupported expression/type combination for instantiation")),
    }
}

/// Instantiate a tuple value.
fn instantiate_tuple<'db>(
    db: &'db dyn crate::Db,
    elements: &[ExprFull<'db>],
    field_types: &[TypeAndHeap<'db>],
    tydesc_table: &mut TyDescTable<'db>,
    value_arena: &mut ValueArena,
) -> Result<*const u8, String> {
    // Compute layout.
    let tuple_ty = Type::AnonTuple(TypeAnonTuple::new(db, field_types.to_vec()));
    let tydesc = tydesc_table.get_or_create(&tuple_ty);

    let layout = unsafe { rtdt::layout::compute_tuple_layout(tydesc) };

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

/// Instantiate a value from a typechecked AST.
pub fn instantiate_value<'db>(
    db: &'db dyn crate::Db,
    typechecked: TypecheckResult<'db>,
) -> Result<(TyDescTable<'db>, ValueArena, InstantiatedValue<'static>), String> {
    let root_type = typechecked.root_type(db)
        .ok_or_else(|| "No root type".to_string())?;
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
