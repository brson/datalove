## The Datatypes of Datalove Literals

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

Booleans are used in `if` and `else if` conditions.

```datalove
require sys/core/debug

let a = true
let b = false
if a
  debug.print("a")
else if b
  debug.print("b")
else
  debug.print("other")
end if
```

Booleans support logic operators `and`, `or`, `xor`, `implies` and `not`.

```datalove
let a = true
let b = false
let c = a or b
let d = a and  b
let e = a xor b
let f = a implies b
let g = not a
```

`implies` being logical implication,
rarely given a first-class operator but common in logic:

| a     | b     | a implies b |
|-------|-------|-------------|
| false | false | true        |
| false | true  | true        |
| true  | false | false       |
| true  | true  | true        |




## Fixed integers

Standard fixed integers,
`u8`, `u16`, `u16`, `u32`, `u64`,
`i8`, `i16`, `i16`, `i32`, `i64`;
these are efficient and inlineable,
but because Datalove cares insists on numerical correctness,
must be used with checked operators to deal with overflow:

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
Signed fixed integers additionally support unary negation,
`-?` and `-!`.

The "bare" math ops are supported but widen to `int`.

```datalove
fun add(a: u16, b: u16): int
  ret a + b
end fun
```

This compromise allows for convenient but inefficient math in the repl and scripts,
while modules are expected to be more careful with overflow for performance.




## `index` and `offset`

These are fixed integers with the
width used to represent collection indexes.

There are no pointers in datafun,
but the size of collections is constrained by the pointer size.

These have the same representation as `u32` and `i32` by default,
but their representation is compile-time configurable,
mostly to ensure the compiler is well-factored to support
reconfiguration.

Their size must be smaller than the pointer size
of the interpreter, and of any AOT target;
and in practice probably the compiler as well
since it needs to do compile-time evaluation.

Operations that deal in collection indexes,
length, and capacity use these types.

```datalove
let foo = "test"
let foo_length: index = len(foo)
```

Arithmetic operators widen `index` and `offset`
to `int` like other fixed int types.
They otherwise do not automatically coerce
to any other types.




## Big integers

## Floating point numbers

## Anonymous tuples

## Anonymous structs

## Anonymous enums

## Strings

## Lists

## Maps and sets


## Tensors

Current syntax:

```datalove
let data: tensor<f32, 2> = tensor [2 2] [1 2, 3 4]
```


## Tables

Tables, a.k.a. dataframes ala Pandas / Polars / Arrow.
Tables provide "struct-of-array" memory layout.

```datalove
let data: {|
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

One call always use manual linebreaks with `;`:

```
```datalove
let data = {|
  x, t; 1, 2; 3, 4
|}
```

The type reads like a struct;
the expression is a table each row, each an instance of that struct.

Table column projections,
analogous to struct field projections;
have type "list of field type",
but can't be mutated or moved.
They can be passed to reference destinations,
particularly `ref`-mode function arguments.

```datalove
let data: {|
  x: int,
  t: int,
|} = {|
  x, t
  1, 2  
  3, 4
|}

// Binding a column projection to a `ref` slot.
let ref xs = data.x

// This function can accept a table column projection.
fun process(ref xs: [int]): u32
  ret core.list.len(xs)
end fun

let p = process(xs)
```


## Optional types

## Result types

## Existential data types

## Existential error types
