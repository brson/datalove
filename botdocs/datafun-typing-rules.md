# Datafun Typing Rules

This document specifies the type system for datafun (the expression/statement language).
It extends datalit typing rules with functions, control flow, and operations.

## Core Principles

- **Bidirectional**: Expressions either synthesize (=>) or check (<=) against types
- **Extends datalit**: All datalit types are also datafun types
- **Function types**: Functions have types of the form `(T1, T2, ...) -> R`
- **Effect tracking**: Checked/optional operators require compatible function return types
- **Structural coercion**: Numeric widening and data coercion

## Type Representation

Datafun types extend datalit types:

```rust
enum Type {
    Datalit(datalit::Type),  // All datalit types
    Function(TypeFunction),   // Function types
}
```

### Function Types

```
TypeFunction {
    param_types: Vec<TypeAndHeap>,
    param_modes: Vec<ParamMode>,  // Value, Ref, RefMut
    return_type: TypeAndHeap,
}
```

Function type syntax: `(T1, T2, ...) -> R`

Examples:
```
(u32, u32) -> u32       // Two u32 params, returns u32
(string) -> bool        // One string param, returns bool
() -> ()                // No params, returns unit (void)
```

### Unit Type

The unit type `()` is represented as an empty anonymous tuple.
Functions without explicit return types implicitly return `()`.

## Expression Synthesis Rules

### Rule: Syn-Name
```
x is bound to type T in context
-------------------------------
x => T
```

### Rule: Syn-BinOp

Binary operations require operands of the same type.

#### Basic Arithmetic (+, -, *)
```
e1 => T, e2 => T
T is numeric type
---------------------------------
e1 + e2 => result_type(T)

where result_type(T) =
  - T if T is float (f32)
  - T if T is bigint (int)
  - int if T is fixed int (u8, i8, u16, i16, u32, i32, u64, i64)
```

**Note**: Fixed integer arithmetic widens to `int` to prevent overflow.

#### Division (/)
```
e1 => T, e2 => T
T is float type
-----------------
e1 / e2 => T
```

**Note**: Bare division only works for floats. Use `/!` or `/?` for integers.

#### Checked Arithmetic (+!, -!, *!, /!)
```
e1 => T, e2 => T
T is fixed int (or bigint for /!)
function returns Result<U>
---------------------------------
e1 +! e2 => T
```

**Note**: Checked operators return the element type directly. On overflow/error, the function early-returns an error.

#### Optional Arithmetic (+?, -?, *?, /?)
```
e1 => T, e2 => T
T is fixed int (or bigint for /?)
function returns Option<U>
---------------------------------
e1 +? e2 => T
```

**Note**: Optional operators return the element type directly. On overflow/error, the function early-returns None.

#### Comparison (<, >, <=, >=, ==, !=)
```
e1 => T, e2 => T
T is numeric type
-----------------
e1 < e2 => bool
```

### Rule: Syn-UnaryOp

#### Negation (-)
```
e => T
T is float or bigint
--------------------
-e => T
```

**Note**: Bare negation only for floats and bigints (no overflow possible).

#### Checked Negation (-!)
```
e => T
T is fixed signed int
function returns Result<U>
--------------------------
-!e => T
```

#### Optional Negation (-?)
```
e => T
T is fixed signed int
function returns Option<U>
--------------------------
-?e => T
```

**Note**: Unsigned integers cannot use `-?` (always fails - footgun prevention).

### Rule: Syn-FunctionCall
```
f is bound to function type (T1, T2, ..., Tn) -> R
args.len() == n
for all i: ei <= Ti
---------------------------------------------------
f(e1, e2, ..., en) => R
```

### Rule: Syn-TryOption (?)
```
e => Option<T>
function returns Option<U>
--------------------------
e? => T
```

**Note**: Unwraps the option, early-returning None if the value is None.

### Rule: Syn-TryResult (!)
```
e => Result<T>
function returns Result<U>
--------------------------
e! => T
```

**Note**: Unwraps the result, early-returning the error if the value is Err.

### Rule: Syn-Tuple
```
for all i: ei => Ti
Ti is datalit type
----------------------------------
(e1, e2, ..., en) => (T1, T2, ..., Tn)
```

**Note**: Tuple elements must be datalit types, not function types.

### Literal Synthesis

Literals follow the same rules as datalit:

- `true`, `false` => `bool`
- Integer literals => `u32` by default (or `i32` if negative fits)
- Float literals => `f32`
- Hex literals => `u32` by default
- String literals => `string`
- `none` => cannot synthesize (requires type context)
- Anonymous enums => cannot synthesize (requires type context)

### Rule: Syn-Some
```
e => T
------------------
some(e) => Option<T>
```

### Rule: Syn-Ok
```
e => T
------------------
ok(e) => Result<T>
```

### Rule: Syn-Er
```
-------------------
er(e) => cannot synthesize (requires type context)
```

### Rule: Syn-Data
```
e => T (any type)
------------------
data(e) => data
```

### Rule: Syn-Error
```
e => T (any type)
------------------
error(e) => error
```

### Collection Synthesis

Collections follow datalit rules with element type inference:

```
[e1, e2, ...] => [T]     where first element determines T
set{e1, e2, ...} => set<T>
map{k1 = v1, ...} => map<K, V>
```

Empty collections default to unit element type: `[] => [()]`

## Expression Checking Rules

### Rule: Check-Subsume
```
e => T'
T' = T OR can_widen(T', T) OR T = data
--------------------------------------
e <= T
```

### Rule: Check-None
```
-------------------
none <= Option<T>
```

### Rule: Check-Some
```
e <= T
-------------------
some(e) <= Option<T>
```

### Rule: Check-Ok
```
e <= T
-------------------
ok(e) <= Result<T>
```

### Rule: Check-Er
```
e <= error
-------------------
er(e) <= Result<T>
```

### Rule: Check-BinOp (Bidirectional Type Propagation)

Binary operators support checking mode for type propagation from context to operands.

#### Check-BinOp-Checked (Fixed Int)
```
T is fixed int (u8, i8, u16, i16, u32, i32, u64, i64, index, offset)
function returns Result<U>
e1 <= T
e2 <= T
------------------------------------------
e1 +! e2 <= T   (also -!, *!, /!)
```

**Note**: The expected type T propagates to both operands, allowing literals to infer their type from context.

#### Check-BinOp-Optional (Fixed Int)
```
T is fixed int
function returns Option<U>
e1 <= T
e2 <= T
------------------------------------------
e1 +? e2 <= T   (also -?, *?, /?)
```

#### Check-BinOp-Checked (Bigint Division)
```
T is bigint (int)
function returns Result<U>
e1 <= T
e2 <= T
------------------------------------------
e1 /! e2 <= T
```

#### Check-BinOp-Optional (Bigint Division)
```
T is bigint (int)
function returns Option<U>
e1 <= T
e2 <= T
------------------------------------------
e1 /? e2 <= T
```

#### Check-BinOp-Float
```
T is float (f32 or f64)
e1 <= T
e2 <= T
------------------------------------------
e1 + e2 <= T   (also -, *, /)
```

**Note**: Float literals can check against f32 or f64 and infer their type.

#### Check-BinOp Fallback

If checking fails (e.g., operand type mismatch, wrong return type), the expression falls through to synthesis mode and the synthesized type is compared against the expected type.

```
check fails
e => T'
T' = T OR can_widen(T', T)
---------------------------
e <= T
```

**Example - Type Propagation:**
```
fun add(): !u32
    ret ok (1 +! 2)    // 1 and 2 check against u32 from ok wrapper
end fun
```

1. `ok (1 +! 2)` checks against `!u32`
2. Check-Ok extracts inner type `u32`
3. `1 +! 2` checks against `u32`
4. Check-BinOp-Checked: return type is Result, T=u32 is fixed int
5. `1` checks against `u32` (Check-Int succeeds)
6. `2` checks against `u32` (Check-Int succeeds)
7. Expression type is `u32`

**Example - Return Type Validation:**
```
// ERROR: +? requires Option return, but script returns Result
let x: u32 = a +? b    // Falls through to synthesis, produces error
```

### Rule: Check-AnonEnum
```
variant V exists in expected enum type
payload <= expected payload type (if any)
-----------------------------------------
enum V(payload) <= enum{V: T, ...}
```

### Rule: Check-Int
```
n fits in expected integer type
-------------------------------
n <= T (where T is integer type)
```

**Note**: Integer literals can check against any integer type they fit in.

### Coercion Rules

1. **Exact match**: `T <= T`
2. **Numeric widening**: `u8 <= u16 <= u32 <= u64 <= int`, `i8 <= i16 <= i32 <= i64 <= int`
3. **Data coercion**: Any type `T <= data`

## Statement Typing

### Statement: Let
```
let x: T = e
-------------
e <= T
x : T in subsequent context

let x = e
----------
e => T
x : T in subsequent context
```

### Statement: Var
```
var x: T = e
-------------
e <= T
x : T in subsequent context (mutable)

var x = e
----------
e => T
x : T in subsequent context (mutable)
```

### Statement: Set
```
x : T in context
e <= T
---------------
set x = e
```

### Statement: Fun
```
params have types T1, T2, ...
return type is R (or () if omitted)
body type checks with params in scope
-------------------------------------
fun name(p1: T1, p2: T2, ...) -> R { body }
name : (T1, T2, ...) -> R in subsequent context
```

### Statement: Ret
```
e <= R (where R is function's return type)
------------------------------------------
ret e

function has no declared return type
------------------------------------
ret  (bare return for void functions)
```

### Statement: If

#### Boolean condition
```
condition <= bool
then_body type checks
else_body type checks (if present)
----------------------------------
if condition { then_body } else { else_body }
```

#### Option destructuring
```
condition => Option<T>
binding : T in then_body scope
then_body type checks
else_body type checks (if present)
----------------------------------
if let some(binding) = condition { then_body } else { else_body }
```

#### Result destructuring
```
condition => Result<T>
binding : T in then_body scope
err_binding : error in else_body scope
else_body required
then_body type checks
else_body type checks
----------------------------------
if let ok(binding) = condition { then_body } else error(err_binding) { else_body }
```

### Statement: Loop
```
carry bindings: c1: T1 = e1, c2: T2 = e2, ...
bring bindings: b1: U1, b2: U2, ... (require type hints)
condition <= bool (if present, checked after carries bound)
body type checks with carries in scope
break values <= bring types
continue values <= carry types
loops with carries must not fall through (all paths must break/continue/return)
-------------------------------------------------------------------------------
loop carry c1: T1 = e1, ... bring b1: U1, ... while condition { body }
b1: U1, b2: U2, ... in subsequent context
```

### Statement: Break
```
inside loop with bring bindings b1: U1, ...
values.len() == brings.len()
for all i: vi <= Ui
-------------------------------------------
break v1, v2, ...
```

### Statement: Continue
```
inside loop with carry bindings c1: T1, ...
values.len() == carries.len()
for all i: vi <= Ti
-------------------------------------------
continue v1, v2, ...
```

### Statement: DebugLog
```
e => T (any type)
-----------------
debuglog e
```

## Function Typing Context

Functions establish a typing context:

- **expected_return_type**: The declared return type (or `()` if void)
- **is_void_function**: Whether the function has no declared return type
- **parameters**: Added to variable context with declared types
- **loop_contexts**: Stack for validating break/continue

Try operators (`?`, `!`) and checked/optional arithmetic require:
- Being inside a function (`expected_return_type` is set)
- Return type matches operator requirement (Option for `?`/`+?`, Result for `!`/`+!`)

## Type Error Codes

### Expression Errors
- **F001**: Undefined variable
- **F002**: Undefined function
- **F011**: Cannot synthesize type
- **F016**: Type mismatch
- **F026**: Invalid operand type for operator
- **F027**: Invalid tuple element type (function type in tuple)
- **F045**: Function arity mismatch
- **F046**: Result destructuring requires error binding
- **F047**: Try operator used outside function
- **F048**: Try operator operand type mismatch
- **F049**: Try operator return type mismatch

### Control Flow Errors
- **BreakOutsideLoop**: Break statement outside loop
- **ContinueOutsideLoop**: Continue statement outside loop
- **BreakArityMismatch**: Wrong number of break values
- **ContinueArityMismatch**: Wrong number of continue values
- **LoopBodyFallthrough**: Loop with carries has paths that don't break/continue/return

### Inherited from Datalit
- **TypeMismatch**: Expected type doesn't match actual type
- **HeapMismatch**: Expected heap doesn't match actual heap
- **CannotSynthesize**: Cannot infer type without context
- **IntOutOfRange**: Integer literal out of range for target type
- **ArityMismatch**: Wrong number of tuple/struct fields
- **FieldOrderMismatch**: Struct fields in wrong order
- **VariantNotFound**: Enum variant doesn't exist
- **MissingField/ExtraField**: Struct field errors

## Numeric Type Hierarchy

```
Unsigned: u8 -> u16 -> u32 -> u64 -> int
Signed:   i8 -> i16 -> i32 -> i64 -> int
Float:    f32 (no widening)
```

**No cross-widening**: Unsigned cannot widen to signed or vice versa.

## Operator Type Requirements

| Operator | Allowed Types | Return Type | Context Required |
|----------|---------------|-------------|------------------|
| `+`, `-`, `*` | numeric | int (fixed), T (float/bigint) | - |
| `/` | f32 only | f32 | - |
| `+!`, `-!`, `*!` | fixed int | T | Result return |
| `/!` | fixed int, bigint | T | Result return |
| `+?`, `-?`, `*?` | fixed int | T | Option return |
| `/?` | fixed int, bigint | T | Option return |
| `-` (unary) | f32, bigint | T | - |
| `-!` (unary) | signed fixed int | T | Result return |
| `-?` (unary) | signed fixed int | T | Option return |
| `<`, `>`, etc. | numeric | bool | - |
| `?` | Option<T> | T | Option return |
| `!` | Result<T> | T | Result return |

## Implementation Notes

### Two-Pass Function Processing

Function definitions are processed in two passes:
1. **Signature collection**: Extract parameter types and return type, add to context
2. **Body checking**: Type check function body with parameters in scope

This allows mutual recursion between functions.

### Expression Type Storage

Expression types are stored in a vector indexed by Salsa ID for efficient lookup during code generation.

### Call Target Resolution

Function calls store resolved call targets (AST + module ID) for the interpreter/compiler, eliminating runtime name lookup.

### Module Imports

Imported functions are added to the context with their resolved AST and source module ID. The type signature comes from the source module's exports.
