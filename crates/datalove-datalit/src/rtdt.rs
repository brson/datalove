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

/// Computed layout information for a tuple.
pub struct TupleLayout {
    pub size: u32,
    pub align: u32,
    pub field_offsets: Vec<u32>,
}

/// Computed layout information for a struct (same as tuple).
pub struct StructLayout {
    pub size: u32,
    pub align: u32,
    pub field_offsets: Vec<u32>,
}

/// Computed layout information for an enum.
pub struct EnumLayout {
    pub size: u32,
    pub align: u32,
    pub discriminant_size: u32,
    pub payload_offset: u32,
    pub variant_offsets: Vec<u32>,
}




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
    pub size: u32,
    pub align: u32,
    pub type_info: TyInfo,
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum TyTag {
    Bool,
    U32,
    F32,
    Int,

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
    pub tuple: TyInfoTuple,
    pub struct_: TyInfoStruct,
    pub enum_: TyInfoEnum,
    pub list: TyInfoList,
    pub map: TyInfoMap,
    pub set: TyInfoSet,
    pub option: TyInfoOption,
    pub result: TyInfoResult,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoTuple {
    pub num_fields: u32,
    pub fields: *const TyInfoTupleField,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoTupleField {
    pub offset: u32,
    pub tydesc: *const TyDesc,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoStruct {
    pub fields: *const TyInfoStructField,
    pub num_fields: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoStructField {
    pub name: *const u8,
    pub name_len: u32,
    pub offset: u32,
    pub tydesc: *const TyDesc,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoEnum {
    pub variants: *const TyInfoEnumVariant,
    pub num_variants: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoEnumVariant {
    pub name: *const u8,
    pub name_len: u32,
    pub offset: u32,
    pub payload: *const TyDesc,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoList {
    pub element_tydesc: *const TyDesc,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoMap {
    pub key_tydesc: *const TyDesc,
    pub value_tydesc: *const TyDesc,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoSet {
    pub element_tydesc: *const TyDesc,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoOption {
    pub inner_tydesc: *const TyDesc,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoResult {
    pub ok_tydesc: *const TyDesc,
}

/// Simple runtime argument ABI.
///
/// We'll always pass a type descriptor even though it won't always be needed.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ByValArg {
    pub value: *const u8, // type aligned
    pub tydesc: *const TyDesc,
}

/// Simple runtime argument ABI.
///
/// We'll pass return values like arguments for now.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ReturnArg {
    pub value: *mut u8, // type aligned
    pub tydesc: *const TyDesc,
}
