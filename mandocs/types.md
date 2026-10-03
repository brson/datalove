# The Datatypes of Datalove

Datalove's datatypes, their representation,
and semantics are focused on readability,
completeness, numerical correctness, and predictability.

Datalove Literals is a declarative
typed language for describing pure data —
it has no first-class pointers or object identity.
It includes scalar values,
tuples, structs, and enums,
and a rich set of collection types:
_lists_, _maps_, _sets_,
_tensors_ (multi-dimensional arrays),
and _tables_ (dataframes / structs-of-arrays).

Most types in Datalove are Datalove Literal types,
and we call them _datalit types_.
In some languages they might be called "plain old data".

Datalit types are value types and stored inline.
Datalit types are structurally typed.



## Unit

Spelled `()`, which can also be considered a zero-element anonymous tuple.
This is the implicit return type of void functions.

Note that `{}`, the empty anonymous struct,
has identical representation,
but is not used as the unit type.




## Booleans

The `bool` type has two values, `true` and `false`.

```datalove
let a: bool = true
let b: bool = false
```

Booleans are used in `if`, `else if`, and `loop while` conditions.

```datalove
let a = true
let b = false
if a
  debuglog "a"
else if b
  debuglog "b"
else
  debuglog "other"
end if
```

Booleans support logic operators `and`, `or`, `xor`, and `not`.

```datalove
let a = true
let b = false
let c = a or b
let d = a and b
let e = a xor b
let f = not a
```




## Fixed integers

Standard fixed integers,
`u8`, `u16`, `u32`, `u64`,
`i8`, `i16`, `i32`, `i64`;
these are efficient and inlineable,
but because Datalove insists on numerical correctness,
they must be used with checked operators to deal with overflow:

```datalove
fun add_result(a: u16, b: u16): !u16
  let c = a +! b // early-return error on overflow
  ret ok c
end fun

fun add_option(a: u16, b: u16): ?u16
  let c = a +? b // early-return option on overflow
  ret some c
end fun
```

Unsigned fixed integers support
`+?`, `-?`, `*?` and `/?` checked-option binops, and
`+!`, `-!`, `*!` and `/!` checked-result binops.
Signed fixed integers additionally support checked unary negation,
`-?` and `-!`.
Division by zero is treated like overflow.

The "bare" math ops `+`, `-`, `*` and `/`, and bare unary `-`,
are not defined on fixed integers.
Instead, operands can be explicitly widened to `int` with the `@` operator,
which takes its target type from context:

```datalove
fun add(a: u16, b: u16): int
  ret a@ + b@
end fun
```

This compromise allows for convenient but inefficient math in the repl and scripts,
while modules are expected to be more careful with overflow for performance.

Bare integer literals are `int`,
so fixed integer literals need a type hint or checking context:

```datalove
let a = : u32 / 42
let b: i8 = -1
```

`@` also widens between fixed integers,
when the target can hold every value of the source:
`u8` to `u16`, `u8` to `i16`, and so on.
There is no implicit widening.




## `index` and `offset`

These are fixed integers with the
width used to represent collection indexes.

There are no pointers in datafun,
but the size of collections is constrained by the pointer size.

These have the same representation as `u32` and `i32` by default,
but their representation is compile-time configurable
(the `index-64` feature makes them `u64` and `i64`),
mostly to ensure the compiler is well-factored to support
reconfiguration.

Their size must be no larger than the pointer size
of the interpreter, and of any AOT target;
and in practice probably the compiler as well
since it needs to do compile-time evaluation.

Operations that deal in collection indexes,
length, and capacity use these types.

```datalove
require module sys/std/string
import string.len

let foo = "test"
let foo_length: index = len(ref foo)
```

Like other fixed int types,
`index` and `offset` support the checked and optional operators,
not the bare ones,
and widen to `int` with `@`.
They do not automatically coerce
to any other types.




## Big integers

`int` is an arbitrary-precision integer,
and the type of bare integer literals.
It supports the bare `+`, `-` and `*` operators, and unary `-`,
none of which can overflow.
Division can fail on a zero divisor,
so there is no bare `/`;
use `/?` or `/!`, which yield `int`.
Division truncates toward zero.

```datalove
let a = 123456789012345678901234567890
let b = a * a - 1
```

`int` is heap-allocated and so, unlike the fixed integers,
is moved rather than copied.

## Floating point numbers

`f32` and `f64` are IEEE 754 floats.
A literal is a float if it has a point or an exponent,
and bare float literals are `f64`.

```datalove
let a = 3.14
let b: f32 = 1.5
let c = 6.022e23
```

Floats support the bare `+`, `-`, `*` and `/` operators,
yielding the same float type.
An integer literal is not a float:
`let x: f32 = 1` is an error,
except that a hex literal is read as the float's bit pattern.

## Anonymous tuples

Tuples are ordered, fixed-size, heterogeneous sequences.
Elements are accessed by position.
A one-element tuple needs a trailing comma.

```datalove
let t: (bool, int) = (true, 42)
let a = t.0
let one = (1,)
```

## Anonymous structs

Structs are sequences of named fields.
Fields are accessed by name.

```datalove
let p: { x: f32, y: f32 } = { x = 1.0, y = 2.0 }
let x = p.x
```

Field order is significant:
`{ x: f32, y: f32 }` and `{ y: f32, x: f32 }` are different types,
and a literal must list its fields in the order of its type.

## Atoms

Atoms are named unit values that introduce a name component to the structural type system.

Two atoms with the same name are type-compatible;
two atoms with different names are not.

```datalove
let a = atom Foo
let b: atom Foo = atom Foo

var c = atom Foo
set c = atom Foo     // ok - same name
set c = atom Bar     // error - different name
```


## Terms

Terms are like atoms but carry a typed value,
parsed as a primary expression (no binops).

The name and the inner type must both match for type compatibility.

```datalove
let a = term Foo 1
let a = term Foo (1,)     // tuple payload
let a = term Bar 1
let b = term Baz #{ 1 }

let d: term Foo int = term Foo 1

// The name and type must match.
var c = term What [1]
set c = term What [1, 2]
```


## Anonymous enums

Anonymous enums are sets of atom and term types
where all the names are unique.

```datalove
type MyEnum: enum {
  atom Foo,
  term Bar int,
  term Baz (f32, f32),
}

// Full enum literal form (requires checking context)
let a: MyEnum = enum { atom Foo }

// Bare atom and term literals check against an enum directly
let b: MyEnum = atom Foo
let c: MyEnum = term Bar 1

// Coercion with @, for atom and term values that are not literals
let foo = atom Foo
let d: MyEnum = foo@
```

Only `atom` and `term` types are allowed in enums.
The full enum literal form does not synthesize a type;
it must be in a checking context.
`@` does not widen one enum to another enum with more variants.

Enums are destructured with `match`; see types-lits-destr.md.

## Strings

`string` is a UTF-8 string.
String literals are double-quoted and may contain
the escapes `\"`, `\\`, `\n`, `\r`, `\t`, `\0`, and `\u{...}`.

```datalove
let s = "hello\tworld\u{21}"
```

Strings are heap-allocated, so moved rather than copied.
Operations are in `sys/std/string`.

## Lists

`[T]` is a growable, ordered sequence of `T`.

```datalove
let a: [u32] = [1, 2, 3]
```

Indexing is by `index` and is fallible:
`a[i]?` early-returns `none` and `a[i]!` early-returns an error
when the index is out of bounds.
Operations are in `sys/std/list`.

## Maps and sets

`%{K = V}` maps keys to values
and `#{T}` is a set of unique values.
Both are ordered by key.

```datalove
let m: %{string = u32} = %{ "a" = 1, "b" = 2 }
let s: #{int} = #{ 3, 1, 2 }
```

Maps are indexed by key, fallibly, like lists: `m["a"]?`.
Operations are in `sys/std/map` and `sys/std/set`.


## Tensors

Tensors are multi-dimensional arrays.
The type gives the element type and the rank (number of axes);
the shape is part of the value.

```datalove
let t: [|f32, 2|] = [| 1.0 2.0, 3.0 4.0 |]
```

In a literal, whitespace separates elements along the innermost axis,
`,` separates rows, `,,` separates slabs of the third axis, and so on.
`t[i]?` indexes along the first axis.


## Tables

Tables, a.k.a. dataframes ala Pandas / Polars / Arrow.
Tables provide "struct-of-array" memory layout.

```datalove
let tbl: {|
  x: int,
  t: int,
|} = {|
  x, t       // column names are required
  1, 2
  3, 4
|}
```

Note that the `{|` opening bracket enters a line-oriented
parsing context, one row per line.
This allows a natural CSV-like presentation for tables,
taking advantage of Datalove's mixed-mode brace-matched parser.

One can always use manual linebreaks with `;`:

```datalove
let tbl = {| x, t; 1, 2; 3, 4 |}
```

The type reads like a struct;
the expression is a table of rows, each an instance of that struct.

Column projections are not implemented yet;
a table is currently opaque.
Table column projections,
analogous to struct field projections;
have type "list of field type",
but can't be mutated or moved.
They can be passed to reference destinations,
particularly `ref`-mode function arguments.

```datalove
require module sys/std/list
import list.len

let tbl: {|
  x: int,
  t: int,
|} = {|
  x, t
  1, 2
  3, 4
|}

// This function can accept a table column projection.
fun process(ref xs: [int]): index
  ret len(ref xs)
end fun

let p = process(ref tbl.x)
```


## Optional types

`?T` is either `some` value of `T` or `none`.

```datalove
let a: ?int = some 1
let b: ?int = none
```

The postfix `?` operator unwraps an option,
early-returning `none` from the enclosing function if there is no value.
`if o |v|` binds the value if there is one.

## Result types

`!T` is either `ok` with a value of `T` or `er` with an `error`.

```datalove
let a: !int = ok 1
let b: !int = er error "oops"
```

The postfix `!` operator unwraps a result,
early-returning the error from the enclosing function on failure.
`if r |v| ... else |e|` binds either the value or the error.

## Dynamic types

`data` can hold a value of any type.
A value only becomes `data` when explicitly wrapped:

```datalove
let a: data = data 1
let b: data = data : u32 / 2
```

There is not yet any way to inspect or unwrap a `data` value.

`error` is the error type carried by results,
and like `data` can hold a value of any type,
most commonly a string.

```datalove
let e: error = error "oops"
let r: !int = er error "oops"
```

There is not yet any way to inspect or unwrap an `error` value.
