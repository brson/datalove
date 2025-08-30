//! Datalove runtime data types.

#[repr(transparent)]
pub struct Nil(());

#[repr(transparent)]
pub struct Bool(pub u8);

#[repr(transparent)]
pub struct U32(pub u32);

#[repr(transparent)]
pub struct F32(pub f32);

#[repr(C)]
pub struct String {
    pub data: *const u8,
    pub size: u32,
    pub capacity: u32,
}

#[repr(C)]
pub struct List {
    pub data: *const u8, // type-aligned
    pub size: u32, // in elements,
    pub capacity: u32, // in elements,
}

#[repr(C)]
pub struct TyDesc {
    pub type_tag: TyTag,
    pub flags: u8,
}

#[repr(u8)]
pub enum TyTag {
    Nil,
    Bool,
    U32,
    F32,
    Struct,
    Tuple,
    Enum,
    Token,
    Map,
    Set,
    Option,
    Result,
    AnyData,
    Error,
}

// This is optimized for simplicity and interoperability,
// not performance.
#[repr(C)]
pub struct AnyData {
    pub data: usize,
    pub tydesc: *const TyDesc,
}
