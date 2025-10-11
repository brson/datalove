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
}
