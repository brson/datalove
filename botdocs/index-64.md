# index-64 Feature

Compile-time feature that switches `Usize`/`Isize` types between 32-bit and 64-bit representations.

## Usize and Isize Types

`Usize` and `Isize` are the index types used throughout the language for:

- Collection sizes and capacities (List, Map, Set, Table)
- Tensor dimensions and strides
- String length and capacity
- Bigint limb allocation (Int.capacity)

Defined in `datalove-rtdt/src/lib.rs` as newtype wrappers:

```rust
#[repr(transparent)]
pub struct Usize(pub UsizeRepr);

#[repr(transparent)]
pub struct Isize(pub IsizeRepr);
```

The `#[repr(transparent)]` ensures ABI compatibility with the underlying representation.

## Feature Configuration

| Aspect | Default (32-bit) | With index-64 |
|--------|------------------|---------------|
| `UsizeRepr` | `u32` | `u64` |
| `IsizeRepr` | `i32` | `i64` |
| `INDEX_SIZE` | 4 bytes | 8 bytes |
| `INDEX_ALIGN` | 4 bytes | 8 bytes |

The feature is declared in `datalove-rtdt/Cargo.toml`:

```toml
[features]
index-64 = []
```

## Memory Layout Impact

Collections grow by 4-8 bytes when index-64 is enabled:

| Type | Size Increase |
|------|---------------|
| List | +8 bytes (len + capacity) |
| String | +8 bytes (len + capacity) |
| Map/Set | +8 bytes |
| Table | +8 bytes |
| Int | +4 bytes (capacity field) |
| Tensor | +8 bytes per rank (shape + strides) |

## Runtime Impact (datalove-rt, datalove-rtdt)

The runtime uses `INDEX_SIZE` and `INDEX_ALIGN` constants for:

- Type descriptor emission (TyDesc size/alignment for Usize/Isize)
- Collection memory allocation and capacity tracking
- Intrinsic operations (30+ operations on Usize/Isize)

Key constants in `datalove-rtdt`:

```rust
pub const INDEX_SIZE: u32 = 4;   // or 8 with index-64
pub const INDEX_ALIGN: u32 = 4;  // or 8 with index-64
```

## Interpreter Impact (datalove-datafun-interp)

The interpreter in `ops.rs`:

- Recognizes Usize/Isize as fixed-width integer types
- Treats them as copy types (no drop needed)
- Handles widening to Int with correct magnitude extraction
- All arithmetic operations respect the configured width

## AOT Compiler Impact (datalove-datafun-aot-cranelift)

Cranelift codegen uses centralized constants in `index_types.rs`:

```rust
#[cfg(not(feature = "index-64"))]
pub const INDEX_TYPE: cranelift_codegen::ir::Type = cl_types::I32;
#[cfg(feature = "index-64")]
pub const INDEX_TYPE: cranelift_codegen::ir::Type = cl_types::I64;

pub const INDEX_BITS: u8 = 32;  // or 64
```

These constants are used in:

- `types.rs` - IR type to Cranelift type mapping
- `runtime.rs` - Function signatures for collection operations
- `codegen/ops.rs` - Checked arithmetic operations
- `codegen/constants.rs` - Int literal emission
- `codegen/collections.rs` - Tensor initialization
- `tydesc_emit.rs` - Type descriptor data emission

## Feature Propagation

The feature propagates through the dependency tree:

```
datalove-datafun
├── datalove-rt/index-64
├── datalove-datafun-interp/index-64
│   ├── datalove-rtdt/index-64
│   └── datalove-datafun-ir/index-64
├── datalove-datafun-ir/index-64
│   ├── datalove-rtdt/index-64
│   └── datalove-datalit/index-64
├── datalove-datafun-compiler/index-64
│   └── (7 transitive dependencies)
├── datalove-datafun-aot-cranelift/index-64
└── datalove-datafun-jit/index-64
```

Leaf crates that declare but don't propagate:
- `datalove-rtdt`
- `datalove-datafun-intrinsics`
- `datalove-exampletest`

## Testing

Two test modes in the justfile:

```bash
just test      # Default 32-bit mode
just test-64   # 64-bit index mode
```

### Expected Output Files

The test framework (`datalove-exampletest`) supports feature-specific expected outputs:

```
tests/fixtures/example.dfs
├── example.out.expected      # Default output
└── example.out.expected.64   # Used when index-64 enabled
```

If `.out.expected.64` exists and index-64 is enabled, it takes precedence.

### What Differs Between Modes

1. **Literal ranges** - Max usize changes from ~4B to ~18E
2. **Error messages** - Type range descriptions differ
3. **Collection capacities** - Internal representation sizes differ
4. **Arithmetic wrapping** - Overflow points differ

Example diagnostic constants in `datalove-datalit/src/tycheck/types.rs`:

```rust
#[cfg(not(feature = "index-64"))]
const USIZE_RANGE: &str = "usize can represent values from 0 to 4,294,967,295";
#[cfg(feature = "index-64")]
const USIZE_RANGE: &str = "usize can represent values from 0 to 18,446,744,073,709,551,615";
```

## Build Instructions

```bash
# Build with default 32-bit indices
cargo build --workspace

# Build with 64-bit indices
cargo build --workspace --features index-64

# Run all tests in both modes
just test && just test-64
```

## Design Rationale

32-bit indices are the default because:

- Most collections won't exceed 4 billion elements
- Reduces memory overhead for common cases
- Matches WebAssembly's 32-bit memory model

64-bit indices are available for:

- Large-scale data processing
- Compatibility with native 64-bit platforms
- Future-proofing for very large datasets

The compile-time switch (vs runtime) ensures:

- No runtime overhead for mode detection
- Consistent struct layouts across the codebase
- Type-safe separation between modes
