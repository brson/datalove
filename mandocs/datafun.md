# Datalove Functions Primer

_Datalove Functions_ is the side-effect-free sublanguage of Datalove.
We often refer to it as _datafun_.
On top of the data types defined by [Datalove Literals](datalit.md) it adds:

- Pure functions that cannot perform I/O and have no exceptional control-flow.
- Constants with full compile-time function evaluation.
- A simple acyclic module system.
- Reactive script units that may be chained together, and that incrementally
  recompile and reevaluate as dependent units and modules are updated.

Datafun scripts may be interpreted directly via lowered IR, optionally with per-function JIT,
or may be compiled to statically-linked binaries, either via Cranelift or C.
The Datalove Functions implementation is self-contained and independent from full Datalove,
suitable as a constrained embedded application scripting language.

This document is a broad overview of the language.
For detail see additional documentation.




## Contents

<div class="toc toc-repeat-8">

- [A first script](#user-content-a-first-script)
- [Data types](#user-content-data-types)
- [Functions](#user-content-functions)
- [Control flow](#user-content-control-flow)
- [Data types and destructuring](#user-content-data-types-and-destructuring)
- [Option and result handling](#user-content-option-and-result-handling)
- [Numerics](#user-content-numerics)
- [Comparison and equality](#user-content-comparison-and-equality)
- [The adapt operator: `@`](#user-content-the-adapt-operator-)
- [Constants and compile-time evaluation](#user-content-constants-and-compile-time-evaluation)
- [Modules, packages, and libraries](#user-content-modules-packages-and-libraries)
- [The system library and the `std` package](#user-content-the-system-library-and-the-std-package)
- [Scripts](#user-content-scripts)
- [Interactive script units](#user-content-interactive-script-units)
- [Workspaces](#user-content-workspaces)

</div>



## A first script

Datafun programs are either _modules_, `.dfm` files that define functions for others to use,
or _scripts_, `.dfs` files that also contain top-level statements,
run in order from top to bottom.
Every example in this document is a script.

```datalove
// Load a module from the standard library.
require module sys/std/string

// Bring one of its functions into scope by name.
import string.to_uppercase

let greeting = "hello, world"
let shout = to_uppercase(ref greeting)

// Or call it qualified by the module name, without importing.
let size = string.len(ref greeting)

debuglog (greeting, shout, size)
```

Scripts are run with the `datalove` command.

```
$ datalove script hello.dfs
("hello, world", "HELLO, WORLD", 12)
```

Since datafun cannot perform I/O,
`debuglog` is the only way for a script to produce output.
It prints any value.

Comments begin with `//` and run to the end of the line.

`require module` loads a module by its path,
here the `string` module of the `std` package in the `sys` library.
Its functions are then called either qualified by the module name, as in `string.len`,
or by bare name after an `import`.
A name can only be imported once per scope,
so when two modules export functions of the same name,
as `u8.from_int` and `i8.from_int`,
at least one must be called qualified.
Modules are covered further in [Modules, packages, and libraries](#user-content-modules-packages-and-libraries).

The `ref` before `greeting` passes the string by reference
instead of moving it into the function.
This is described in [Ownership](#user-content-ownership).


## Data types

Datalove functions operate on the [datalit](datalit.md) types,
which are structural and linear. They are in brief:

_Primitives_

| Type              | Literal          |
|-------------------|------------------|
| `bool`            | `true`, `false`  |
| `int`             | `42`             |
| `f64`             | `3.14`, `1.0e10` |
| `f32`             | `: f32 / 3.14`   |
| `string`          | `"hello"`        |
| `u8` .. `u64`     | `: u32 / 42`     |
| `i8` .. `i64`     | `: i32 / -1`     |
| `index`           | `: index / 0`    |
| `offset`          | `: offset / 0`   |

_Collections_

| Name   | Type                             | Literal                          |
|--------|----------------------------------|----------------------------------|
| list   | `[T]`                            | `[1, 2, 3]`                      |
| map    | `%{ K = V }`                     | `%{ 0 = 5, 1 = 2 }`              |
| set    | `#{ K }`                         | `#{ 1, 2, 3 }`                   |
| table  | `{\| col1: T1, col2: T2 \|}`     | `{\| col1, col2; 1, 2; 3, 4 \|}` |
| tensor | `[\|T, N\|]`                     | `[\| 1 2 3, 4 5 6 \|]`           |

_Aggregates_

| Name    | Type                          | Literal                       |
|---------|-------------------------------|-------------------------------|
| unit    | `()`                          | `()`                          |
| 1-tuple | `(T1,)`                       | `(true,)`                     |
| n-tuple | `(T1, T2)`                    | `(true, 42)`                  |
| struct  | `{ x: T1, y: T2 }`            | `{ x = 1, y = 2 }`            |
| option  | `?T`                          | `some 1` <br> `none`          |
| result  | `!T`                          | `ok 1` <br> `er error 2`      |
| atom    | `atom Foo`                    | `atom Foo`                    |
| term    | `term Foo T`                  | `term Foo 1`                  |
| enum    | `enum { atom A, term B T }`   | `enum { atom A }`             |

As well as the dynamic types, `data` and `error`.

Types can be given names with the `type` statement.
These are called _type aliases_ and do not create new types.
They can be used to name the type but not construct it.
They are spelled with `PascalCase` by convention.

```datalove
type Shape: enum {
  atom Circle,
  term Rect (f32, f32),
}

fun debug_shape(my_shape: Shape)
  debuglog my_shape
end fun

call debug_shape(atom Circle)
```




## Variables

Immutable bindings are declared with `let`.
They must be assigned at declaration.
Mutable bindings are declared with `var`,
and reassigned via `set` statements.
`var`s may be unassigned initially,
in which case a type annotation is required.
Static analysis ensures that values are written to all bindings before they are read.

```datalove
let a = 10.0
var b = 20.0
var c: f64

set c = 30.0

debuglog (a, b, c)

set c = 40.0

debuglog (a, b, c)
```

Shadowing is allowed.

```datalove
let a = 10.0
let a = "shady"

debuglog a
```




## Functions

Functions have a line-oriented and statement-oriented syntax.
They are declared with `fun` and closed with `end fun`.
Parameters and return types are always annotated,
and values are returned with an explicit `ret`.

```datalove
fun average(a: f64, b: f64): f64
  ret (a + b) / 2.0
end fun

let avg = average(3.0, 4.0)

debuglog avg
```

Lines can break freely between matched braces of all kinds
(`( .. )`, `{ .. }`, `< .. >` and others).

```datalove
require module sys/std/f64

fun average(
  a: f64, b: f64
): {
  mean: f64,
  stddev: f64,
}
  ret {
    mean = (a + b) / 2.0,
    stddev = f64.abs(a - b) / 2.0,
  }
end fun

let avg = average(
  3.0, 4.0
)

debuglog avg
```

Function arguments are either passed by value, by reference (`ref`),
by mutable reference (`mut`), or as `out` parameters.
The caller must correspondingly indicate the passing mode with `ref`, `mut` or `out`.

```datalove
fun demo_param_modes(
  by_value: int,
  ref by_ref: int,
  mut by_mut: int,
  out by_out: int,
): int
  set by_mut = by_value + by_ref
  set by_out = by_value - by_ref
  ret by_value * by_ref
end fun

let a = 3
var b = 4
var c: int
let d = demo_param_modes(2, ref a, mut b, out c)

debuglog (b, c, d)
```

All statements begin with a reserved word.
In statement position functions are called with `call`.
In expression position they are called by name.

```datalove
fun min_value(a: u32, b: u32): u32
  if a <= b
    ret a
  else
    ret b
  end if
end fun

let min = min_value(1, 2)

fun set_min_value(a: u32, b: u32, out target: u32)
  if a <= b
    set target = a
  else
    set target = b
  end if
end fun

var min: u32
call set_min_value(1, 2, out min)
```




## Ownership

Datalove has a linear type system where all values
are uniquely owned, not reference counted or garbage collected.

Types that do not contain heap allocations are automatically copied when used,
leaving the original value in place to be reused.
Types that contain heap allocations are instead moved into their new location,
statically invalidating the original location.

```datalove
fun print_copy_values(a: f64, b: f64, c: f64)
  debuglog (a, b, c)
end fun

let a = 10.0
let b = a
let c = a

call print_copy_values(a, b, c)
```

The following does not compile because strings
are non-copyable, thus `let b = a` moves `a` into `b`
and `let c = a` cannot access `a` because it is moved.

```datalove
fun print_move_values(a: string, b: string, c: string)
  debuglog (a, b, c)
end fun

let a = "ten"
let b = a
let c = a

call print_move_values(a, b, c)
```

Attempting to run the above produces an error:

```
$ datalove script test.dfs
[D001] Error: use of moved value: `a`
   ╭─[test.dfs:7:9]
   │
 6 │ let b = a
   │         ┬
   │         ╰── value moved here
 7 │ let c = a
   │         ┬
   │         ╰── value used after move
   │
   │ Help: insert `@` to clone:
 6 │    let b = a@
   │             +
───╯

< ... other errors elided ... >
```

The postfix _adapt_ operator, `@`, produces a _clone_
of the value, leaving the original in place.
All first-class types in datafun &mdash;
those that can be accepted as arguments to functions &mdash;
are clonable.


```datalove
fun print_move_values(a: string, b: string, c: string)
  debuglog (a, b, c)
end fun

let a = "ten"
let b = a@
let c = a@

call print_move_values(a, b, c)
```

The adapt operator is a multipurpose tool
that performs lossless type coercions of various kinds.
Whereas Datalove in general is strict and explicit
about data correctness and execution semantics,
the adapt operator is a singular "magic" operator
that does whatever is necessary to fit a value
of one compatible type into another.
Beyond cloning it also performs widening numeric conversions and more.

When `@` solves a compiler error, the compiler will say exactly where to write it.
In some cases Datalove can optionally compile in an "auto-adapt" mode
for increased ergonomics.

The `ref`, `mut` and `out` parameter modes _borrow_ values temporarily
instead of moving them.

```datalove
require module sys/std/string

fun concat_borrow(ref a: string, ref b: string, ref c: string): string
  var all = ""
  call string.push_str(mut all, ref a)
  call string.push_str(mut all, ref b)
  call string.push_str(mut all, ref c)
  ret all
end fun

let a = "a"
let b = "b"
let c = "c"

let d = concat_borrow(ref a, ref b, ref c)

// Can still access a, b and c
debuglog (a, b, c, d)
```

Borrowed values in Datalove are not first class types
and cannot be e.g. named as fields of structs.
Values can not be moved out of borrowed locations;
they must be cloned to obtain a movable instance.

```datalove
fun print_ref_values(ref a: string, ref b: string, ref c: string)
  // Putting these strings into a tuple requires a clone
  debuglog (a@, b@, c@)
end fun

let a = "ten"
let b = a@
let c = a@

call print_ref_values(ref a, ref b, ref c)
```




## Control flow

Loops are written with the `loop` keyword, `break` and `continue`.
Basic conditional control flow is performed with `if`.

```datalove
require module sys/std/int

var counter = 10
var evens = 0
var odds = 0

loop
  if counter == 0
    break
  else if int.rem_checked(counter@, 2) == some 0
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

var counter = 10
var evens = 0
var odds = 0

loop while counter != 0
  if int.rem_checked(counter@, 2) == some 0
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
are reserved solely for operations involving
optional values and error handling.

Optionals contain an optional payload, spelled `some x`,
or they are empty, spelled `none`. Results are for handling
fallible operations, and are either `ok x` or `er e`,
where the type of `e` must be the dynamic `error` type.

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

fun add_saturating(self: u32, other: u32): u32
  if u32.add_checked(self, other) |value|
    ret value
  else
    ret u32.max_value()
  end if
end fun

debuglog(add_saturating(: u32 / 100, u32.max_value()))
```

The postfix `?` and `!` operators propagate
option and result return types.

```datalove
require module sys/std/u32

fun add_twice(self: u32, other: u32): ?u32
  let once: u32 = u32.add_checked(self, other)?
  let twice: u32 = u32.add_checked(once, other)?
  ret some twice
end fun

debuglog(add_twice(: u32 / 1, : u32 / 2))
```

With the result type, `!`:

```datalove
require module sys/std/u32
require module sys/std/option

fun add_twice(self: u32, other: u32): !u32
  let once: u32 = option.ok_or(u32.add_checked(self, other), error "overflow")!
  let twice: u32 = option.ok_or(u32.add_checked(once, other), error "overflow")!
  ret ok twice
end fun

debuglog(add_twice(: u32 / 1, : u32 / 2))
debuglog(add_twice(: u32 / 1, : u32 / 4000000000))
```

Putting these together,
this function counts the occurrences of one string in another,
propagating `none` with `?` if slicing the string ever fails.

```datalove
require module sys/std/string

fun count_substrings(s: string, ref needle: string): ?int
  var haystack = s
  var count = 0
  loop
    if string.len(ref haystack) == 0
      break
    end if

    if string.starts_with(ref haystack, ref needle)
      set count = count + 1
    end if

    let next_char_index = string.find_char(ref haystack, 1)
    if next_char_index |index|
      set haystack = string.slice_from(ref haystack, index)?
    else
      set haystack = ""
    end if
  end loop
  ret some count
end fun

let needle = "an"

debuglog count_substrings("banana", ref needle)
```




## Numerics

Datalove has fixed-width integer types,
`u8` .. `u64` and `i8` .. `i64`,
and unbounded big integer types, `int`.

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

Floating point literals are always written with a decimal
and produce `f64` unless hinted with `f32`.

```datalove
fun f64_zero(): f64
  ret 0.0
end fun

fun f32_zero(): f32
  ret : f32 / 0.0
end fun
```

Numeric types never automatically coerce between types,
neither truncating nor widening.
Widening conversions are performed with the multi-purpose adapt operator, `@`.

```datalove
let a: u8 = 10
let a: u32 = a@

let b: i8 = -10
let b: i32 = b@

let c: u64 = 10
let c: int = c@

let d: f32 = 10.0
let d: f64 = d@
```

Lossy conversions are performed with type-specific library functions.

```datalove
require module sys/std/u8

let fits: ?u8 = u8.from_u64(100)
let dontfits: ?u8 = u8.from_u64(1000)

debuglog (fits, dontfits)
```

Datalove supports basic numerical binops for
addition, subtraction, multiplication, division, and unary negation.
For floating point types these work according to IEEE spec,
with some operations producing `NaN` or +/- infinity.

Because Datalove prioritizes numerical correctness,
silent overflow and divide-by-zero is not allowed for integers.
Thus none of the bare math binops work on fixed-sized integers.
Instead these types must use checked versions of the binops
which early return from their enclosing function with either `none` or `er`.

```datalove
fun do_some_math_opt(a: u32, b: u32, c: u32): ?u32
  ret some ((a +? b) /? c)
end fun

fun do_some_math_result(a: u32, b: u32, c: u32): !u32
  ret ok ((a +! b) /! c)
end fun
```

Bigints directly support `+`, `-` and `*`, but still division requires
a checked operator.

```datalove
fun do_some_big_math(a: int, b: int, c: int): ?int
  ret some ((a + b) /? c)
end fun
```

For further control over math operations,
wrapping and saturating versions are provided as library functions.

```datalove
require module sys/std/u8

let v = u8.add_wrapping(255, 1)

debuglog(v)
```




## Comparison and equality

## The adapt operator: `@`

## Constants and compile-time evaluation

## Modules, packages, and libraries

## The system library and the `std` package

## Scripts

## Interactive script units

## Workspaces

## Todo

aggregates and collections