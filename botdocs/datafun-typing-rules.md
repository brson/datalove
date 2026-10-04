# Datafun Typing Rules

This document specifies the type system for datafun (the expression/statement language).
It extends datalit typing rules with functions, control flow, and operations.

## Core Principles

- **Bidirectional**: Expressions either synthesize (=>) or check (<=) against types
- **Extends datalit**: All datalit types are also datafun types
- **Function types**: Functions have types of the form `(T1, T2, ...) -> R`
- **Effect tracking**: Checked/optional operators require compatible function return types
- **No implicit coercion**: A checked expression must have exactly the expected
  type. Numeric widening is written with `@` and wrapping in `data` with
  `data` (botspec Sections 3.5, 10 and 12)

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
    param_types: Vec<Type>,
    param_modes: Vec<ParamMode>,  // In, Out, Ref, Mut
    param_comptime: Vec<bool>,    // Which parameters are `const`
    return_type: Type,
}
```

Function type syntax: `(T1, T2, ...) -> R`

Examples:
```datalove
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
T is float (f32, f64) or bigint (int)
-------------------------------------
e1 + e2 => T
```

**Note**: Bare arithmetic on fixed integers (u8 through u64, i8 through i64,
`index`, `offset`) is an error (F026). Widen the operands to `int` with `@`, or
use the checked or optional operators.

#### Division (/)
```
e1 => T, e2 => T
T is float (f32, f64)
---------------------
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

#### Ordering (.<, .>, <=, >=)
```
e1 => T, e2 => T
T is numeric type
-----------------
e1 .< e2 => bool
```

#### Equality (==, !=)
```
e1 => T, e2 => T
T has equality
-----------------
e1 == e2 => bool
```

A type has equality if it is numeric, `bool`, `string`, unit or an atom, or
if it is an option, tuple, struct, term or enum whose parts all have
equality. Floats inside compare by IEEE 754. Lists, sets, maps, tables,
tensors, results, `data`, `error` and functions do not have equality, and
neither does a type parameter inside another type.

When one operand is a construction with no type of its own (`none`,
`some e`, a tuple or struct literal, an atom or a term) and the other is
not, the construction is checked against the other operand's type.

Comparisons do not chain: `a == b == c` is a parse error (P067).

### Rule: Syn-UnaryOp

#### Negation (-)
```
e => T
T is float (f32, f64) or bigint (int)
-------------------------------------
-e => T
```

**Note**: Bare negation only for floats and bigints (no overflow possible). A
negated integer literal checked against a fixed int is read as a signed
literal, so `let x: i32 = -5` is fine; `-x` for an `i32` variable is not.

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

### Rule: Syn-QualifiedCall
```
m is required as a module or rider whose function f has type (T1, ..., Tn) -> R
args.len() == n
for all i: ei <= Ti
---------------------------------------------------
m.f(e1, e2, ..., en) => R
```

**Note**: Only the `m.f` pairs a unit writes are looked up, gathered by the
parser into `ParsedStatements::qualified_calls`, so a unit depends on the
functions it calls and not on everything its required modules have.

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
----------------------------------
(e1, e2, ..., en) => (T1, T2, ..., Tn)
```

### Literal Synthesis

Literals follow the same rules as datalit:

- `true`, `false` => `bool`
- Integer literals => `int` (arbitrary-precision) by default
- Float literals => `f64` by default (a float literal checks against `f32` or `f64`)
- Hex literals => `int` by default
- String literals => `string`
- `none` => cannot synthesize (requires type context)

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

### Rule: Syn-Atom
```
---------------------
atom Name => Atom(Name)
```

Atom expressions synthesize a standalone atom type.

### Rule: Syn-Term
```
e => T
---------------------
term Name e => Term(Name, T)
```

Term expressions synthesize a standalone term type from the payload.

### Rule: Syn-EnumLiteral
```
---------------------
Cannot synthesize type for enum literal (requires type context)
```

Enum literals (`enum { atom Foo }`) can only be checked, not synthesized.

### Collection Synthesis

Collections follow datalit rules with element type inference:

```
[e1, e2, ...] => [T]          where first element determines T
#{e1, e2, ...} => #{T}
%{k1 = v1, ...} => %{K = V}
```

Empty collections default to unit element type: `[] => [()]`

## Expression Checking Rules

### Rule: Check-Subsume
```
e => T'
T' = T
-------
e <= T
```

There is no subsumption beyond equality: no numeric widening and no implicit
`data`.

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
T' = T
-------
e <= T
```

**Example - Type Propagation:**
```datalove
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
```datalove
// ERROR: +? requires Option return, but script returns Result
let x: u32 = a +? b    // Falls through to synthesis, produces error
```

### Rule: Check-Atom
```
expected is Atom(Name) with same name
--------------------------------------
atom Name <= Atom(Name)

expected is Enum(variants) and Name is atom variant in variants
---------------------------------------------------------------
atom Name <= Enum(variants)
```

### Rule: Check-Term
```
expected is Term(Name, T) with same name
e <= T
--------------------------------------
term Name e <= Term(Name, T)

expected is Enum(variants) and Name is term variant with payload T
e <= T
------------------------------------------------------------------
term Name e <= Enum(variants)
```

### Rule: Check-EnumLiteral
```
expected is Enum(variants)
inner variant checks against expected
--------------------------------------
enum { variant } <= Enum(variants)
```

### Rule: Check-Int
```
n fits in expected integer type
-------------------------------
n <= T (where T is integer type)
```

**Note**: Integer literals can check against any integer type they fit in.

### Coercion Rules

There are none: the only rule is exact match, `T <= T`. Widening is the `@`
operator (botspec Sections 6.8 and 10), which is itself checked against the
expected type, and a value becomes `data` only through `data e`.

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

### Statement: Let, destructuring
```
let (x1, ..., xn) = e
---------------------
e => (T1, ..., Tn)
xi : Ti in subsequent context

let {f1 = x1, ..., fn = xn} = e
-------------------------------
e => {f1: T1, ..., fn: Tn}
xi : Ti in subsequent context

let term N x = e
----------------
e => term N T
x : T in subsequent context

let atom N = e
--------------
e => atom N
```

With a hint, `e <= T` against the hint instead, and the pattern is checked
against the hint. `{f}` is short for `{f = f}`. A struct pattern names every
field, in any order. `let ()` takes apart `()`. The pattern is one level deep,
and an enum is not taken apart here but with `match`. `var` takes the same
patterns, each name a mutable binding of its own. A pattern that does not fit
is F071, reported at the value.

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
fun name(p1: T1, p2: T2, ...): R body end fun
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
if condition then_body else else_body end if
```

#### Option destructuring
```
condition => Option<T>
binding : T in then_body scope
then_body type checks
else_body type checks (if present)
----------------------------------
if condition |binding| then_body else else_body end if
```

#### Result destructuring
```
condition => Result<T>
binding : T in then_body scope
err_binding : error in else_body scope
else_body and err_binding required (F046)
then_body type checks
else_body type checks
----------------------------------
if condition |binding| then_body else |err_binding| else_body end if
```

### Statement: Loop
```
condition <= bool (if present)
body type checks
------------------------------
loop while condition body end loop
```

### Statement: Break
```
inside a loop (F050 otherwise)
------------------------------
break
```

### Statement: Continue
```
inside a loop (F051 otherwise)
------------------------------
continue
```

### Statement: Match
```
input => Enum(variants)
for each case:
  atom Name: Name is atom variant in variants
  term Name binding: Name is term variant with payload T, binding : T in body scope
all variant names covered (or default present)
no duplicate cases
------------------------------------------------------
match input
case atom Name
    body
case term Name binding
    body
case default
    body
end match
```

The input must synthesize an enum type. Each case arm must name a variant in
the enum. Term arms bind the payload to a variable in scope for the arm body.
Without a default arm, the match must be exhaustive.

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

Try operators (`?`, `!`) and checked/optional arithmetic require the return
type to match the operator: Option for `?`/`+?`, Result for `!`/`+!` (F049
otherwise). Script top level returns `!()`, so the Result forms work there and
the Option forms do not.

## Type Error Codes

Every code here is rendered as a diagnostic with a source span. F047 and F056
are unassigned.

### Expression Errors
- **F001**: Undefined variable
- **F002**: Undefined function; also an import or qualified call of a function
  the module or rider does not have
- **F011**: Cannot synthesize type; also an index into something that is not a
  list, map or tensor, whether read or written by `set`
- **F016**: Type mismatch; also an element of an unhinted collection literal
  that differs from the first
- **F026**: Invalid operand type for operator
- **F045**: Function arity mismatch
- **F046**: Result destructuring requires error binding
- **F048**: Try operator operand type mismatch
- **F049**: Try operator return type mismatch; also `set a[i]? = v` outside a
  function returning an option, and `set a[i]! = v` outside one returning a
  result
- **F057**: Argument mode marker does not match the parameter's mode
- **F058**: A const expression names something that is not a const
- **F065**: Integer or hex literal out of range for its type
- **F071**: Destructuring pattern does not fit the value
- **F075**: A tuple, struct, table or tensor literal whose shape does not fit
  its type: the wrong number of elements, fields or columns, fields or
  columns out of order, or the wrong rank
- **F076**: An argument to a const parameter that is not a const binding

### Place Errors
Fields and elements read out of an aggregate, and the targets of `set`.

- **F067**: No such field
- **F068**: Field projection on a type with no fields
- **F069**: Tuple element index out of range
- **F070**: A non-copy field read out of the aggregate that holds it
- **F072**: A non-copy element read out of the collection that holds it
- **F073**: A view into a tensor of rank above one passed `mut` or `out`, or
  assigned with `set`
- **F074**: A bare index (upsert) in a `set` target that is not the last step,
  or not into a map

### Match Errors (reported as F016 type mismatches)
- Non-exhaustive match: missing variant names without default arm
- Unknown variant name in match case
- Atom case used for term variant (or vice versa)
- Match input is not an enum type

### Statement and Function Errors
- **F050** (BreakOutsideLoop): Break statement outside loop
- **F051** (ContinueOutsideLoop): Continue statement outside loop
- **F052**: A void function returns a value
- **F053**: A bare `ret` in a function that returns a value
- **F054**: `set` on an undefined variable
- **F055**: `set` on an immutable variable
- **F063**: A function that can reach the end of its body without returning a
  value

### Declaration Errors
- **F059**: A name imported twice
- **F060**: A type parameter under a collection in a signature, where erasure
  cannot reach it
- **F061**: A type parameter used as a set element or map key without
  `is ord`
- **F062**: A native function declared where no rider implements it
- **F064**: Unknown type name
- **F066**: A type alias defined twice
- **F077**: A const parameter whose type is a type parameter
- **F078**: An import from, or qualified call through, a name that no
  `require` brought in

### Inherited from Datalit
Datalit's own `TypeMismatch`, `CannotSynthesize` and `IntOutOfRange` are
reported as F016, F011 and F065, and its structural `ArityMismatch` and
`FieldOrderMismatch` as F075.

## Operator Type Requirements

| Operator | Allowed Types | Return Type | Context Required |
|----------|---------------|-------------|------------------|
| `+`, `-`, `*` | f32, f64, int | T | - |
| `/` | f32, f64 | T | - |
| `+!`, `-!`, `*!` | fixed int | T | Result return |
| `/!` | fixed int, bigint | T | Result return |
| `+?`, `-?`, `*?` | fixed int | T | Option return |
| `/?` | fixed int, bigint | T | Option return |
| `-` (unary) | f32, f64, int | T | - |
| `-!` (unary) | signed fixed int | T | Result return |
| `-?` (unary) | signed fixed int | T | Option return |
| `.<`, `.>`, `<=`, `>=` | numeric | bool | - |
| `==`, `!=` | numeric, bool, string, unit, atom; option, tuple, struct, term, enum of those | bool | - |
| `?` | Option<T> | T | Option return |
| `!` | Result<T> | T | Result return |

## Implementation Notes

### Two-Pass Function Processing

Function definitions are processed in two passes:
1. **Signature collection**: Extract parameter types and return type, add to context
2. **Body checking**: Type check function body with parameters in scope

This allows mutual recursion between functions.

### Expression Type Storage

Expression types are stored in a `BTreeMap` keyed by `ExprKey`, which is the
expression's module, enclosing function name and sequential index within that
function.

They were once a vector indexed by salsa id. That indexed by the wrong half of
an id, dropping the generation that tells two expressions in a reused slot
apart, and the vector grew to the highest index salsa had handed out - in one
measured case 1416 slots to hold 8 entries. See `salsa-patterns.md`.

### Call Target Resolution

Function calls store resolved call targets (AST + module ID) for the interpreter/compiler, eliminating runtime name lookup.

### Module Imports

Imported functions are added to the context with their resolved AST and source module ID. The type signature comes from the source module's exports.

Qualified calls resolve the same way, into `TypeContext::qualified` rather than the name table, so `u8.from_int` and `i8.from_int` sit side by side. A module's are resolved with its imports in `resolve_module_imports`; a script unit's in `typecheck_script_unit`, through `module_alias_at` for an alias an earlier unit required, adding the module to the unit's `imported_modules`.
