//! Safe, immutable type descriptor traversal.
//!
//! This module provides safe wrappers around TyDesc pointers for traversing
//! the type descriptor tree without unsafe operations. From the runtime's
//! perspective, type descriptors are immutable and safe to traverse.

use crate::*;

/// Safe reference wrapper for immutable type descriptor traversal.
///
/// Provides safe access to type descriptor information without requiring
/// unsafe pointer operations. All nested type descriptor pointers are
/// converted to safe references with appropriate lifetimes.
#[derive(Copy, Clone)]
pub struct TyDescRef<'a> {
    inner: &'a TyDesc,
}

impl<'a> TyDescRef<'a> {
    /// Creates a safe reference from a raw pointer.
    ///
    /// # Safety
    /// The pointer must be non-null and point to a valid TyDesc for the
    /// lifetime 'a. The TyDesc and all nested type descriptors it references
    /// must remain valid for 'a.
    pub unsafe fn from_ptr(ptr: *const TyDesc) -> Self {
        debug_assert!(!ptr.is_null());
        unsafe {
            TyDescRef {
                inner: &*ptr,
            }
        }
    }

    /// Creates a safe reference from a Rust reference.
    pub fn from_ref(td: &'a TyDesc) -> Self {
        TyDescRef { inner: td }
    }

    /// Returns the type tag.
    pub fn type_tag(&self) -> TyTag {
        self.inner.type_tag
    }

    /// Returns the size in bytes.
    pub fn size(&self) -> u32 {
        self.inner.size
    }

    /// Returns the alignment in bytes.
    pub fn align(&self) -> u32 {
        self.inner.align
    }

    /// Returns the underlying raw pointer.
    pub fn as_ptr(&self) -> *const TyDesc {
        self.inner as *const TyDesc
    }

    /// Returns a reference to the underlying TyDesc.
    pub fn as_ref(&self) -> &'a TyDesc {
        self.inner
    }

    // Tuple accessors.

    /// Returns the tuple field information.
    ///
    /// # Panics
    /// Panics if this is not a Tuple type.
    pub fn tuple_info(&self) -> TupleInfo<'a> {
        assert_eq!(self.inner.type_tag, TyTag::Tuple);
        unsafe {
            let info = self.inner.type_info.tuple;
            let fields = std::slice::from_raw_parts(info.fields, info.num_fields as usize);
            TupleInfo { fields }
        }
    }

    /// Returns an iterator over tuple fields.
    ///
    /// # Panics
    /// Panics if this is not a Tuple type.
    pub fn iter_tuple_fields(&self) -> TupleFieldIter<'a> {
        let info = self.tuple_info();
        TupleFieldIter {
            fields: info.fields,
            index: 0,
        }
    }

    // Struct accessors.

    /// Returns the struct field information.
    ///
    /// # Panics
    /// Panics if this is not a Struct type.
    pub fn struct_info(&self) -> StructInfo<'a> {
        assert_eq!(self.inner.type_tag, TyTag::Struct);
        unsafe {
            let info = self.inner.type_info.struct_;
            let fields = std::slice::from_raw_parts(info.fields, info.num_fields as usize);
            StructInfo { fields }
        }
    }

    /// Returns an iterator over struct fields.
    ///
    /// # Panics
    /// Panics if this is not a Struct type.
    pub fn iter_struct_fields(&self) -> StructFieldIter<'a> {
        let info = self.struct_info();
        StructFieldIter {
            fields: info.fields,
            index: 0,
        }
    }

    // Enum accessors.

    /// Returns the enum variant information.
    ///
    /// # Panics
    /// Panics if this is not an Enum type.
    pub fn enum_info(&self) -> EnumInfo<'a> {
        assert_eq!(self.inner.type_tag, TyTag::Enum);
        unsafe {
            let info = self.inner.type_info.enum_;
            let variants = std::slice::from_raw_parts(info.variants, info.num_variants as usize);
            EnumInfo { variants }
        }
    }

    /// Returns an iterator over enum variants.
    ///
    /// # Panics
    /// Panics if this is not an Enum type.
    pub fn iter_enum_variants(&self) -> EnumVariantIter<'a> {
        let info = self.enum_info();
        EnumVariantIter {
            variants: info.variants,
            index: 0,
        }
    }

    // Collection accessors.

    /// Returns the element type descriptor for a List type.
    ///
    /// # Panics
    /// Panics if this is not a List type.
    pub fn list_element_ty(&self) -> TyDescRef<'a> {
        assert_eq!(self.inner.type_tag, TyTag::List);
        unsafe {
            let info = self.inner.type_info.list;
            TyDescRef::from_ptr(info.element_tydesc)
        }
    }

    /// Returns the key type descriptor for a Map type.
    ///
    /// # Panics
    /// Panics if this is not a Map type.
    pub fn map_key_ty(&self) -> TyDescRef<'a> {
        assert_eq!(self.inner.type_tag, TyTag::Map);
        unsafe {
            let info = self.inner.type_info.map;
            TyDescRef::from_ptr(info.key_tydesc)
        }
    }

    /// Returns the value type descriptor for a Map type.
    ///
    /// # Panics
    /// Panics if this is not a Map type.
    pub fn map_value_ty(&self) -> TyDescRef<'a> {
        assert_eq!(self.inner.type_tag, TyTag::Map);
        unsafe {
            let info = self.inner.type_info.map;
            TyDescRef::from_ptr(info.value_tydesc)
        }
    }

    /// Returns the element type descriptor for a Set type.
    ///
    /// # Panics
    /// Panics if this is not a Set type.
    pub fn set_element_ty(&self) -> TyDescRef<'a> {
        assert_eq!(self.inner.type_tag, TyTag::Set);
        unsafe {
            let info = self.inner.type_info.set;
            TyDescRef::from_ptr(info.element_tydesc)
        }
    }

    /// Returns the element type descriptor for a Tensor type.
    ///
    /// # Panics
    /// Panics if this is not a Tensor type.
    pub fn tensor_element_ty(&self) -> TyDescRef<'a> {
        assert_eq!(self.inner.type_tag, TyTag::Tensor);
        unsafe {
            let info = self.inner.type_info.tensor;
            TyDescRef::from_ptr(info.element_tydesc)
        }
    }

    /// Returns the rank (number of dimensions) for a Tensor type.
    ///
    /// # Panics
    /// Panics if this is not a Tensor type.
    pub fn tensor_rank(&self) -> u32 {
        assert_eq!(self.inner.type_tag, TyTag::Tensor);
        unsafe {
            let info = self.inner.type_info.tensor;
            info.rank
        }
    }

    /// Returns the number of columns for a Table type.
    ///
    /// # Panics
    /// Panics if this is not a Table type.
    pub fn table_num_columns(&self) -> u32 {
        assert_eq!(self.inner.type_tag, TyTag::Table);
        unsafe { self.inner.type_info.table.num_columns }
    }

    /// Returns an iterator over column info for a Table type.
    ///
    /// # Panics
    /// Panics if this is not a Table type.
    pub fn table_column_tydescs(&self) -> TableColumnIter<'a> {
        assert_eq!(self.inner.type_tag, TyTag::Table);
        let info = unsafe { &self.inner.type_info.table };
        TableColumnIter {
            ptr: info.columns,
            remaining: info.num_columns,
            _marker: std::marker::PhantomData,
        }
    }

    // Generic container accessors.

    /// Returns the inner type descriptor for an Option type.
    ///
    /// # Panics
    /// Panics if this is not an Option type.
    pub fn option_inner_ty(&self) -> TyDescRef<'a> {
        assert_eq!(self.inner.type_tag, TyTag::Option);
        unsafe {
            let info = self.inner.type_info.option;
            TyDescRef::from_ptr(info.inner_tydesc)
        }
    }

    /// Returns the ok type descriptor for a Result type.
    ///
    /// # Panics
    /// Panics if this is not a Result type.
    pub fn result_ok_ty(&self) -> TyDescRef<'a> {
        assert_eq!(self.inner.type_tag, TyTag::Result);
        unsafe {
            let info = self.inner.type_info.result;
            TyDescRef::from_ptr(info.ok_tydesc)
        }
    }
}

impl<'a> From<&'a TyDesc> for TyDescRef<'a> {
    fn from(td: &'a TyDesc) -> Self {
        TyDescRef::from_ref(td)
    }
}

// Tuple field wrappers.

/// Information about tuple fields.
pub struct TupleInfo<'a> {
    fields: &'a [TyInfoTupleField],
}

impl<'a> TupleInfo<'a> {
    /// Returns the number of fields.
    pub fn num_fields(&self) -> u32 {
        self.fields.len() as u32
    }

    /// Returns a slice of raw field information.
    pub fn fields(&self) -> &'a [TyInfoTupleField] {
        self.fields
    }

    /// Returns a specific field by index.
    pub fn field(&self, index: usize) -> core::option::Option<TupleFieldRef<'a>> {
        self.fields.get(index).map(|f| {
            unsafe {
                TupleFieldRef {
                    offset: f.offset,
                    tydesc: TyDescRef::from_ptr(f.tydesc),
                }
            }
        })
    }
}

/// Safe reference to a tuple field.
#[derive(Copy, Clone)]
pub struct TupleFieldRef<'a> {
    offset: u32,
    tydesc: TyDescRef<'a>,
}

impl<'a> TupleFieldRef<'a> {
    /// Returns the field offset in bytes.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// Returns the field's type descriptor.
    pub fn tydesc(&self) -> TyDescRef<'a> {
        self.tydesc
    }
}

/// Iterator over tuple fields.
pub struct TupleFieldIter<'a> {
    fields: &'a [TyInfoTupleField],
    index: usize,
}

impl<'a> Iterator for TupleFieldIter<'a> {
    type Item = TupleFieldRef<'a>;

    fn next(&mut self) -> core::option::Option<Self::Item> {
        if self.index < self.fields.len() {
            let field = &self.fields[self.index];
            self.index += 1;
            unsafe {
                core::option::Option::Some(TupleFieldRef {
                    offset: field.offset,
                    tydesc: TyDescRef::from_ptr(field.tydesc),
                })
            }
        } else {
            core::option::Option::None
        }
    }
}

// Struct field wrappers.

/// Information about struct fields.
pub struct StructInfo<'a> {
    fields: &'a [TyInfoStructField],
}

impl<'a> StructInfo<'a> {
    /// Returns the number of fields.
    pub fn num_fields(&self) -> u32 {
        self.fields.len() as u32
    }

    /// Returns a slice of raw field information.
    pub fn fields(&self) -> &'a [TyInfoStructField] {
        self.fields
    }

    /// Returns a specific field by index.
    pub fn field(&self, index: usize) -> core::option::Option<StructFieldRef<'a>> {
        self.fields.get(index).map(|f| {
            unsafe {
                let name_bytes = std::slice::from_raw_parts(f.name, f.name_len as usize);
                let name = std::str::from_utf8_unchecked(name_bytes);
                StructFieldRef {
                    name,
                    offset: f.offset,
                    tydesc: TyDescRef::from_ptr(f.tydesc),
                }
            }
        })
    }
}

/// Safe reference to a struct field.
#[derive(Copy, Clone)]
pub struct StructFieldRef<'a> {
    name: &'a str,
    offset: u32,
    tydesc: TyDescRef<'a>,
}

impl<'a> StructFieldRef<'a> {
    /// Returns the field name.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns the field offset in bytes.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// Returns the field's type descriptor.
    pub fn tydesc(&self) -> TyDescRef<'a> {
        self.tydesc
    }
}

/// Iterator over struct fields.
pub struct StructFieldIter<'a> {
    fields: &'a [TyInfoStructField],
    index: usize,
}

impl<'a> Iterator for StructFieldIter<'a> {
    type Item = StructFieldRef<'a>;

    fn next(&mut self) -> core::option::Option<Self::Item> {
        if self.index < self.fields.len() {
            let field = &self.fields[self.index];
            self.index += 1;
            unsafe {
                let name_bytes = std::slice::from_raw_parts(field.name, field.name_len as usize);
                let name = std::str::from_utf8_unchecked(name_bytes);
                core::option::Option::Some(StructFieldRef {
                    name,
                    offset: field.offset,
                    tydesc: TyDescRef::from_ptr(field.tydesc),
                })
            }
        } else {
            core::option::Option::None
        }
    }
}

// Enum variant wrappers.

/// Information about enum variants.
pub struct EnumInfo<'a> {
    variants: &'a [TyInfoEnumVariant],
}

impl<'a> EnumInfo<'a> {
    /// Returns the number of variants.
    pub fn num_variants(&self) -> u32 {
        self.variants.len() as u32
    }

    /// Returns a slice of raw variant information.
    pub fn variants(&self) -> &'a [TyInfoEnumVariant] {
        self.variants
    }

    /// Returns a specific variant by index.
    pub fn variant(&self, index: usize) -> core::option::Option<EnumVariantRef<'a>> {
        self.variants.get(index).map(|v| {
            unsafe {
                let name_bytes = std::slice::from_raw_parts(v.name, v.name_len as usize);
                let name = std::str::from_utf8_unchecked(name_bytes);
                let payload = if v.payload.is_null() {
                    core::option::Option::None
                } else {
                    core::option::Option::Some(TyDescRef::from_ptr(v.payload))
                };
                EnumVariantRef {
                    name,
                    offset: v.offset,
                    payload,
                }
            }
        })
    }
}

/// Safe reference to an enum variant.
#[derive(Copy, Clone)]
pub struct EnumVariantRef<'a> {
    name: &'a str,
    offset: u32,
    payload: core::option::Option<TyDescRef<'a>>,
}

impl<'a> EnumVariantRef<'a> {
    /// Returns the variant name.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns the payload offset in bytes.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// Returns the payload type descriptor, if any.
    pub fn payload(&self) -> core::option::Option<TyDescRef<'a>> {
        self.payload
    }
}

/// Iterator over enum variants.
pub struct EnumVariantIter<'a> {
    variants: &'a [TyInfoEnumVariant],
    index: usize,
}

impl<'a> Iterator for EnumVariantIter<'a> {
    type Item = EnumVariantRef<'a>;

    fn next(&mut self) -> core::option::Option<Self::Item> {
        if self.index < self.variants.len() {
            let variant = &self.variants[self.index];
            self.index += 1;
            unsafe {
                let name_bytes = std::slice::from_raw_parts(variant.name, variant.name_len as usize);
                let name = std::str::from_utf8_unchecked(name_bytes);
                let payload = if variant.payload.is_null() {
                    core::option::Option::None
                } else {
                    core::option::Option::Some(TyDescRef::from_ptr(variant.payload))
                };
                core::option::Option::Some(EnumVariantRef {
                    name,
                    offset: variant.offset,
                    payload,
                })
            }
        } else {
            core::option::Option::None
        }
    }
}

/// Safe reference to a table column.
#[derive(Copy, Clone)]
pub struct TableColumnRef<'a> {
    name: &'a str,
    tydesc: TyDescRef<'a>,
}

impl<'a> TableColumnRef<'a> {
    /// Returns the column name.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns the column's type descriptor.
    pub fn tydesc(&self) -> TyDescRef<'a> {
        self.tydesc
    }
}

/// Iterator over columns in a Table.
pub struct TableColumnIter<'a> {
    ptr: *const TyInfoTableColumn,
    remaining: u32,
    _marker: std::marker::PhantomData<&'a TyDesc>,
}

impl<'a> Iterator for TableColumnIter<'a> {
    type Item = TableColumnRef<'a>;

    fn next(&mut self) -> core::option::Option<Self::Item> {
        if self.remaining == 0 {
            return core::option::Option::None;
        }
        unsafe {
            let col = &*self.ptr;
            self.ptr = self.ptr.add(1);
            self.remaining -= 1;
            let name_bytes = std::slice::from_raw_parts(col.name, col.name_len as usize);
            let name = std::str::from_utf8_unchecked(name_bytes);
            core::option::Option::Some(TableColumnRef {
                name,
                tydesc: TyDescRef::from_ptr(col.tydesc),
            })
        }
    }

    fn size_hint(&self) -> (usize, core::option::Option<usize>) {
        let len = self.remaining as usize;
        (len, core::option::Option::Some(len))
    }
}

impl<'a> ExactSizeIterator for TableColumnIter<'a> {}
