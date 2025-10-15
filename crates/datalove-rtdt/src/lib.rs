//! Datalove runtime data types.

pub mod layout;
pub mod anypack;



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

/// B-tree order parameter for Map nodes.
///
/// Following Rust's BTreeMap design with B = 6.
pub const MAP_NODE_B: u32 = 6;

/// Node capacity for Map nodes.
///
/// All Map nodes use this fixed capacity: CAPACITY = 2 * B - 1 = 11.
/// Nodes allocate space for CAPACITY elements but only use `len` elements.
pub const MAP_NODE_CAPACITY: u32 = 2 * MAP_NODE_B - 1;

/// B+tree node with fixed-capacity variable-sized data following the fixed header.
///
/// Memory layout depends on node type:
/// - Internal nodes: CAPACITY keys → CAPACITY+1 child pointers
/// - Leaf nodes: CAPACITY keys → CAPACITY values (matching key-value pairs)
///
/// Arrays are not interleaved for cache locality and alignment efficiency.
/// All nodes use MAP_NODE_CAPACITY = 11 elements.
#[repr(C)]
pub struct MapNode {
    pub tag: MapNodeTag,
    pub len: u32,
    // Variable-sized data follows (use compute_map_*_node_layout to determine offsets):
    // Internal: padding, keys[CAPACITY], padding, child_ptrs[CAPACITY+1]
    // Leaf: next_leaf (*const MapNode), padding, keys[CAPACITY], padding, values[CAPACITY]
}

#[repr(u8)]
pub enum MapNodeTag {
    Internal = 1,
    Leaf = 2,
}

#[repr(C)]
pub struct Set {
    pub root: *const SetNode,
    pub len: u32,
}

/// B-tree order parameter for Set nodes.
///
/// Following Rust's BTreeMap design with B = 6.
pub const SET_NODE_B: u32 = 6;

/// Node capacity for Set nodes.
///
/// All Set nodes use this fixed capacity: CAPACITY = 2 * B - 1 = 11.
/// Nodes allocate space for CAPACITY elements but only use `len` elements.
pub const SET_NODE_CAPACITY: u32 = 2 * SET_NODE_B - 1;

/// B+tree node with fixed-capacity variable-sized data following the fixed header.
///
/// Memory layout depends on node type:
/// - Internal nodes: CAPACITY keys → CAPACITY+1 child pointers
/// - Leaf nodes: CAPACITY keys (set elements stored as keys)
///
/// Arrays are not interleaved for cache locality and alignment efficiency.
/// All nodes use SET_NODE_CAPACITY = 11 elements.
#[repr(C)]
pub struct SetNode {
    pub tag: SetNodeTag,
    pub len: u32,
    // Variable-sized data follows (use compute_set_*_node_layout to determine offsets):
    // Internal: padding, keys[CAPACITY], padding, child_ptrs[CAPACITY+1]
    // Leaf: next_leaf (*const SetNode), padding, keys[CAPACITY]
}

#[repr(u8)]
pub enum SetNodeTag {
    Internal = 1,
    Leaf = 2,
}

/// Computed layout information for a Map internal node.
pub struct MapNodeInternalLayout {
    pub size: u32,
    pub align: u32,
    pub keys_offset: u32,
    pub child_ptrs_offset: u32,
}

/// Computed layout information for a Map leaf node.
pub struct MapNodeLeafLayout {
    pub size: u32,
    pub align: u32,
    pub next_leaf_offset: u32,
    pub keys_offset: u32,
    pub values_offset: u32,
}

/// Computed layout information for a Set internal node.
pub struct SetNodeInternalLayout {
    pub size: u32,
    pub align: u32,
    pub keys_offset: u32,
    pub child_ptrs_offset: u32,
}

/// Computed layout information for a Set leaf node.
pub struct SetNodeLeafLayout {
    pub size: u32,
    pub align: u32,
    pub next_leaf_offset: u32,
    pub keys_offset: u32,
}




// ## Special generic containers

// Option<T> has a tag (u8) followed by optional payload.
// Memory layout: tag (u8) + padding + payload of type T.
// - None (tag=1): no payload
// - Some (tag=2): payload of type T at offset align_up(1, align(T))
#[repr(C)]
pub struct Option /* <T> */ {
    pub tag: OptionTag,
    // Variable-sized data follows:
    // Some: value of type T at computed offset
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OptionTag {
    None = 1,
    Some = 2,
}

// Result<T> has a tag (u8) followed by payload space for max(T, Error).
// Memory layout: tag (u8) + padding + max(sizeof(T), sizeof(Error)) payload.
// - Ok (tag=1): payload contains value of type T at offset align_up(1, max(align(T), align(Error)))
// - Err (tag=2): payload contains Error at offset align_up(1, max(align(T), align(Error)))
#[repr(C)]
pub struct Result /* <T> */ {
    pub tag: ResultTag,
    // Variable-sized data follows:
    // Ok: value of type T at computed offset
    // Err: Error value at computed offset
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ResultTag {
    Ok = 1,
    Err = 2,
}

/// Computed layout information for ?T.
pub struct OptionLayout {
    pub size: u32,
    pub align: u32,
    pub tag_size: u32,
    pub payload_offset: u32,
}

/// Computed layout information for a !T. 
pub struct ResultLayout {
    pub size: u32,
    pub align: u32,
    pub tag_size: u32,
    pub payload_offset: u32,
}




// ## Dynamic types

#[repr(C)]
pub struct Data {
    // Encoding tbd.
    pub data: usize,
    pub tydesc: *const TyDesc,
}

#[repr(C)]
pub struct Error {
    // Encoding tbd.
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
    Bool = 0x01,

    U8 = 0x10,
    I8 = 0x11,
    U16 = 0x12,
    I16 = 0x13,
    U32 = 0x14,
    I32 = 0x15,
    U64 = 0x16,
    I64 = 0x17,

    F32 = 0x20,
    F64 = 0x21,

    Int = 0x30,

    Tuple = 0x40,
    Struct = 0x41,
    Enum = 0x42,

    List = 0x50,
    String = 0x51,
    Map = 0x52,
    Set = 0x53,

    Option = 0x60,
    Result = 0x61,

    Data = 0x70,
    Error = 0x71,
}

#[repr(C)]
pub union TyInfo {
    // For scalars etc.
    pub nothing: TyInfoNothing,
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
pub struct TyInfoNothing;

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
/// We'll always pass a type descriptor even though it won't always be needed.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ByRefArg {
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
