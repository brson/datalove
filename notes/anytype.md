# Anypack Design: Packing Types and Values into @data

This document specifies the encoding scheme for packing Datalove types and values into the dynamic `@data` type.

## Overview

The `@data` type is a dynamic/existential type that can hold any Datalove value with runtime type information. It enables runtime introspection, reflection, and heterogeneous data structures.

**Design Goals:**
- Avoid allocations for common cases (primitives, small values)
- Support full numeric tower (bool, u32, i32, u64, i64, f32, f64, u128, i128, u256, i256, Int)
- Maintain compatibility with existing runtime ABI
- Be comprehensible and debuggable
- Work on all architectures (no pointer truncation)
- Use only standard tagged pointer techniques (3-bit tags)

**Size:** 128 bits (16 bytes) = two 64-bit words

## Core Layout

```rust
#[repr(C)]
pub struct Data {
    primary: u64,    // Type descriptor pointer (possibly tagged) OR immediate value
    secondary: u64,  // Value pointer OR immediate value
}
```

The interpretation of these two words depends on a **tag** stored in the bottom 3 bits of `primary`.

## Tagging Strategy

All heap allocations are 8-byte aligned (standard malloc guarantee), so the bottom 3 bits of pointers are always `0b000`. We repurpose these bits as tags.

```rust
const TAG_MASK: u64 = 0b111;
const PTR_MASK: u64 = !TAG_MASK;

fn tag(primary: u64) -> u8 {
    (primary & TAG_MASK) as u8
}

fn untag_ptr<T>(primary: u64) -> *const T {
    (primary & PTR_MASK) as *const T
}

fn tag_ptr<T>(ptr: *const T, tag: u8) -> u64 {
    debug_assert_eq!(ptr as usize & TAG_MASK as usize, 0);
    debug_assert!(tag < 8);
    (ptr as u64) | (tag as u64)
}
```

## Tag Encodings

### Tag 0 (0b000) - Two Pointers (Default)

**The unoptimized, general case.**

```
primary:   *const TyDesc (untagged, naturally 8-byte aligned)
secondary: *const u8 (pointer to actual value allocation)
```

**Use for:**
- All heap-allocated types: Int, String, List, Map, Set
- Large fixed-size types: u128, i128, u256, i256
- Large composites: tuples, structs, enums that don't fit inline
- Nested Data and Error values

**Example (Int):**
```rust
Data {
    primary: 0x00007f8a4c001000,    // tydesc for Int (untagged, ends in 0b000)
    secondary: 0x00007f8a4c002000,  // pointer to Int struct
}
```

**Advantages:**
- Matches current Data representation exactly
- No wrapper structs needed - points directly to value
- TyDesc can be shared across many Data instances
- Simple and obvious

### Tag 4 (0b100) - TyDesc Pointer + Inline 64-bit Value

**Optimization for types that need tydesc but value fits in 64 bits.**

```
primary:   *const TyDesc (tagged with 0b100)
secondary: 64-bit immediate value (interpretation depends on type)
```

**Use for:**
- **f32** (32 bits in lower part of secondary)
- **f64** (64 bits, full secondary) ← **Important case**
- **u32** (all values, when named or needs tydesc context)
- **i32** (all values, when named or needs tydesc context)
- **u64** (all values)
- **i64** (all values)
- **Small tuples/structs** (if all fields pack into ≤ 64 bits)
- **Small enums** (discriminant + small payload ≤ 64 bits)

**Example (f64):**
```rust
Data {
    primary: 0x00007f8a4c001000 | 0b100,  // tydesc for f64, tagged
    secondary: 0x400921fb54442d18,         // 3.14159265... (IEEE-754 bits)
}
```

**Advantages:**
- No allocation for value
- Still have full type information via tydesc
- f64 fits perfectly (most important 64-bit type)

### Tag 1 (0b001) - Small Immediate (No TyDesc)

**Super-optimized for common primitives that don't need separate tydesc.**

```
primary:   bits 0-2   = 0b001 (tag)
           bits 3-63  = 61-bit immediate value
secondary: bits 0-7   = TyTag enum value
           bits 8-63  = reserved/unused
```

**Use for:**
- **bool** (0 or 1)
- **u32** when ≤ 2^30 (most values fit)
- **i32** when in ±2^30 range (most values fit)
- **Unit enum variants** (just discriminant)

**Example (bool true):**
```rust
Data {
    primary: (1 << 3) | 0b001,  // value=1, tag=1 → 0x0000000000000009
    secondary: TyTag::Bool,     // → 0x0000000000000001
}
```

**Advantages:**
- No heap allocation
- No tydesc pointer dereference
- Minimal memory footprint
- Fast construction and access

**Limitation:**
- Cannot distinguish named bool types (e.g., `struct IsReady(bool)` vs bare `bool`)
- For named types, must use Tag 4 instead

### Reserved Tags

**Tag 2 (0b010)** - Reserved for future use
**Tag 3 (0b011)** - Reserved for future use
**Tag 5 (0b101)** - Reserved (could be for nested Data)
**Tag 6 (0b110)** - Reserved
**Tag 7 (0b111)** - Reserved (could be for NaN-boxing or special values)

## Type-by-Type Encodings

| Type | Size | Tag | Primary | Secondary | Notes |
|------|------|-----|---------|-----------|-------|
| **bool** (anon) | 1 bit | 1 | 0 or 1 (bits 3-63) | TyTag::Bool | No tydesc needed |
| **bool** (named) | 1 bit | 4 | TyDesc* \| 0b100 | 0 or 1 | Need tydesc for name |
| **u32** (≤ 2^30) | 32 bits | 1 | value (bits 3-63) | TyTag::U32 | Most values fit |
| **u32** (> 2^30 or named) | 32 bits | 4 | TyDesc* \| 0b100 | value (32 bits) | Full range |
| **i32** (±2^30) | 32 bits | 1 | value (bits 3-63) | TyTag::I32 | Most values fit |
| **i32** (full or named) | 32 bits | 4 | TyDesc* \| 0b100 | value (32 bits) | Full range |
| **f32** | 32 bits | 4 | TyDesc* \| 0b100 | IEEE-754 bits | Lower 32 bits of secondary |
| **u64** | 64 bits | 4 | TyDesc* \| 0b100 | value | Full inline |
| **i64** | 64 bits | 4 | TyDesc* \| 0b100 | value | Full inline |
| **f64** | 64 bits | 4 | TyDesc* \| 0b100 | IEEE-754 bits | Full inline, important! |
| **u128** | 128 bits | 0 | TyDesc* | *u128 | Heap (16 bytes) |
| **i128** | 128 bits | 0 | TyDesc* | *i128 | Heap (16 bytes) |
| **u256** | 256 bits | 0 | TyDesc* | *u256 | Heap (32 bytes) |
| **i256** | 256 bits | 0 | TyDesc* | *i256 | Heap (32 bytes) |
| **Int** | variable | 0 | TyDesc* | *Int | GMP-style, no inline optimization |
| **String** | variable | 0 | TyDesc* | *String | Heap, points to String struct |
| **List** | variable | 0 | TyDesc* | *List | Heap, points to List struct |
| **Map** | variable | 0 | TyDesc* | *Map | Heap, points to Map struct |
| **Set** | variable | 0 | TyDesc* | *Set | Heap, points to Set struct |
| **Tuple** (≤ 64 bits) | variable | 4 | TyDesc* \| 0b100 | packed fields | Small tuple optimization |
| **Tuple** (> 64 bits) | variable | 0 | TyDesc* | *tuple | Heap allocation |
| **Struct** (≤ 64 bits) | variable | 4 | TyDesc* \| 0b100 | packed fields | Small struct optimization |
| **Struct** (> 64 bits) | variable | 0 | TyDesc* | *struct | Heap allocation |
| **Enum** (unit variant) | variable | 1 | discriminant | TyTag::Enum | No payload |
| **Enum** (small payload) | variable | 4 | TyDesc* \| 0b100 | disc + payload | If ≤ 64 bits total |
| **Enum** (large payload) | variable | 0 | TyDesc* | *enum | Heap allocation |
| **Option None** | variable | 1 | 0 | TyTag::Option | Special case |
| **Option Some(T)** | variable | depends | ... | ... | Depends on T size |
| **Result Ok(T)** | variable | depends | ... | ... | Depends on T size |
| **Result Err** | variable | 0 | TyDesc* | *Error | Always heap |
| **Data** (nested) | 128 bits | 0 | TyDesc* | *Data | Points to another Data |
| **Error** | variable | 0 | TyDesc* | *Error | Heap allocation |

## Complete Numeric Tower Support

The design fully supports all numeric types:

**Inline without tydesc (Tag 1):**
- bool, small u32/i32

**Inline with tydesc (Tag 4):**
- f32, f64 (← important!)
- u32, i32, u64, i64 (all values)

**Heap-allocated (Tag 0):**
- u128, i128, u256, i256
- Int (arbitrary precision bigint)

This ensures f64 is first-class and inline, which is critical for modern numeric computing.

## Safety Guarantees

✅ **All pointers are full 64-bit** - no truncation, works on Intel LA57, ARM52, WASM64
✅ **Standard 8-byte alignment** - universal malloc guarantee, works everywhere
✅ **Only 3-bit tags** - proven technique (V8, SpiderMonkey, CPython)
✅ **No clever tricks** - comprehensible and debuggable
✅ **ABI-compatible** - can be passed across language boundaries
✅ **Portable** - works on x86-64, ARM64, WASM64, RISC-V, etc.

## Implementation Structure

### Construction

```rust
impl Data {
    // Tag 0: Default two-pointer case
    pub fn from_pointers(tydesc: *const TyDesc, value: *const u8) -> Self {
        debug_assert_eq!(tydesc as usize & TAG_MASK as usize, 0);
        Self {
            primary: tydesc as u64,    // Untagged (tag = 0)
            secondary: value as u64,
        }
    }

    // Tag 4: Tydesc + inline 64-bit value
    pub fn from_inline64(tydesc: *const TyDesc, value: u64) -> Self {
        debug_assert_eq!(tydesc as usize & TAG_MASK as usize, 0);
        Self {
            primary: (tydesc as u64) | 0b100,
            secondary: value,
        }
    }

    // Tag 1: Small immediate, no tydesc
    pub fn from_immediate(value: u64, tytag: TyTag) -> Self {
        debug_assert!(value < (1u64 << 61));  // Must fit in 61 bits
        Self {
            primary: (value << 3) | 0b001,
            secondary: tytag as u64,
        }
    }
}
```

### Accessors

```rust
impl Data {
    pub fn tag(&self) -> u8 {
        (self.primary & TAG_MASK) as u8
    }

    pub fn tydesc(&self) -> *const TyDesc {
        match self.tag() {
            0 => self.primary as *const TyDesc,
            4 => (self.primary & PTR_MASK) as *const TyDesc,
            1 => {
                // Need to synthesize or look up tydesc for TyTag
                synthesize_tydesc_for_tytag(self.secondary as u8)
            }
            _ => panic!("invalid tag"),
        }
    }

    pub fn value_ptr(&self) -> *const u8 {
        match self.tag() {
            0 => self.secondary as *const u8,
            _ => panic!("value not stored as pointer"),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self.tag() {
            1 if self.secondary as u8 == TyTag::Bool as u8 => {
                Some((self.primary >> 3) != 0)
            }
            4 => {
                unsafe {
                    let tydesc = self.tydesc();
                    if (*tydesc).type_tag == TyTag::Bool {
                        return Some(self.secondary != 0);
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        if self.tag() == 4 {
            unsafe {
                let tydesc = self.tydesc();
                if (*tydesc).type_tag == TyTag::F64 {
                    return Some(f64::from_bits(self.secondary));
                }
            }
        }
        None
    }
}
```

### Type-Specific Constructors

```rust
impl Data {
    pub fn from_bool(value: bool) -> Self {
        Self::from_immediate(value as u64, TyTag::Bool)
    }

    pub fn from_u32(value: u32) -> Self {
        if value <= (1u32 << 30) {
            Self::from_immediate(value as u64, TyTag::U32)
        } else {
            Self::from_inline64(get_u32_tydesc(), value as u64)
        }
    }

    pub fn from_f64(value: f64) -> Self {
        Self::from_inline64(get_f64_tydesc(), value.to_bits())
    }

    pub fn from_int(int_ptr: *const Int) -> Self {
        Self::from_pointers(get_int_tydesc(), int_ptr as *const u8)
    }

    pub fn from_string(string_ptr: *const String) -> Self {
        Self::from_pointers(get_string_tydesc(), string_ptr as *const u8)
    }
}
```

## Examples with Bit Patterns

### Example 1: bool (true)

```
Tag 1 encoding:
primary:   0x0000000000000009  = (1 << 3) | 0b001
secondary: 0x0000000000000001  = TyTag::Bool (1)

Interpretation:
- Tag = 1 (small immediate)
- Value = 1 (true)
- Type = Bool
```

### Example 2: f64 (3.14159...)

```
Tag 4 encoding:
primary:   0x00007f8a4c001004  = tydesc_ptr | 0b100
secondary: 0x400921fb54442d18  = IEEE-754 representation

Interpretation:
- Tag = 4 (inline with tydesc)
- TyDesc at 0x00007f8a4c001000
- Value = 3.14159265358979... (f64)
```

### Example 3: u32 (small, 42)

```
Tag 1 encoding:
primary:   0x0000000000000151  = (42 << 3) | 0b001
secondary: 0x0000000000000002  = TyTag::U32 (2)

Interpretation:
- Tag = 1 (small immediate)
- Value = 42
- Type = U32
```

### Example 4: u32 (large, 3000000000)

```
Tag 4 encoding:
primary:   0x00007f8a4c002004  = tydesc_ptr | 0b100
secondary: 0x00000000b2d05e00  = 3000000000

Interpretation:
- Tag = 4 (inline with tydesc)
- TyDesc at 0x00007f8a4c002000
- Value = 3000000000 (doesn't fit in 30 bits)
```

### Example 5: Int (bigint)

```
Tag 0 encoding:
primary:   0x00007f8a4c003000  = tydesc_ptr (untagged)
secondary: 0x00007f8a4c004000  = pointer to Int struct

Interpretation:
- Tag = 0 (two pointers)
- TyDesc at 0x00007f8a4c003000
- Int struct at 0x00007f8a4c004000
```

### Example 6: String

```
Tag 0 encoding:
primary:   0x00007f8a4c005000  = tydesc_ptr (untagged)
secondary: 0x00007f8a4c006000  = pointer to String struct

Interpretation:
- Tag = 0 (two pointers)
- TyDesc at 0x00007f8a4c005000
- String struct at 0x00007f8a4c006000
```

## Design Rationale

### Why Two 64-bit Words?

- Standard size for a "fat pointer" in many languages
- Fits in two registers on 64-bit architectures
- Can be passed efficiently in calling conventions
- Matches current Datalove Data representation

### Why Tag in Primary (Not Secondary)?

- Primary is the "type" information (usually tydesc)
- Tagging the type pointer is more natural
- Secondary can be used for full 64-bit values without masking
- Consistent with other languages (Swift, etc.)

### Why Only 3 Bits (Not More)?

- More conservative - 8-byte alignment is universal
- Comprehensible - only 8 possible tags
- Debuggable - easy to see in hex (last digit is 0,1,2,...,7)
- Standard - proven in production systems

### Why Not NaN-Boxing?

- NaN-boxing is clever but confusing
- Doesn't work well with full numeric tower (u64, i64, f64, u128, etc.)
- Harder to explain and debug
- Tagged pointers are simpler and more general

### Why Not Always Use Tag 0?

- Tag 0 works but wastes allocations for primitives
- bool, u32, f64 are extremely common - inline them
- Avoid GC pressure and memory traffic
- Performance matters for data-intensive applications

### Why Separate Tag 1 and Tag 4?

- Tag 1: Fast path for common anonymous primitives (bool, small u32)
- Tag 4: Support named types and larger values (f64, u64, named u32)
- Clear separation of concerns
- Room for growth (Tag 2, 3, 5, 6, 7 available)

## Future Extensions

Possible uses for reserved tags:

**Tag 2** - Could be used for:
- Inline immediate with type info (alternative to Tag 1)
- Small strings (up to 15 bytes inline)

**Tag 3** - Could be used for:
- Optimized Option/Result representation
- Discriminant + small payload inline

**Tag 5** - Could be used for:
- Nested Data with metadata
- Capability pointers

**Tag 6, 7** - Reserved for future innovations

## Comparison to Current Design

**Current (unoptimized):**
```rust
struct Data {
    data: usize,              // Always a pointer (even for bool!)
    tydesc: *const TyDesc,    // Always a pointer
}
```
Every Data is two pointers, even `bool`.

**New (optimized):**
```rust
struct Data {
    primary: u64,     // Tagged: pointer OR immediate
    secondary: u64,   // Pointer OR immediate
}
```
- bool: 0 allocations (Tag 1)
- f64: 0 allocations (Tag 4)
- String: 0 extra allocations (Tag 0, same as before)

**Benefits:**
- Reduced allocations for primitives
- Reduced memory traffic
- Reduced GC pressure
- Better cache locality
- Still backward compatible (Tag 0 is identical to current)

## Implementation Checklist

- [ ] Define `Data` struct in `anypack.rs`
- [ ] Implement tag manipulation helpers
- [ ] Implement constructors for each tag type
- [ ] Implement accessors for common types
- [ ] Add comprehensive tests for each encoding
- [ ] Document bit patterns in code comments
- [ ] Add examples to demonstrate usage
- [ ] Update runtime to use new Data representation
- [ ] Benchmark vs current implementation
