# Datalove Functions Primer

_Datalove Functions_ is the side-effect-free sublanguage of Datalove.
We often refer to it as _datafun_.
On top of the data types defined by [Datalove Literals](datalit.md) it adds:

- Pure and (nearly) total functions
  that cannot perform I/O and have no exceptional control-flow.
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



## Data types

Datalove functions operate on the [datalit](datalit.md) types,
which are structural and linear.
They are in brief:

Primitives: `bool`, `int`, `f64`, `f32`, `string`,
`u8` .. `u64`, `i8` .. `i64`, `index`, `offset`.

Collections:

- list, `[T]`
- map, `%{ K = V }`
- set, `#{ K }`
- table, `{| col1: T1, col2: T2 |}`
- tensor, `[|T, N|]`, where `N` is the rank

Aggregates:

- unit, `()`
- 1-tuple, `(T1,)`
- n-tuple, `(T1, T2)`
- struct, `{ x: T1, y: T2 }`
- option, `?T`
- result, `!T`
- atom, `atom Foo`
- term, `term Foo T`
- enum, `enum { atom A, term B T }`

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
    count = count,
    other_flags = 0x00,
  }
end fun
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
)
  set by_mut = by_value + by_ref
  set by_out = by_value * by_ref
end fun

let a = 3
var b = 4
var c: int
call demo_param_modes(2, ref a, mut b, out c)

debuglog (b, c)
```

Every statement begins with a keyword,
so a function called for its effect rather than its value
is written after `call`.

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
  else if rem_checked(counter@, 2) == some 0
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
  if rem_checked(counter@, 2) == some 0
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

The option form may continue with an `else if` chain.
In the result case the `else` branch is required,
and must bind the error, `else |e|`;
it cannot be followed by `if`.

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
use `case default` for a catch-all.

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
optional values and error handling
(the `!=` operator aside).

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

import u32.add_checked
import u32.max_value

fun add_saturating(self: u32, other: u32): u32
  if add_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

debuglog(add_saturating(: u32 / 100, max_value()))
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

debuglog(add_twice(: u32 / 1, : u32 / 2))
```

With the result type, `!`:

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

Datalove has fixed-width integer types,
`u8` .. `u64` and `i8` .. `i64`,
and an unbounded big integer type, `int`.

Integer literals have the bigint `int` type by default,
when nothing in their context expects another type.
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

Floating point literals are written with a decimal point
or an exponent (`1.5`, `2.5e-10`, `1e6`)
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

import u8.from_u64

let fits: ?u8 = from_u64(100)
let dontfits: ?u8 = from_u64(1000)

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

import u8.add_wrapping

let v = add_wrapping(255, 1)

debuglog(v)
```




## Comparison and equality

Values are compared with `==` and `!=`,
and ordered with `.<`, `.>`, `<=` and `>=`.
All of them produce `bool`.
Both operands must have the same type;
as with arithmetic, comparisons never widen.
Unlike arithmetic, comparisons work directly on the fixed-width integers.

```datalove
let x: u32 = 5
let maybe: ?u32 = some 5
let name = "datalove"

debuglog (x .< 10, x >= 5, maybe == some 5, name != "datafun")
```

The ordering operators take only numbers.
Equality takes numbers, `bool`, `string`, unit and atoms,
along with options, tuples, structs, terms and enums
whose parts all support equality,
which are compared part by part.
Lists, maps, sets, tables, tensors, results, `data` and `error`
do not yet have an equality operator.

Floats compare according to IEEE 754 wherever they appear,
so `NaN` is not equal to itself, even inside an option,
and `0.0 == -0.0`.

Comparisons do not chain: `a == b == c` is an error.
Parenthesize the comparison being compared,
or join two comparisons with `and`.

Everything else can be ordered with `sys/std/ord`,
which provides a total order over every type,
the same order sets and maps keep their keys in,
along with `min`, `max`, `clamp`, sorting and searching.
Strings also have `string.cmp`.

```datalove
require module sys/std/ord

import ord.less
import ord.sorted

debuglog less(ref "apple", ref "banana")
debuglog sorted(ref ["pear", "apple", "fig"])
```




## The adapt operator: `@`

Every type is either _copy_ or _linear_.
The copy types are `bool`, the fixed-width integers,
`index`, `offset`, `f32` and `f64`, and atoms,
plus terms and enums whose payloads are all copy.
Everything else is linear,
including `int`, `string`, the collections, `data` and `error`.

A linear value has exactly one owner.
Binding it to a new name, passing it by value,
or returning it _moves_ it,
and the old name may not be used again.
Even `let y = x` followed by a use of `x`
is an error when `x` is an `int`.

```datalove
let a = "hello"
let b = a
// debuglog a  -- error: use of moved value `a`
debuglog b
```

Operators and `debuglog` borrow their operands rather than consuming them,
so `x + 1` leaves `x` usable.

The postfix adapt operator, `@`, makes an explicit copy or conversion.
With no expected type, or where the expected type is the value's own,
it _clones_: a deep copy of a linear value,
leaving the original where it was.
Where the context expects a wider type,
it _widens_ to that type:

- unsigned to wider unsigned, `u8` -> `u16` -> `u32` -> `u64` -> `int`
- signed to wider signed, `i8` -> `i16` -> `i32` -> `i64` -> `int`
- unsigned to a wider signed type, e.g. `u8` -> `i16`
- `index` and `offset` to `int`
- `f32` to `f64`
- an atom or term into an enum that lists it

The expected type comes from a binding's type annotation,
a parameter's type, a function's return type,
or the other operands of an operator.

```datalove
let greeting = "hello"
let copy = greeting@
debuglog (greeting, copy)

let small: u8 = 200
let wide: i16 = small@
let big: int = small@ + 1000

let red = atom Red
let color: enum { atom Red, atom Blue } = red@

debuglog (wide, big, color)
```

`@` never loses information.
It does not narrow, and it does not convert between integers and floats;
those conversions are library functions (see [Numerics](#user-content-numerics)).
An enum does not widen into another enum.

When a value is used after it has been moved,
the compiler's error suggests where an `@` would fix it.




## Constants and compile-time evaluation

`const` binds a value that the compiler computes.
It is valid at script top level, in function bodies,
and at module top level,
where it is in scope for every function in the module.

Any function can be called at compile time,
evaluated by the same means as at run time,
so a constant may hold anything a function can compute,
including strings, bigints and collections.

```datalove
fun fib(n: u32): !u64
  var a: u64 = 0
  var b: u64 = 1
  var i: u32 = 0
  loop while i .< n
    let next = a +! b
    set a = b
    set b = next
    set i = i +! 1
  end loop
  ret ok a
end fun

const FIB_50: !u64 = fib(50)
const GREETING: string = "hello"
const PRIMES: [u32] = [2, 3, 5, 7]

let a = GREETING
let b = GREETING
debuglog (FIB_50, a, b, PRIMES)
```

A const expression may only name other consts,
since nothing bound by `let` or `var` exists yet when it is evaluated.
A const names a value, not a place,
so reading one does not move it:
a const of a linear type may be read any number of times.
Unlike `let` bindings, script-level consts are also visible
inside the script's functions.

If a const expression early-returns,
e.g. because `/?` divided by zero,
compilation fails.
A module-level const has no enclosing function to return from,
so it cannot use the early-return operators at all.

Function parameters may also be `const`.
The argument must be the name of a const binding,
and the compiler makes a specialized copy of the function
for each distinct value it is called with.

```datalove
fun scale(const factor: u32, x: u32): ?u32
  ret some (x *? factor)
end fun

const DOUBLE: u32 = 2
const TRIPLE: u32 = 3

debuglog (scale(DOUBLE, 21), scale(TRIPLE, 21))
```




## Modules, packages, and libraries

Datalove code is organized in three levels:
a _library_ contains _packages_, which contain _modules_.
A module is a single `.dfm` source file
and is named by its path, `library/package/module`,
e.g. `sys/std/u32`.
The system library is `sys`;
user code goes in other libraries, conventionally `local`.

A module contains functions, type aliases, constants,
and its own `require` and `import` statements.
Within a module, top-level names may be used
before the point where they are defined,
so module functions may be mutually recursive.

`require module` makes a module available under its last path component,
and `import` brings one of its functions into scope.
There is no qualified call syntax:
`u8.max_value()` does not parse,
so every function used must be imported by name.
A name binds one function,
so importing a second function under a name already imported is an error,
e.g. `u8.max_value` and `u16.max_value` cannot both be imported into one scope.

```datalove
require module sys/std/u8
require module sys/std/string

import u8.max_value
import string.to_uppercase

debuglog (max_value(), to_uppercase(ref "loud"))
```

Only functions can currently be imported;
a module's type aliases and constants are private to it.

A package may also have a native _rider_:
a Rust crate implementing functions
declared with `native fun` in an interface file, `rider.dli`.
The package's modules `require rider` and import from it
like any other module,
and wrap the native functions for their callers.
The system library is built this way,
e.g. string operations and float math are implemented by its rider.




## The system library and the `std` package

The system library, `sys`, is built into the `datalove` binary
and is available to every script unless `--no-sys` is given.
Its one package, `std`, is a small compute-only standard library
with a module for each built-in type:

- `bool`, `int`, `string`
- `u8` .. `u64`, `i8` .. `i64`, `index`, `offset`
- `f32`, `f64`
- `list`, `map`, `set`, `tensor`
- `option`, `result`
- `fixedint`, `float` and `ord`, which are generic over families of types

Most functions take the value they operate on as their first parameter,
e.g. `len(ref s)` from `string`, `push(mut xs, x)` from `list`,
and `unwrap_or(opt, default)` from `option`.

The numeric modules follow consistent naming conventions:

- `add_checked`, `sub_checked`, etc. return an option;
  `_saturating` and `_wrapping` variants clamp or wrap.
- `from_X` converts from a wider type of the same signedness,
  returning `none` if the value does not fit,
  and `from_X_wrapping` keeps the low bits.
  Every fixed-width type has `from_int`,
  and `f32` and `f64` have `from_` each fixed-width integer type.
- `min_value`, `max_value` and `bits` give a type's limits.

```datalove
require module sys/std/list
require module sys/std/option

import list.push
import list.get
import option.unwrap_or

var xs: [u32] = [10, 20]
call push(mut xs, 30)

debuglog unwrap_or(get(ref xs, 2), 0)
debuglog unwrap_or(get(ref xs, 5), 0)
```




## Scripts

A script is a `.dfs` file of top-level statements,
run in order with `datalove script <file>`.
A script may define functions, type aliases and constants,
`require` and `import` modules,
and contain any statement that may appear in a function body.

Functions in a script may be called before they are defined,
but they cannot see the script's `let` and `var` bindings,
only its consts.

Datafun cannot perform I/O.
The only output is `debuglog`, which prints any value to stderr.
If the script leaves a binding named `output`,
its value is printed to stdout when the script finishes.

```datalove
require module sys/std/string

import string.concat

fun greet(ref name: string): string
  ret concat(ref "hello, ", ref name)
end fun

let output = greet(ref "world")
debuglog "done"
```

By default scripts are interpreted.
`datalove script --jit` compiles functions with Cranelift as they are called,
and `datalove aot-compile` compiles a script ahead of time
to an object file or, with `--link` or `--run`, a statically-linked executable;
with `--c` it compiles via generated C instead.




## Interactive script units

A script is a sequence of _units_.
A whole `.dfs` file is a single unit,
but at the REPL, `datalove repl`, each input is its own unit:
a statement, a function definition, or an expression whose value is printed.
Bindings and functions defined by earlier units are visible to later ones.

Because each line is a separate unit,
a unit copies the values of earlier units' bindings rather than moving them,
so `let b = a` at the prompt leaves `a` usable.
Within one unit the usual move rules apply.

The compiler tracks which units each unit depends on,
and when a unit or a module it imports is edited,
it re-typechecks and re-executes only that unit
and the units that depend on it.
The terminal REPL does not yet expose editing earlier units;
today it only appends.

`datalove repl --script <file>` runs a file of units separated by `---` lines
non-interactively, reporting the result of each unit.




## Workspaces

A _workspace_ is everything the compiler can `require` from:
the system library and any number of user libraries.
Scripts are compiled against a workspace rather than as part of it.

Workspaces on the filesystem are not implemented yet.
`datalove script` compiles against the system library alone.
To use modules of your own, put them in a _worldfile_,
a single file holding modules and a script,
and run it with `datalove script-world`.
Each section starts with a header between two dashed lines.

```
----------
module local/geom/shapes
----------
require module sys/std/f64
import f64.sqrt

fun distance(ax: f64, ay: f64, bx: f64, by: f64): f64
  ret sqrt(square(ax - bx) + square(ay - by))
end fun

fun square(x: f64): f64
  ret x * x
end fun

----------
scriptunit-fragment
----------
require module local/geom/shapes
import shapes.distance

debuglog distance(0.0, 0.0, 3.0, 4.0)
```




## Todo

aggregates and collections