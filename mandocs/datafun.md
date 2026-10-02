# Datalove Functions Primer

_Datalove Functions_ is the side-effect-free sublanguage of Datalove.
We often refer to it as _datafun_.
On top of the data types defined by [Datalove Literals](datalit.md) it adds:

- Pure and (nearly) total functions
  that cannot perform I/O and have no exceptional control-flow.
- A simple, acyclic module system.
- Constants with full compile-time function evaluation.
- Reactive script units that may be chained together, and that incrementally
  recompile and reevaluate as dependent units and modules are updated.

Datafun scripts may be interpreted directly via lowered IR, optionally with per-function JIT,
or may be compiled to statically-linked binaries, either via Cranelift or C.
The Datalove Functions implementation is self-contained and independent from full Datalove,
suitable as a constrained embedded application scripting language.

This document is a broad overview of the language.
For detail see additional documentation.




## Contents



## Data types

Datalove functions operate on the [datalit](datalit.md) types,
which are structuaral and linear.
They are in brief:

Primitives: `bool`, `int`, `f64`, `f32`, `string`,
`u8` .. `u64`, `i8` .. `i64`, `index`, `offset`.

Collections:

- list, `[T]`
- map, `%{ K = V }`
- set, `#{ K }`
- tables `{\| col1: T1, col2: T2 \|}`
- tensor, `[\|T, N\|]`

Aggregates:

- unit, `()`
- 1-tuple, `(T1,)`
- n-tuple, `(T1, T2)`
- struct, `{ x: T1, y: T2}`
- option, `?T`
- result, `!T`
- atom, `atom Foo`
- term, `term Foo T`
- enum, `enum { atom A, term B T }`

As well as the dynamic types, `data` and `error`.

Types can be given names with the `type` statement.
These are called _type aliases_ and do not create new types.
They can be used to name the type but not construct it.
They are spelled with `SnakeCase` by convention.

```datalove
type Shape: enum {
  atom Circle,
  term Rect (f32, f32),
}

fun debug_shape(my_shape: Shape)
  debuglog my_shape
end fun

debug_shape(atom Circle)
```




## Functions

Functions have a line-oriented and statement-oriented syntax.

```datalove
require module sys/std/string
import string.len
import string.find_char
import string.starts_with
import string.slice_from

fun count_substrings(s: string, ref needle: string): ?int
  var haystack = s
  var count = 0
  loop
    if len(ref haystack) == 0
      break
    end if

    if starts_with(ref haystack, ref needle)
      set count = count + 1
    end if

    let next_char_index = find_char(ref haystack, 1)
    if next_char_index |index|
      set haystack = slice_from(ref haystack, index)?
    else
      set haystack = ""
    end if
  end loop
  ret some count
end fun
```

Lines can break freely between matched braces of all kinds
(`( .. )`, `{ .. }`, `< .. >` and others).

```datalove
fun count_substrings(
  s: string, ref needle: string,
): ?{
  count: int, other_flags: u8,
}
  // ... etc ...

  ret some {
    count: count,
    other_flags: 0x00,
  }
end fun
```

Function arguments are either passed by value, by reference (`ref`),
by mutable reference (`mut`), or as `out` paramaters.
The caller must correspondingly indicate the passing mode with `ref`, `mut` or `out`.

```datalove
fun demo_param_modes(
  by_value: int,
  ref by_ref: int,
  mut by_mut: int,
  out by_out: int,
)
  set by_mut = by_value + by_ref
  set by_out = by_value * by_ref
end fun

let a = 3
var b = 4
var c: int
demo_param_modes(2, ref a, mut b, out c)

debuglog (b, c)
```

Immutable bindings are declared with `let`, mutable with `var`.
Mutable bindings are reassigned with `set`.
New bindings may shadow previous bindings.

```datalove
let a = true
debuglog a
let a = 100
debuglog a
```




## Control flow

Loops are written with the `loop` keyword, `break` and `continue`.
Basic conditional control flow is performed with `if`.

```datalove
require module sys/std/int

import int.rem_checked

var counter = 10
var evens = 0
var odds = 0

loop
  if counter == 0
    break
  else if rem_checked(counter, 2) == some 0
    set evens = evens + 1
  else
    set odds = odds + 1
  end if
  set counter = counter - 1
end loop

debuglog (evens, odds)
```

Conditional loops are written with `loop while`.

```datalove
require module sys/std/int

import int.rem_checked

var counter = 10
var evens = 0
var odds = 0

loop while counter != 0
  if rem_checked(counter, 2) == some 0
    set evens = evens + 1
  else
    set odds = odds + 1
  end if
  set counter = counter - 1
end loop

debuglog (evens, odds)
```




## Data types and destructuring

Datalove types and values are generally spelled the same way,
or at least in predictably similar ways,
so e.g. the set type is `#{ K }` and its constructor is `#{ 1, 2, 3}`.
Similarly, aggregate data types can generally be destructured
with spellings similar to their constructors,
with struct-like types (product types) destructuring directly
into `let` bindings, and option/result and enums (sum types)
using specialized control flow constructs.

```datalove
let (a, b) = (true, 100)

let {a, b} = {a = true, b = 100}

// Binding the fields to new names.
let {
  a = my_a,
  b = my_b,
} = {a = true, b = 100}

debuglog (my_a, my_b)

let term Foo x = term Foo "bar"
```

Values can also be destructured into `var` bindings,
making each binding individually mutable.

```datalove
var (a, b) = (true, 100)
set a = false
set b = 200
debuglog (a, b)
```

The option and result types are destructured
with special `if` statements.

```datalove
let maybe_label = some "report"

if maybe_label |label_text|
  debuglog label_text
else
  debuglog "no label"
end if

let report_status = ok "sent"

if report_status |status|
  debuglog ("report status ok", status)
else |e|
  debuglog ("report status error", e)
end if
```

These forms disallow `else`-`if` chains.
In the result case the `else` branch is required.

Enums are destructured with `match`.

```datalove
type Shape: enum {
  atom Point,
  term Rect (f32, f32),
}

let s: Shape = term Rect (3.0, 4.0)

var area: f32 = 0.0
match s
case atom Point
  set area = 0.0
case term Rect dims
  set area = dims.0 * dims.1
end match
```

Match must be exhaustive;
Use `case default` for a catch-all.

```datalove
match s
case atom Point
  set area = 0.0
case default
  set area = 1.0
end match
```




## Option and result handling

Optional and result types are first-class in the language,
spelled `?T` and `!T`. The question mark and bang sigils
are reserved solely for types operations involving
optional values and error handling.

Optionals contain an optional payload, spelled `some x`,
or they are empty, spelled `none`. Results are for handling
fallible operations, and are either `ok x` or `er e`,
where the type of `e` must by the dynamic `error` type.

```datalove
let maybe_text1: ?string = some "status info"
let maybe_text2: ?string = none
let result_text1: !string = ok "status info"
let result_text2: !string = er error "failed to load"
```

As described previously their payloads are accessed
through destructuring `if` statements.

```datalove
require module sys/std/u32

import u32.add_checked
import u32.max_value

fun add_saturating(self: u32, other: u32): u32
  if add_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

debuglog(: u32 / 100, max_value())
```

The postfix `?` and `!` operators propagate
option and result return types.

```datalove
require module sys/std/u32

import u32.add_checked

fun add_twice(self: u32, other: u32): ?u32
  let once: u32 = add_checked(self, other)?
  let twice: u32 = add_checked(once, other)?
  ret some twice
end fun

debuglog(add_twice(:u32 / 1, : u32 / 2))
```

With `!`:

```datalove
require module sys/std/u32
require module sys/std/option

import u32.add_checked
import option.ok_or

fun add_twice(self: u32, other: u32): !u32
  let once: u32 = ok_or(add_checked(self, other), error "overflow")!
  let twice: u32 = ok_or(add_checked(once, other), error "overflow")!
  ret ok twice
end fun

debuglog(add_twice(: u32 / 1, : u32 / 2))
debuglog(add_twice(: u32 / 1, : u32 / 4000000000))
```




## Numerics

todo

Integer literals have the bigint `int` type by default.
To create integers of other types use a prefix
type hint of the form `: <type> / <expr>`,
which can be applied to any expression.

```datalove
fun u32_zero(): u32
  ret : u32 / 0
end fun
```

Specifying the type in an intermediate `let` binding
also works.

```datalove
fun u32_zero(): u32
  let zero: u32 = 0
  ret zero
end fun
```




## Comparison and equality

## Other operators

## The adapt operator: `@`

## Constants and compile-time evaluation

## Modules and their organization

modules, packages, libraries

## The standard library

## Scripts

## Interactive script units

## Workspaces
