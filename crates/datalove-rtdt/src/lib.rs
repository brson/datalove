//! Datalove runtime data types.

pub mod layout;
pub mod anypack;
pub mod tydesc_ref;

pub use tydesc_ref::{
    TyDescRef,
    TupleInfo, TupleFieldRef, TupleFieldIter,
    StructInfo, StructFieldRef, StructFieldIter,
    EnumInfo, EnumVariantRef, EnumVariantIter,
    TableColumnRef, TableColumnIter,
};



// ## Basic scalars

#[repr(transparent)]
pub struct Bool(pub u8);

#[repr(transparent)]
pub struct U8(pub u8);

#[repr(transparent)]
pub struct I8(pub i8);

#[repr(transparent)]
pub struct U16(pub u16);

#[repr(transparent)]
pub struct I16(pub i16);

#[repr(transparent)]
pub struct U32(pub u32);

#[repr(transparent)]
pub struct I32(pub i32);

#[repr(transparent)]
pub struct U64(pub u64);

#[repr(transparent)]
pub struct I64(pub i64);

// Collection index types - configurable size.
#[cfg(not(feature = "index-64"))]
mod index_types {
    pub type IndexRepr = u32;
    pub type OffsetRepr = i32;
    pub const INDEX_SIZE: u32 = 4;
    pub const INDEX_ALIGN: u32 = 4;
}
#[cfg(feature = "index-64")]
mod index_types {
    pub type IndexRepr = u64;
    pub type OffsetRepr = i64;
    pub const INDEX_SIZE: u32 = 8;
    pub const INDEX_ALIGN: u32 = 8;
}
pub use index_types::*;


#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Index(pub IndexRepr);

#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Offset(pub OffsetRepr);


impl Index {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);

    #[inline]
    pub const fn new(val: IndexRepr) -> Self {
        Self(val)
    }

    #[inline]
    pub const fn get(self) -> IndexRepr {
        self.0
    }

    #[inline]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl Offset {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);

    #[inline]
    pub const fn new(val: OffsetRepr) -> Self {
        Self(val)
    }

    #[inline]
    pub const fn get(self) -> OffsetRepr {
        self.0
    }

    #[inline]
    pub const fn as_isize(self) -> isize {
        self.0 as isize
    }
}

impl From<IndexRepr> for Index {
    #[inline]
    fn from(val: IndexRepr) -> Self {
        Self(val)
    }
}

impl From<Index> for IndexRepr {
    #[inline]
    fn from(val: Index) -> Self {
        val.0
    }
}

// Only implement From<u32> when IndexRepr != u32 (i.e., when index-64 is enabled).
#[cfg(feature = "index-64")]
impl From<u32> for Index {
    #[inline]
    fn from(val: u32) -> Self {
        Self(val as IndexRepr)
    }
}

impl From<usize> for Index {
    #[inline]
    fn from(val: usize) -> Self {
        Self(val as IndexRepr)
    }
}

impl From<OffsetRepr> for Offset {
    #[inline]
    fn from(val: OffsetRepr) -> Self {
        Self(val)
    }
}

impl From<Offset> for OffsetRepr {
    #[inline]
    fn from(val: Offset) -> Self {
        val.0
    }
}

// Only implement From<i32> when OffsetRepr != i32 (i.e., when index-64 is enabled).
#[cfg(feature = "index-64")]
impl From<i32> for Offset {
    #[inline]
    fn from(val: i32) -> Self {
        Self(val as OffsetRepr)
    }
}

impl From<isize> for Offset {
    #[inline]
    fn from(val: isize) -> Self {
        Self(val as OffsetRepr)
    }
}

impl core::fmt::Debug for Index {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Index({})", self.0)
    }
}

impl core::fmt::Display for Index {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::fmt::Debug for Offset {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Offset({})", self.0)
    }
}

impl core::fmt::Display for Offset {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::ops::Add for Index {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl core::ops::Sub for Index {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl core::ops::Mul for Index {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self(self.0 * rhs.0)
    }
}

impl core::ops::Div for Index {
    type Output = Self;
    #[inline]
    fn div(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }
}

impl core::ops::Rem for Index {
    type Output = Self;
    #[inline]
    fn rem(self, rhs: Self) -> Self {
        Self(self.0 % rhs.0)
    }
}

impl core::ops::AddAssign for Index {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl core::ops::SubAssign for Index {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl core::ops::MulAssign for Index {
    #[inline]
    fn mul_assign(&mut self, rhs: Self) {
        self.0 *= rhs.0;
    }
}

impl core::iter::Sum for Index {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |a, b| a + b)
    }
}

impl core::iter::Product for Index {
    fn product<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ONE, |a, b| a * b)
    }
}

impl core::ops::Add for Offset {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl core::ops::Sub for Offset {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl core::ops::Mul for Offset {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self(self.0 * rhs.0)
    }
}

impl core::ops::Div for Offset {
    type Output = Self;
    #[inline]
    fn div(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }
}

impl core::ops::Rem for Offset {
    type Output = Self;
    #[inline]
    fn rem(self, rhs: Self) -> Self {
        Self(self.0 % rhs.0)
    }
}

impl core::ops::Neg for Offset {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl core::ops::AddAssign for Offset {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl core::ops::SubAssign for Offset {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl core::ops::MulAssign for Offset {
    #[inline]
    fn mul_assign(&mut self, rhs: Self) {
        self.0 *= rhs.0;
    }
}

impl core::iter::Sum for Offset {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |a, b| a + b)
    }
}

impl core::iter::Product for Offset {
    fn product<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ONE, |a, b| a * b)
    }
}

#[repr(transparent)]
pub struct F32(pub f32);

#[repr(transparent)]
pub struct F64(pub f64);


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
    pub capacity: Index,
}

impl Int {
    /// The value written in base ten.
    ///
    /// Reading the limbs is all this needs, so it lives here beside them
    /// rather than in the runtime: a rider formatting an `int` is doing
    /// arithmetic on data it already holds, not asking the runtime for
    /// anything.
    ///
    /// # Safety
    ///
    /// `data` must point to `abs(size_and_sign)` readable limbs.
    pub unsafe fn to_decimal_string(&self) -> std::string::String {
        let abs_size = self.size_and_sign.abs() as usize;
        let is_negative = self.size_and_sign < 0;

        if abs_size == 0 {
            return "0".to_string();
        }

        let limbs = unsafe { std::slice::from_raw_parts(self.data, abs_size) };
        let mut working = limbs.to_vec();

        // Divide by 10^9 until nothing is left, which yields the decimal
        // digits nine at a time, least significant chunk first.
        const DIVISOR: u64 = 1_000_000_000;
        let mut chunks = Vec::new();

        loop {
            let mut remainder: u64 = 0;
            let mut all_zero = true;

            for i in (0..working.len()).rev() {
                let current = (remainder << 32) | (working[i] as u64);
                working[i] = (current / DIVISOR) as u32;
                remainder = current % DIVISOR;

                if working[i] != 0 {
                    all_zero = false;
                }
            }

            chunks.push(remainder as u32);

            if all_zero {
                break;
            }
        }

        let mut result = std::string::String::new();

        if is_negative {
            result.push('-');
        }

        // The most significant chunk is written as it is; the rest carry
        // their leading zeros, being digits in the middle of a number.
        result.push_str(&chunks.last().unwrap().to_string());
        for i in (0..chunks.len() - 1).rev() {
            result.push_str(&format!("{:09}", chunks[i]));
        }

        result
    }
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
    pub size: Index, // in elements,
    pub capacity: Index, // in elements,
}

/// Columnar table storage.
///
/// All column data stored in a single contiguous allocation. Column offsets
/// are computed dynamically from the type descriptors in TyInfoTable.
#[repr(C)]
pub struct Table {
    pub len: Index,
    pub capacity: Index,
    pub data: *const u8,
}

#[repr(C)]
pub struct String {
    pub data: *const u8,
    pub size: Index,
    pub capacity: Index,
}

#[repr(C)]
pub struct Map {
    pub root: *const MapNode,
    pub len: Index,
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
    pub len: Index,
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

/// Heap-allocated strided multidimensional array.
///
/// Tensors store a pointer to the base allocation plus an offset to the view's
/// first element. This design supports linear types: when transforming an owned
/// tensor to a view, the base pointer is preserved for proper deallocation.
///
/// The rank (number of dimensions) is known at compile time via the type descriptor.
/// Shape and strides are heap-allocated arrays of length rank.
#[repr(C)]
pub struct Tensor {
    pub ptr_base: *mut u8,
    pub capacity_elems: Index,
    pub offset_elems: Index,
    // Index x rank
    pub shape: *const Index,
    // Index x rank (can be negative for reversed dimensions, but stored unsigned)
    pub strides: *const Index,
    pub layout: TensorLayout,
}

/// How many elements a tensor of this shape holds: the product of its extents.
///
/// A tensor has at least one axis. An empty tensor keeps its shape, with a
/// zero somewhere in it, so the product is what answers here too.
pub fn tensor_element_count(extents: &[usize]) -> usize {
    assert!(!extents.is_empty(), "a tensor has at least one axis");
    extents.iter().product()
}

/// Memory layout convention for tensors.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TensorLayout {
    RowMajor = 1,
    ColMajor = 2,
    RowMajorTransposed = 3,
    ColMajorTransposed = 4,
}

/// Computed layout information for a Tensor.
pub struct TensorLayoutInfo {
    pub size: u32,
    pub align: u32,
}

/// Range specification for tensor slicing.
///
/// Represents a half-open interval [start, end).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SliceRange {
    pub start: IndexRepr,
    pub end: IndexRepr,
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

// Encoding is described in anypack.
// Fields are pointers to preserve provenance through tagged pointer manipulation.
#[repr(C)]
pub struct Data {
    primary: *const (),
    secondary: *const (),
}

// Encoding is described in anypack.
// Fields are pointers to preserve provenance through tagged pointer manipulation.
#[repr(C)]
pub struct Error {
    primary: *const (),
    secondary: *const (),
}





// ## Type descriptors

#[repr(C)]
pub struct TyDesc {
    pub type_tag: TyTag,
    pub size: u32,
    pub align: u32,
    pub type_info: TyInfo,
}

/// The descriptor for a type that packs into a `data`'s own words.
///
/// A narrow scalar rides in a `data` as a tag and a value, with no descriptor
/// pointer beside it, so a value taken back out of one has none to give. These
/// are the descriptors it would have had. They are constants, not something
/// made up at run time: the size and alignment of `u8` are as fixed as its
/// name.
///
/// `None` for anything else, including the types that pack with their
/// descriptor already in hand.
pub fn packed_tydesc(tag: TyTag) -> core::option::Option<*const TyDesc> {
    // A `TyDesc` holds a union of pointers, so it is not `Sync` and cannot be
    // a plain `static`. These are read-only and never point anywhere, so the
    // wrapper carries the promise rather than each one.
    struct Packed(TyDesc);
    // SAFETY: the union is read as `nothing` for every one of these, which
    // holds no pointer, and nothing writes to them.
    unsafe impl Sync for Packed {}

    macro_rules! desc {
        ($name:ident, $tag:expr, $size:expr) => {{
            static $name: Packed = Packed(TyDesc {
                type_tag: $tag,
                size: $size,
                align: $size,
                type_info: TyInfo { nothing: TyInfoNothing },
            });
            &$name.0 as *const TyDesc
        }};
    }
    core::option::Option::Some(match tag {
        TyTag::Bool => desc!(BOOL, TyTag::Bool, 1),
        TyTag::U8 => desc!(U8, TyTag::U8, 1),
        TyTag::I8 => desc!(I8, TyTag::I8, 1),
        TyTag::U16 => desc!(U16, TyTag::U16, 2),
        TyTag::I16 => desc!(I16, TyTag::I16, 2),
        TyTag::U32 => desc!(U32, TyTag::U32, 4),
        TyTag::I32 => desc!(I32, TyTag::I32, 4),
        TyTag::F32 => desc!(F32, TyTag::F32, 4),
        _ => return core::option::Option::None,
    })
}

/// An operator applied to values whose type only a descriptor says.
///
/// A generic bounded to `float` may add, subtract, multiply, divide and compare
/// its parameter, but which float it is is not known until the call. The
/// operation travels as one of these and the runtime reads the descriptor.
///
/// The numbering is the ABI: every backend passes it and the runtime reads it,
/// so it is written down once here rather than in each of them.
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum DynOp {
    Add = 0,
    Sub = 1,
    Mul = 2,
    Div = 3,
    Eq = 4,
    Ne = 5,
    Lt = 6,
    Le = 7,
    Gt = 8,
    Ge = 9,
}

impl DynOp {
    pub fn from_code(code: u8) -> core::option::Option<DynOp> {
        core::option::Option::Some(match code {
            0 => DynOp::Add,
            1 => DynOp::Sub,
            2 => DynOp::Mul,
            3 => DynOp::Div,
            4 => DynOp::Eq,
            5 => DynOp::Ne,
            6 => DynOp::Lt,
            7 => DynOp::Le,
            8 => DynOp::Gt,
            9 => DynOp::Ge,
            _ => return None,
        })
    }

    /// Whether the result is a bool rather than the operands' own type.
    pub fn gives_bool(&self) -> bool {
        matches!(self, DynOp::Eq | DynOp::Ne | DynOp::Lt | DynOp::Le | DynOp::Gt | DynOp::Ge)
    }
}

/// A constant every fixed-width integer has.
///
/// A literal has to be written at some type, and inside a generic the type is
/// not known where the literal is written. These are asked for by number and
/// made at whatever type the call site's descriptor names.
///
/// Numbered like `DynOp`, and for the same reason: the number is the ABI.
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum DynConst {
    Zero = 0,
    One = 1,
    MinValue = 2,
    MaxValue = 3,
}

impl DynConst {
    pub fn from_code(code: u8) -> core::option::Option<DynConst> {
        core::option::Option::Some(match code {
            0 => DynConst::Zero,
            1 => DynConst::One,
            2 => DynConst::MinValue,
            3 => DynConst::MaxValue,
            _ => return None,
        })
    }
}

/// A constant every float has.
///
/// The same bargain `DynConst` makes for the integers: a literal has to be
/// written at some type, and inside a generic the type is not known where the
/// literal is. These are asked for by number and made at whatever width the
/// call site's descriptor names.
///
/// Numbered like `DynOp`, and for the same reason: the number is the ABI.
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum DynFloatConst {
    Zero = 0,
    One = 1,
    Nan = 2,
    Infinity = 3,
    NegInfinity = 4,
    MinValue = 5,
    MaxValue = 6,
    MinPositive = 7,
    Epsilon = 8,
    Pi = 9,
    E = 10,
}

impl DynFloatConst {
    pub fn from_code(code: u8) -> core::option::Option<DynFloatConst> {
        core::option::Option::Some(match code {
            0 => DynFloatConst::Zero,
            1 => DynFloatConst::One,
            2 => DynFloatConst::Nan,
            3 => DynFloatConst::Infinity,
            4 => DynFloatConst::NegInfinity,
            5 => DynFloatConst::MinValue,
            6 => DynFloatConst::MaxValue,
            7 => DynFloatConst::MinPositive,
            8 => DynFloatConst::Epsilon,
            9 => DynFloatConst::Pi,
            10 => DynFloatConst::E,
            _ => return None,
        })
    }
}

/// A one-operand operation on a value whose type only a descriptor says.
///
/// Numbered like `DynOp`, and for the same reason: the number is the ABI.
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum DynUnOp {
    Abs = 0,
    Sqrt = 1,
    Floor = 2,
    Ceil = 3,
    Round = 4,
    Trunc = 5,
    Fract = 6,
    Recip = 7,
    Signum = 8,
    Neg = 9,
}

impl DynUnOp {
    pub fn from_code(code: u8) -> core::option::Option<DynUnOp> {
        core::option::Option::Some(match code {
            0 => DynUnOp::Abs,
            1 => DynUnOp::Sqrt,
            2 => DynUnOp::Floor,
            3 => DynUnOp::Ceil,
            4 => DynUnOp::Round,
            5 => DynUnOp::Trunc,
            6 => DynUnOp::Fract,
            7 => DynUnOp::Recip,
            8 => DynUnOp::Signum,
            9 => DynUnOp::Neg,
            _ => return core::option::Option::None,
        })
    }
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
    Index = 0x18,
    Offset = 0x19,

    F32 = 0x20,
    F64 = 0x21,

    Int = 0x30,

    Tuple = 0x40,
    Struct = 0x41,
    Enum = 0x42,
    Atom = 0x43,
    Term = 0x44,

    List = 0x50,
    String = 0x51,
    Map = 0x52,
    Set = 0x53,
    Tensor = 0x54,
    Table = 0x55,

    Option = 0x60,
    Result = 0x61,

    Data = 0x70,
    Error = 0x71,
}

impl TyTag {
    /// Convert a raw tag byte, or `None` if it names no type.
    ///
    /// The discriminants are sparse, so most byte values are not tags.
    /// Transmuting one would be undefined behaviour.
    pub fn from_u8(raw: u8) -> core::option::Option<TyTag> {
        let tag = match raw {
            0x01 => TyTag::Bool,

            0x10 => TyTag::U8,
            0x11 => TyTag::I8,
            0x12 => TyTag::U16,
            0x13 => TyTag::I16,
            0x14 => TyTag::U32,
            0x15 => TyTag::I32,
            0x16 => TyTag::U64,
            0x17 => TyTag::I64,
            0x18 => TyTag::Index,
            0x19 => TyTag::Offset,

            0x20 => TyTag::F32,
            0x21 => TyTag::F64,

            0x30 => TyTag::Int,

            0x40 => TyTag::Tuple,
            0x41 => TyTag::Struct,
            0x42 => TyTag::Enum,
            0x43 => TyTag::Atom,
            0x44 => TyTag::Term,

            0x50 => TyTag::List,
            0x51 => TyTag::String,
            0x52 => TyTag::Map,
            0x53 => TyTag::Set,
            0x54 => TyTag::Tensor,
            0x55 => TyTag::Table,

            0x60 => TyTag::Option,
            0x61 => TyTag::Result,

            0x70 => TyTag::Data,
            0x71 => TyTag::Error,

            _ => return core::option::Option::None,
        };
        core::option::Option::Some(tag)
    }
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
    pub tensor: TyInfoTensor,
    pub table: TyInfoTable,
    pub option: TyInfoOption,
    pub result: TyInfoResult,
    pub atom: TyInfoAtom,
    pub term: TyInfoTerm,
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
pub struct TyInfoTensor {
    pub element_tydesc: *const TyDesc,
    pub rank: u32,
}

/// Type information for Table.
///
/// Column type descriptors determine the data layout. Column names
/// are compile-time only and not stored at runtime.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoTable {
    pub num_columns: u32,
    pub columns: *const TyInfoTableColumn,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoTableColumn {
    pub name: *const u8,
    pub name_len: u32,
    pub tydesc: *const TyDesc,
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

/// Type information for Atom (named zero-sized type).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoAtom {
    pub name: *const u8,
    pub name_len: u32,
}

/// Type information for Term (named wrapper around a payload type).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoTerm {
    pub name: *const u8,
    pub name_len: u32,
    pub payload: *const TyDesc,
}
