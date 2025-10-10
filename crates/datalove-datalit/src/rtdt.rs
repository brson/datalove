//! Datalove runtime data types.




// ## Basic scalars

#[repr(transparent)]
pub struct Bool(pub u8);

#[repr(transparent)]
pub struct U32(pub u32);

#[repr(transparent)]
pub struct F32(pub f32);


// ## Bigints

// Similar to GMP and others.
#[repr(C)]
pub struct Int {
    // "limbs"
    pub data: *const u32,
    // abs(size_and_sign) == number of limbs;
    // sign(size_and_sign) == sign of self
    pub size_and_sign: i32,
    // Limbs allocated.
    pub capacity: u32,
}








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
    pub root: *const MapNode,
    pub len: u32,
}

// B+tree node with variable-sized data following the fixed header.
#[repr(C)]
pub struct MapNode {
    pub tag: MapNodeTag,
    // Variable-sized data follows:
    // Internal: num_keys (u32), keys[], child_ptrs[]
    // Leaf: num_pairs (u32), next_leaf (*const MapNode), keys[], values[]
}

#[repr(u8)]
pub enum MapNodeTag {
    Internal,
    Leaf,
}

#[repr(C)]
pub struct Set {
    pub root: *const SetNode,
    pub len: u32,
}

// B+tree node with variable-sized data following the fixed header.
#[repr(C)]
pub struct SetNode {
    pub tag: SetNodeTag,
    // Variable-sized data follows:
    // Internal: num_keys (u32), keys[], child_ptrs[]
    // Leaf: num_keys (u32), next_leaf (*const SetNode), keys[]
}

#[repr(u8)]
pub enum SetNodeTag {
    Internal,
    Leaf,
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
    Int,

    Nil,

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

