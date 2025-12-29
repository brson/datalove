# Datalove Compared To ...

Comparison of Datalit types and literals with

- [Python](#user-content-python)
- [JavaScript](#user-content-javascript)
- [Julia](#user-content-julia)


## Python

Python integers are arbitrary precision by default, like Datalit.
Fixed-size integers require NumPy.
Type hints are optional at runtime; enforced by type checkers like mypy.

### Primitive Types

| Type | Datalit | Python |
|------|---------|--------|
| Boolean | `true`, `false` | `True`, `False` |
| Unsigned 8-bit | `: u8 / 1` | `numpy.uint8(1)` |
| Unsigned 16-bit | `: u16 / 1` | `numpy.uint16(1)` |
| Unsigned 32-bit | `: u32 / 1` | `numpy.uint32(1)` |
| Unsigned 64-bit | `: u64 / 1` | `numpy.uint64(1)` |
| Signed 8-bit | `: i8 / -1` | `numpy.int8(-1)` |
| Signed 16-bit | `: i16 / -1` | `numpy.int16(-1)` |
| Signed 32-bit | `: i32 / -1` | `numpy.int32(-1)` |
| Signed 64-bit | `: i64 / -1` | `numpy.int64(-1)` |
| Float 32-bit | `: f32 / 1.0` | `numpy.float32(1.0)` |
| Arbitrary int | `: int / 42` | `42` |
| String | `"hello"` | `"hello"` |

### Collection Types

| Type | Datalit | Python |
|------|---------|--------|
| List | `[1, 2, 3]` | `[1, 2, 3]` |
| Typed list | `: [u32] / [1, 2]` | `list[int]` (hint only) |
| Map/Dict | `map { 0 = 5, 1 = 2 }` | `{0: 5, 1: 2}` |
| Set | `set { 1, 2, 3 }` | `{1, 2, 3}` |
| Empty set | `set {}` | `set()` |
| Tensor type | `tensor<f32, 2>` | `numpy.ndarray` |
| Tensor literal | `tensor [2,2] [1 2, 3 4]` | `numpy.array([[1,2],[3,4]])` |

### Aggregate Types

| Type | Datalit | Python |
|------|---------|--------|
| Tuple | `(1, 2)` | `(1, 2)` |
| Unit | `()` | `()` |
| Struct/Record | `{ x = 1, y = 2 }` | `{"x": 1, "y": 2}` or dataclass |
| Enum variant | `enum Foo` | `Foo` (enum member) |
| Enum with data | `enum Bar(42)` | - |

### Special Types

| Type | Datalit | Python |
|------|---------|--------|
| None | `none` | `None` |
| Option type | `?T` | `Optional[T]` / `T \| None` |
| Some wrapper | `some expr` | - (bare value) |
| Error value | `error "msg"` | `raise Exception("msg")` |
| Result type | `!T` | - (exceptions) |
| Dynamic/Any | `data 1` | any value |

### Numeric Literals

| Format | Datalit | Python |
|--------|---------|--------|
| Decimal | `42` | `42` |
| Hexadecimal | `0xFF` | `0xFF` |
| Binary | - | `0b1010` |
| Octal | - | `0o17` |
| Float | `3.14` | `3.14` |
| Scientific | - | `1e10` |
| BigInt | `42` (default) | `42` (default) |

### Type Annotations

| Purpose | Datalit | Python |
|---------|---------|--------|
| Type hint | `: u32 / 42` | `x: int = 42` |
| Typed collection | `: [u32] / [1]` | `x: list[int]` |
| Typed map | `: map<u32, string>` | `x: dict[int, str]` |


## JavaScript

JavaScript numbers are 64-bit floats by default.
Fixed-size integers require TypedArrays.
BigInt (`42n` suffix) provides arbitrary precision integers.
TypeScript adds compile-time type annotations.

### Primitive Types

| Type | Datalit | JavaScript |
|------|---------|------------|
| Boolean | `true`, `false` | `true`, `false` |
| Unsigned 8-bit | `: u8 / 1` | `new Uint8Array([1])[0]` |
| Unsigned 16-bit | `: u16 / 1` | `new Uint16Array([1])[0]` |
| Unsigned 32-bit | `: u32 / 1` | `new Uint32Array([1])[0]` |
| Unsigned 64-bit | `: u64 / 1` | `new BigUint64Array([1n])[0]` |
| Signed 8-bit | `: i8 / -1` | `new Int8Array([-1])[0]` |
| Signed 16-bit | `: i16 / -1` | `new Int16Array([-1])[0]` |
| Signed 32-bit | `: i32 / -1` | `new Int32Array([-1])[0]` |
| Signed 64-bit | `: i64 / -1` | `new BigInt64Array([-1n])[0]` |
| Float 32-bit | `: f32 / 1.0` | `new Float32Array([1.0])[0]` |
| Arbitrary int | `: int / 42` | `42n` |
| String | `"hello"` | `"hello"` |

### Collection Types

| Type | Datalit | JavaScript |
|------|---------|------------|
| List/Array | `[1, 2, 3]` | `[1, 2, 3]` |
| Typed array | `: [u32] / [1, 2]` | `Uint32Array.of(1, 2)` |
| Map | `map { 0 = 5, 1 = 2 }` | `new Map([[0, 5], [1, 2]])` |
| Set | `set { 1, 2, 3 }` | `new Set([1, 2, 3])` |
| Object literal | `{ x = 1, y = 2 }` | `{ x: 1, y: 2 }` |
| Tensor type | `tensor<f32, 2>` | - (no native) |
| Tensor literal | `tensor [2,2] [1 2, 3 4]` | - |

### Aggregate Types

| Type | Datalit | JavaScript |
|------|---------|------------|
| Tuple | `(1, 2)` | `[1, 2]` (no native tuple) |
| Unit | `()` | - |
| Struct/Record | `{ x = 1, y = 2 }` | `{ x: 1, y: 2 }` |
| Enum variant | `enum Foo` | - |
| Enum with data | `enum Bar(42)` | - |

### Special Types

| Type | Datalit | JavaScript |
|------|---------|------------|
| None/Null | `none` | `null`, `undefined` |
| Option type | `?T` | `T \| null` (TypeScript) |
| Some wrapper | `some expr` | - (bare value) |
| Error value | `error "msg"` | `throw new Error("msg")` |
| Result type | `!T` | - (exceptions) |
| Dynamic/Any | `data 1` | any value |

### Numeric Literals

| Format | Datalit | JavaScript |
|--------|---------|------------|
| Decimal | `42` | `42` |
| Hexadecimal | `0xFF` | `0xFF` |
| Binary | - | `0b1010` |
| Octal | - | `0o17` |
| Float | `3.14` | `3.14` |
| Scientific | - | `1e10` |
| BigInt | `42` (default) | `42n` |

### Type Annotations (TypeScript)

| Purpose | Datalit | TypeScript |
|---------|---------|------------|
| Type hint | `: u32 / 42` | `x: number = 42` |
| Typed array | `: [u32] / [1]` | `x: number[]` |
| Typed map | `: map<u32, string>` | `Map<number, string>` |


## Julia

Julia has native fixed-size types like Datalit.
Arbitrary precision requires `big()` or `BigInt()`.
Uses `::` for type assertions/annotations.
Arrays are 1-indexed by default.

### Primitive Types

| Type | Datalit | Julia |
|------|---------|-------|
| Boolean | `true`, `false` | `true`, `false` |
| Unsigned 8-bit | `: u8 / 1` | `UInt8(1)` |
| Unsigned 16-bit | `: u16 / 1` | `UInt16(1)` |
| Unsigned 32-bit | `: u32 / 1` | `UInt32(1)` |
| Unsigned 64-bit | `: u64 / 1` | `UInt64(1)` |
| Signed 8-bit | `: i8 / -1` | `Int8(-1)` |
| Signed 16-bit | `: i16 / -1` | `Int16(-1)` |
| Signed 32-bit | `: i32 / -1` | `Int32(-1)` |
| Signed 64-bit | `: i64 / -1` | `Int64(-1)` |
| Float 32-bit | `: f32 / 1.0` | `Float32(1.0)` |
| Arbitrary int | `: int / 42` | `big(42)` |
| String | `"hello"` | `"hello"` |

### Collection Types

| Type | Datalit | Julia |
|------|---------|-------|
| Array | `[1, 2, 3]` | `[1, 2, 3]` |
| Typed array | `: [u32] / [1, 2]` | `UInt32[1, 2]` |
| Dict | `map { 0 = 5, 1 = 2 }` | `Dict(0 => 5, 1 => 2)` |
| Set | `set { 1, 2, 3 }` | `Set([1, 2, 3])` |
| Tensor type | `tensor<f32, 2>` | `Array{Float32, 2}` |
| Tensor literal | `tensor [2,2] [1 2, 3 4]` | `[1 2; 3 4]` |

### Aggregate Types

| Type | Datalit | Julia |
|------|---------|-------|
| Tuple | `(1, 2)` | `(1, 2)` |
| Unit | `()` | `()` |
| Named tuple | `{ x = 1, y = 2 }` | `(x=1, y=2)` |
| Enum variant | `enum Foo` | - |
| Enum with data | `enum Bar(42)` | - |

### Special Types

| Type | Datalit | Julia |
|------|---------|-------|
| Nothing | `none` | `nothing` |
| Option type | `?T` | `Union{T, Nothing}` |
| Some wrapper | `some expr` | `Some(value)` |
| Error value | `error "msg"` | `error("msg")` |
| Result type | `!T` | - (exceptions) |
| Ok wrapper | `ok expr` | `Ok(value)` (Results.jl) |
| Error wrapper | `er expr` | `Err(value)` (Results.jl) |
| Dynamic/Any | `data 1` | `Any` |

### Numeric Literals

| Format | Datalit | Julia |
|--------|---------|-------|
| Decimal | `42` | `42` |
| Hexadecimal | `0xFF` | `0xFF` |
| Binary | - | `0b1010` |
| Octal | - | `0o17` |
| Float | `3.14` | `3.14` |
| Scientific | - | `1e10` |
| BigInt | `42` (default) | `big"42"` or `123big` |

### Type Annotations

| Purpose | Datalit | Julia |
|---------|---------|-------|
| Type assertion | `: u32 / 42` | `x::Int32 = 42` |
| Typed collection | `: [u32] / [1]` | `x::Vector{Int32}` |
| Typed dict | `: map<u32, string>` | `Dict{Int32, String}` |
