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

        // TODO: Implement type descriptor creation.
        unimplemented!("TyDesc creation")
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

/// Instantiate a value from a typechecked AST.
pub fn instantiate_value<'db>(
    db: &'db dyn crate::Db,
    typechecked: TypecheckResult<'db>,
) -> Result<(TyDescTable<'db>, ValueArena, InstantiatedValue<'static>), String> {
    let root_type = typechecked.root_type(db)
        .ok_or_else(|| "No root type".to_string())?;

    let mut tydesc_table = TyDescTable::new(db);
    let mut value_arena = ValueArena::new();

    // TODO: Build TyDesc and instantiate value.
    unimplemented!("Value instantiation")
}
