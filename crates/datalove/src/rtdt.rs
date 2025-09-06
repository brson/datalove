//! Datalove runtime data types.




// ## Basic scalars

#[repr(transparent)]
pub struct Bool(pub u8);

#[repr(transparent)]
pub struct U32(pub u32);

#[repr(transparent)]
pub struct F32(pub f32);


// ## Bigints

#[repr(C)]
pub struct Int {
    pub payload: usize,
}


// ## Special types

#[repr(transparent)]
pub struct Nil(());




// ## Tuples, structs, and enums

// todo




// ##  Collection types
    
#[repr(C)]
pub struct List {
    pub data: *const u8, // type-aligned
    pub size: u32, // in elements,
    pub capacity: u32, // in elements,
}

#[repr(C)]
pub struct String {
    pub data: *const u8,
    pub size: u32,
    pub capacity: u32,
}

#[repr(C)]
pub struct Map {
    // todo
}

#[repr(C)]
pub struct Set {
    // todo
}




// ## Special generic containers

#[repr(C)]
pub struct Option /* <T> */ {
    pub tag: OptionTag,
    // todo
}

#[repr(u8)]
pub enum OptionTag { None, Some }




// ## Dynamic types

#[repr(C)]
pub struct AnyData {
    pub data: usize,
    pub tydesc: *const TyDesc,
}

#[repr(C)]
pub struct Error {
    pub data: usize,
    pub tydesc: *const TyDesc,
}




// ## Type descriptors

#[repr(C)]
pub struct TyDesc {
    pub type_tag: TyTag,
    pub type_info: TyInfo,
}

#[repr(u8)]
pub enum TyTag {
    Bool,
    U32,
    F32,

    Nil,
    Token,

    Tuple,
    Struct,
    Enum,

    List,
    String,
    Map,
    Set,

    Option,
    Result,

    AnyData,
    Error,
}

#[repr(C)]
pub union TyInfo {
    token: (),
    tuple: (),
    struct_: (),
    enum_: (),
    list: (),
    map: (),
    set: (),
    option: TyInfoOption,
    result: TyInfoResult,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoOption {
    tydesc: *const TyDesc,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoResult {
    tydesc: *const TyDesc,
}

