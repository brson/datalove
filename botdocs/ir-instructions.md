# Datalove IR Instructions Reference

## Overview

The Datalove IR (Intermediate Representation) is an SSA-based (Static Single Assignment) intermediate form designed for datafun functions. It uses:

- **SSA form** for immutable values (expression temps, let bindings)
- **Explicit slots** for mutable bindings (var)
- **Control Flow Graph (CFG)** structure with basic blocks and terminators

### Design Goals

1. **Simple slot-based interpretation** - uniform frame offset execution
2. **Direct lowering to LLVM/Cranelift** - SSA values → registers, slots → stack
3. **Clean semantics** - no nested expressions, 2-3 operands max per instruction

---

## Core Types

### Identifiers

- **ValueId** (`v0`, `v1`, ...) - SSA value, defined exactly once, immutable
- **SlotId** (`s0`, `s1`, ...) - Mutable slot for `var` bindings, can be reassigned
- **ParamId** (`p0`, `p1`, ...) - Function parameter (reference to caller's data)
- **BlockId** (`block0`, `block1`, ...) - Basic block identifier
- **FuncId** (`f0`, `f1`, ...) - Function identifier
- **IrModuleId** (`m0`, `m1`, ...) - Module identifier

### Operand Types

```rust
enum Operand {
    Value(ValueId),                              // Local SSA value
    Slot(SlotId),                                // Local mutable slot
    Param(ParamId),                              // Function parameter
    ExternalValue { unit: u32, value: ValueId }, // Value from previous script unit
    ExternalSlot { unit: u32, slot: SlotId },    // Slot from previous script unit
}
```

### Slot Destinations

```rust
enum SlotDest {
    Local(SlotId),                               // Local slot in current unit
    External { unit: u32, slot: SlotId },        // Slot in previous script unit
}
```

---

## Instructions

### Constants and Basic Moves

#### `Const`
```
v0 = const 42u32
v1 = const "hello"
v2 = const ()
```

Loads a constant value into an SSA value.

**Supported constants:**
- Unit: `()`
- Booleans: `true`, `false`
- Integers: `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`
- Bigint: `42int` (stored as limbs array + sign)
- Float: `3.14f32`
- String: `"hello world"`

**Semantics:** Creates a new value with the specified constant.

---

#### `Copy`
```
v1 = copy v0
v2 = copy s0
```

Creates a shallow bitwise copy of the source operand.

**Restrictions:** Source must be a **Copy type** (no heap allocations):
- Primitives: Unit, Bool, U8-U64, I8-I64, F32
- Tuples/structs of copy types

**Semantics:** Performs a shallow bitwise copy. Source value remains valid after copy.

---

#### `Move`
```
v1 = move v0
v2 = move s0
```

Transfers ownership of the source operand to the destination.

**Semantics:**
- Shallow copy of the data
- Source is marked as dropped (no longer accessible)
- Used for non-copy types (Int, String, List, Set, Map, etc.)

---

### Arithmetic Operations

#### `BinOp`
```
v2 = add v0, v1
v3 = mul v0, v1
v4 = eq v0, v1
```

Performs a binary operation on two operands.

**Operators:**
- **Arithmetic:** `add`, `sub`, `mul`, `div`, `mod`
- **Comparison:** `eq`, `ne`, `lt`, `le`, `gt`, `ge`
- **Logical:** `and`, `or`
- **Bitwise:** `bitand`, `bitor`, `bitxor`, `shl`, `shr`

**Type support:**
- Fixed-width integers: `u8`-`u64`, `i8`-`i64`
- Bigint: `Int` (via runtime calls for Add, Sub, Mul, Div)
- Float: `f32`
- Bool: `and`, `or`

**Semantics:** Operands are **borrowed** (read by reference, not consumed). Result written to dest.

---

#### `BinOpChecked`
```
v2, v3 = add.checked v0, v1
```

Performs a binary operation with overflow detection.

**Output:**
- `dest` - Result value (may be meaningless if overflow occurred)
- `overflow` - Boolean flag (true if overflow/division-by-zero occurred)

**Use cases:**
- Optional arithmetic (`+?`, `-?`, `*?`, `/?`) - early return with None on overflow
- Checked arithmetic (`+!`, `-!`, `*!`, `/!`) - early return with Err on overflow

**Supported types:** All fixed-width integers (`u8`-`u64`, `i8`-`i64`)

**Semantics:**
- For division: detects both div-by-zero and signed overflow (MIN / -1)
- Operands are borrowed

---

#### `UnaryOp`
```
v1 = neg v0
v1 = not v0
v1 = bitnot v0
```

Performs a unary operation.

**Operators:**
- `neg` - Numeric negation (signed integers, f32, Int)
- `not` - Logical NOT (bool)
- `bitnot` - Bitwise NOT (all integer types)

**Semantics:** Operand is borrowed.

---

#### `UnaryOpChecked`
```
v1, v2 = neg.checked v0
```

Performs a unary operation with overflow detection.

**Use cases:**
- `-?` (optional negation) - early return with None on overflow
- `-!` (checked negation) - early return with Err on overflow

**Overflow condition:** Only signed integers can overflow (negating MIN value)

**Semantics:** Operand is borrowed.

---

#### `Widen`
```
v1 = widen v0
```

Converts a fixed-width integer to arbitrary-precision Int (bigint).

**Supported sources:** `u8`-`u64`, `i8`-`i64`

**Semantics:** Allocates a new Int value, converts the fixed-width value to limbs representation.

---

### Function Calls

#### `Call`
```
v2 = call f0(v0, v1)
v3 = call unit2.f1(v0)
v4 = call m0.f2(v0)
```

Calls a function and stores the return value.

**Function references:**
- `Local(f0)` - Function defined locally
- `External { unit: 2, func: f1 }` - Function from previous script unit
- `Module { module: m0, func: f2 }` - Function from a module

**Semantics:**
- Arguments are **moved** (ownership transferred to callee)
- Return value is copied to persistent storage (outlives callee frame)

---

### Aggregate Types

#### `Pack`
```
v2 = pack Tuple2 { v0, v1 }
v3 = pack Struct3 { v0, v1, v2 }
```

Constructs a struct or tuple from field values.

**Type references:** `Tuple{n}`, `Struct{n}`, `()` (unit), `Option`, `Result`, `List`, `Set`, `Map`

**Semantics:**
- Fields are **moved** into the aggregate
- Source values marked as dropped

---

#### `Unpack`
```
(v1, v2, v3) = unpack v0
```

Destructures a struct or tuple into individual field values.

**Semantics:**
- Source aggregate is **consumed** (marked as dropped)
- Each field is moved to its destination value

---

### Option Type

#### `WrapSome`
```
v1 = some v0
```

Wraps a value in the `Some` variant of Option.

**Semantics:** Value is **moved** into the Option.

---

#### `WrapNone`
```
v0 = none
```

Creates the `None` variant of Option.

**Semantics:** Creates an uninitialized Option in the None state.

---

#### `UnwrapOption`
```
v1, v2 = unwrap_option v0
```

Unwraps an Option, producing the inner value and a boolean flag.

**Output:**
- `dest` - Inner value (only valid if `is_some` is true)
- `is_some` - Boolean flag (true if Option was Some)

**Semantics:**
- Source Option is **consumed**
- If None, inner value is uninitialized (don't read it!)
- Used with conditional branch to handle Some/None cases

---

### Result Type

#### `WrapOk`
```
v1 = ok v0
```

Wraps a value in the `Ok` variant of Result.

**Semantics:** Value is **moved** into the Result.

---

#### `WrapErr`
```
v1 = err v0
```

Wraps an Error value in the `Err` variant of Result.

**Semantics:** Error value is **moved** into the Result.

---

#### `UnwrapResult`
```
v1, v2, v3 = unwrap_result v0
```

Unwraps a Result, producing both Ok and Err values plus a flag.

**Output:**
- `ok_dest` - Ok payload (only valid if `is_ok` is true)
- `err_dest` - Error value (only valid if `is_ok` is false)
- `is_ok` - Boolean flag (true if Result was Ok)

**Semantics:**
- Source Result is **consumed**
- Only one of ok_dest/err_dest is initialized (based on is_ok)
- Used with conditional branch to handle Ok/Err cases

---

### Enum Types

#### `EnumVariant`
```
v0 = enum_variant 0           // Variant without payload
v1 = enum_variant 1 v0        // Variant with payload
```

Creates an enum variant.

**Parameters:**
- `variant_index` - Index into sorted variants of the enum type
- `payload` - Optional payload value (moved into enum)

**Semantics:** If payload is provided, it is **moved** into the enum.

---

### Error and Data Types

#### `ErrorFrom`
```
v1 = error_from v0
```

Creates an Error value from any value.

**Semantics:** Inner value is **consumed** and wrapped in Error type.

---

#### `DataFrom`
```
v1 = data_from v0
```

Creates a Data value from any value.

**Semantics:** Inner value is **consumed** and wrapped in Data type (dynamic type).

---

### Collections

#### `ListNew`
```
v3 = list [v0, v1, v2]
```

Creates a new list from elements.

**Semantics:**
- Allocates list structure via runtime
- Reserves capacity for elements
- Elements are **moved** into the list

---

#### `SetNew`
```
v3 = set {v0, v1, v2}
```

Creates a new set from elements.

**Semantics:**
- Elements are sorted
- B-tree is built from sorted elements via runtime
- Elements are **moved** into the set
- Duplicates are handled by the runtime

---

#### `MapNew`
```
v4 = map {v0: v1, v2: v3}
```

Creates a new map from key-value pairs.

**Semantics:**
- Pairs are sorted by key
- B-tree is built from sorted pairs via runtime
- Keys and values are **moved** into the map

---

#### `TensorNew`
```
v4 = tensor [2, 3] [v0, v1, v2, v3, v4, v5]
```

Creates a new tensor with specified shape and elements.

**Parameters:**
- `shape` - Vector of dimension sizes (e.g., [2, 3] for 2x3 matrix)
- `elements` - Flattened element values in row-major order

**Semantics:**
- Allocates data array, shape array, strides array via runtime
- Computes strides for row-major layout
- Elements are **moved** into the tensor

---

### Mutable Slots

#### `SlotStore`
```
store s0, v0
store unit1.s0, v0
```

Stores a value to a mutable slot.

**Destinations:**
- `Local(s0)` - Store to local slot
- `External { unit: 1, slot: s0 }` - Store to slot in previous script unit

**Semantics:**
- If slot is already initialized, old value is **destroyed** first
- New value is **moved** into the slot
- Slot is marked as initialized

---

#### `ParamStore`
```
store p0, v0
```

Stores a value through a mutable parameter (writes back to caller).

**Semantics:**
- Used for `mut` parameter mode
- Value is **moved** to caller's storage

---

#### `SlotLoad`
```
v0 = load s0
```

Loads a value from a mutable slot (destructive read).

**Semantics:**
- Value is **moved** out of the slot
- Slot is marked as dropped (no longer accessible)
- Only used for consuming contexts (function args, return)
- For borrowing (binop, unaryop), use `Operand::Slot` directly

---

### Memory Management

#### `Drop`
```
drop v0
drop s0
```

Explicitly drops a value, running its destructor.

**Semantics:**
- Calls runtime destructor (`dtlv_rti_any_destroy_local`)
- Value/slot is marked as dropped
- Used at scope exits for non-copy types
- Copy types don't need drops

**When drops are emitted:**
- Before `return` statements
- At scope exits (end of if branches, loops, functions)
- Before `break`/`continue` in loops
- Before reassigning slots (in `SlotStore`)

---

### Debug Operations

#### `DebugLog`
```
debuglog v0
```

Logs a value for debugging purposes.

**Semantics:**
- Value is **borrowed** (not consumed)
- Used for debugging and testing

---

#### `Nop`
```
nop
```

No operation. May be used as a placeholder during lowering.

---

## Terminators

Terminators control how execution leaves a basic block.

### `Goto`
```
goto block1
goto block2(v0, v1)
```

Unconditional jump to a target block.

**Parameters:**
- `target` - Destination block
- `args` - Block arguments (moved to target block's parameters)

**Semantics:**
- Args are **moved** into target block's parameters
- Used for loop continue with new carry values

---

### `Branch`
```
branch v0, block1, block2
branch v0, block1(v1), block2(v2)
```

Conditional branch based on a boolean condition.

**Parameters:**
- `cond` - Boolean operand (borrowed)
- `then_block`, `then_args` - Block and args for true case
- `else_block`, `else_args` - Block and args for false case

**Semantics:**
- Condition is **borrowed**
- Args for the taken branch are **moved** to target block's parameters
- Used for if/else, loop exits, while conditions

---

### `Return`
```
return v0
return
```

Returns from a function.

**Semantics:**
- Value is **moved** to caller's return storage
- Function frame is destroyed after return

---

### `UnitEnd`
```
unit_end v0
unit_end
```

Normal completion of a script unit.

**Semantics:**
- Result value (if any) is **moved** to persistent storage
- Unit frame remains alive (for cross-unit references)

---

### `UnitEarlyReturn`
```
unit_early_return v0
```

Early return from a script unit (from `ret`, `?`, `!`, or checked operators).

**Semantics:**
- Value is **moved** to unit result storage
- Unit frame remains alive
- Ends script unit execution immediately

---

## Type System

### IrType Hierarchy

```
Unit                          // ()
Bool                          // true, false
U8, U16, U32, U64            // Unsigned integers
I8, I16, I32, I64            // Signed integers
Int                          // Arbitrary-precision bigint
F32                          // 32-bit float
String                       // UTF-8 string
Data                         // Dynamic data value
Error                        // Error value
Tuple(Vec<IrType>)           // Anonymous tuple
Struct(Vec<(String, IrType)>)// Anonymous struct with named fields
Enum(Vec<(String, Option<IrType>)>) // Anonymous enum with variants
List(Box<IrType>)            // List with element type
Set(Box<IrType>)             // Set with element type
Map(Box<IrType>, Box<IrType>)// Map with key/value types
Option(Box<IrType>)          // Option with inner type
Result(Box<IrType>)          // Result with ok type
Tensor(Box<IrType>, u32)     // Tensor with element type and rank
```

### Copy Types

Types that can be duplicated with shallow bitwise copy:
- **Copy:** Unit, Bool, U8-U64, I8-I64, F32
- **Copy if fields are copy:** Tuple, Struct, Enum, Option

Types that require move semantics:
- **Non-copy:** Int, String, Data, Error, List, Set, Map, Tensor, Result

---

## Parameter Passing Modes

```rust
enum ParamMode {
    In,   // By-value: ownership transfers to callee (default)
    Out,  // By-out-ptr: caller allocates, callee initializes
    Ref,  // By-ref: immutable borrow, caller retains ownership
    Mut,  // By-mut-ref: mutable borrow, caller retains ownership
}
```

---

## Example: Complete Function

```
fn factorial(p0):
block0:
    v0 = const 1i32
    v1 = const 1i32
    goto block1(v0, v1)
block1(v2, v3):
    v4 = le v2, p0
    branch v4, block2, block3
block2:
    v5 = mul v3, v2
    v6 = const 1i32
    v7 = add v2, v6
    goto block1(v7, v5)
block3:
    return v3
```

This computes factorial using a loop:
- `block0` - Initialize counter (v0=1) and accumulator (v1=1)
- `block1` - Loop header with parameters (counter v2, accumulator v3)
- `block2` - Loop body: multiply accumulator, increment counter, continue
- `block3` - Exit: return accumulator

---

## Script Units vs Functions

| Aspect          | IrFunction            | IrScriptUnit              |
|-----------------|-----------------------|---------------------------|
| Parameters      | Yes                   | None                      |
| External refs   | No                    | Yes (cross-unit)          |
| Defines funcs   | No                    | Yes                       |
| Terminator      | Return                | UnitEnd / UnitEarlyReturn |
| Frame lifetime  | Transient             | Persistent                |

Script units support cross-unit references via `Operand::ExternalValue` and `Operand::ExternalSlot`.
