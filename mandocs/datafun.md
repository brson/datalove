# Datalove Functions Primer

_Datalove Functions_ is the side-effect-free sublanguage of Datalove.
We often refer to it as _datafun_.
On top of the data types defined by [Datalove Literals](datalit.md) it adds:

- Pure functions that cannot perform I/O and have no exceptional control-flow.
- Constants with full compile-time function evaluation.
- A simple acyclic module system.
- Script units that incrementally recompile and reevaluate when changed.

Datafun scripts may be interpreted directly via lowered IR,
optionally with high-performance bytecode,
optionally with per-function JIT via Cranelift,
or may be compiled to statically-linked binaries, either via Cranelift or C.
The Datalove Functions implementation is self-contained and independent from full Datalove,
suitable as a constrained embedded application scripting language.

This document is a broad overview of the language.
For detail see additional documentation.




## Contents

<div class="toc toc-repeat-8">

- [A first script](#user-content-a-first-script)
- [Data types](#user-content-data-types)
- [Variables](#user-content-variables)
- [Functions](#user-content-functions)
- [Ownership](#user-content-ownership)
- [Control flow](#user-content-control-flow)
- [Data types and destructuring](#user-content-data-types-and-destructuring)
- [Option and result handling](#user-content-option-and-result-handling)
- [Collections and indexing](#user-content-collections-and-indexing)
- [Numerics](#user-content-numerics)
- [Comparison and equality](#user-content-comparison-and-equality)
- [Modules, packages, libraries and the workspace](#user-content-modules-packages-libraries-and-the-workspace)
- [Constants and compile-time evaluation](#user-content-constants-and-compile-time-evaluation)
- [Generics](#user-content-generics)
- [Scripts and interactive units](#user-content-scripts-and-interactive-units)

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
Modules are covered further in [Modules, packages, libraries and the workspace](#user-content-modules-packages-libraries-and-the-workspace).

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
`loop` and `if` are statements, not expressions, and do not yield a value.

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

Enums are destructured with `match`.
`match` is a statement, not an expression, and does not yield a value.

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

`match` must be exhaustive.
Use `case default` for a catch-all.

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
In the result case the destructuring `else` branch is required.

The postfix `?` and `!` operators propagate
option and result return types.

```datalove
require module sys/std/u32

fun add_twice(self: u32, other: u32): ?u32
  let once: u32 = u32.add_checked(self, other)?
  let twice: u32 = u32.add_checked(once, other)?
  ret some twice
end fun

debuglog (add_twice(: u32 / 1, : u32 / 2))
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

debuglog (add_twice(: u32 / 1, : u32 / 2))
debuglog (add_twice(: u32 / 1, : u32 / 4000000000))
```




## Collections and indexing

Datalove has five bult-in collection types:
lists, maps, sets, tables and tensors.
Like every value, collections are uniquely owned,
and copying one is an explicit clone with `@`.

```datalove
let xs = [10, 20, 30]
let ages = %{ "ada" = 36, "alan" = 41 }
let primes = #{ 7, 2, 5, 3, 2 }
let grid = [| 1 2 3, 4 5 6 |]
let points = {| x, y; 1, 2; 3, 4 |}

debuglog xs
debuglog ages
debuglog primes
debuglog grid
debuglog points
```

Sets and maps keep their elements and keys in order,
the total order described in [Comparison and equality](#user-content-comparison-and-equality),
so a set literal with a duplicate holds it once,
and a map literal with a repeated key keeps the last value.

An empty collection literal has no element type to infer,
so it needs a type annotation.

```datalove
let names: [string] = []
let scores: %{string = u32} = %{}
let seen: #{u64} = #{}

debuglog (names, scores, seen)
```


Struct fields and tuple elements are read with a dot,
by name or by position.

```datalove
let p = { name = "ada", age = 36 }
let t = (1.5, true)

debuglog p.age
debuglog t.0
```

A field of a copy type is simply copied out.
A field of a non-copy type, like a string,
may be borrowed where it is, as an operand or a `ref` argument,
but it cannot be moved out of the aggregate that still holds it.
Clone it with `@` to get a value of its own.

```datalove
let p = { name = "ada", age = 36 }

// A string field is borrowed in place...
debuglog p.name

// ...but moving it out would leave a hole in `p`, so it is cloned.
let name = p.name@

debuglog (name, p)
```


Lists, maps and tensors are indexed with brackets.
Lists and tensors are indexed by the `index` type, and maps by their key type.

Indexing can always fail,
because the index may be out of bounds or the key absent,
and Datalove has no indexing operation that panics.
Every index is followed by `?` or `!`,
which say what happens on failure,
just like the checked arithmetic operators:
`?` returns `none` from the enclosing function,
and `!` returns `er error "index out of bounds"`,
or `er error "key not found"` for a map.
The enclosing function must return an option or a result to match.

```datalove
fun second(xs: [u32]): ?u32
  ret some (xs[1]?)
end fun

fun age_of(ages: %{string = u32}, name: string): !u32
  ret ok (ages[name]!)
end fun

let ages: %{string = u32} = %{ "ada" = 36, "alan" = 41 }

debuglog (second([5, 6, 7]), second([5]))
debuglog (age_of(ages@, "ada"), age_of(ages, "grace"))
```

A script returns a result,
so at the top level of a script `!` stops the script with the error.

```datalove
let xs = [10, 20, 30]

debuglog xs[1]!
debuglog xs[5]!
debuglog "not reached"
```

As with fields, an element of a copy type is copied out of its collection,
and an element of a non-copy type is borrowed or cloned with `@`.

```datalove
let names = ["ada", "alan"]

let first = names[0]!@

debuglog (first, names)
```

Indexes and fields chain.
Each `?` or `!` in a chain is checked in turn, left to right.
Indexing a tensor of rank 2 or more yields its next-lower rank,
so a two-dimensional tensor is indexed twice.

```datalove
let people = [
  { name = "ada", langs = ["analytical engine"] },
  { name = "alan", langs = ["ace", "turing machine"] },
]
let grid = [| 1 2 3, 4 5 6 |]

debuglog people[1]!.langs[0]!
debuglog grid[1]![2]!
```

Sets and tables are not indexed.
A set is queried with functions from `sys/std/set`.


An indexed element of a mutable collection is a place that `set` can write.
On a map, `set m[k]! = v` updates a key that must already be present,
while a bare `set m[k] = v` inserts the key or overwrites it.
A list has no bare form, since its elements must exist to be overwritten.

```datalove
var xs = [1, 2, 3]
set xs[0]! = 10

var ages = %{ "ada" = 36 }
// Update an existing key, failing if it is absent.
set ages["ada"]! = 37
// Insert or overwrite.
set ages["alan"] = 41

debuglog (xs, ages)
```

Most other work on collections is done by functions
in the `list`, `map`, `set` and `tensor` modules of `sys/std`.
Sorting and searching are in `sys/std/ord`.

```datalove
require module sys/std/list
require module sys/std/map
require module sys/std/set

var xs = [1, 2]
call list.push(mut xs, 3)

var ages = %{ "ada" = 36 }
call map.insert(mut ages, "alan", 41)
let who = "ada"

let primes = #{ 2, 3, 5 }

debuglog list.len(ref xs)
debuglog map.get(ref ages, ref who)
debuglog set.contains(ref primes, ref 3)
```

There is no `for` loop.
A list is walked with `loop while` and an index.

```datalove
require module sys/std/list

let xs = [3, 1, 4, 1, 5]
var total = 0
var i: index = 0

loop while i .< list.len(ref xs)
  set total = total + xs[i]!
  set i = i +! 1
end loop

debuglog total
```

Tables are a work-in-progress and need further language support to be useful.




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
Thus none of the bare math operations work on fixed-sized integers.
Instead these types must use checked versions of the operations
which early return from their enclosing function with either `none` or `er`.

```datalove
fun do_some_math_opt(a: i32, b: i32, c: i32): ?i32
  ret some -?((a +? b) /? c)
end fun

fun do_some_math_result(a: i32, b: i32, c: i32): !i32
  ret ok -!((a +! b) /! c)
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

debuglog (v)
```




## Comparison and equality

Values are compared with six operators,
all of which produce a `bool`.

| Operator | Meaning                  |
|----------|--------------------------|
| `==`     | equal                    |
| `!=`     | not equal                |
| `.<`     | less than                |
| `.>`     | greater than             |
| `<=`     | less than or equal       |
| `>=`     | greater than or equal    |

Less-than and greater-than are spelled `.<` and `.>`
because `<` and `>` are brackets in Datalove,
as in the type parameters of a generic function.

```datalove
let a = 3
let b = 4

debuglog (
  a == b,
  a != b,
  a .< b,
  a .> b,
  a <= b,
  a >= b
)
```

Both operands must have the same type.
As with arithmetic, there is no implicit widening,
so a narrower number is widened explicitly with `@`.

```datalove
let small: u8 = 200
let large: u32 = 200

debuglog small@ == large
```

Comparison reads its operands rather than moving them,
so comparing a string leaves it in place.

```datalove
let name = "datalove"
let same = name == "datalove"

// Comparing reads its operands rather than moving them.
debuglog (name, same)
```

Comparisons do not chain:
`a == b == c` is a parse error.
Parenthesize the comparison being compared,
or join two comparisons with `and`.

The ordering operators, `.<`, `.>`, `<=` and `>=`,
work only on numbers.
Other types are ordered with library functions,
described below.

Equality, `==` and `!=`, works on numbers, `bool`, `string`, unit and atoms,
and on options, tuples, structs, terms and enums made of those.
These are compared structurally, part by part.

```datalove
type Shape: enum {
  atom Circle,
  term Rect (f64, f64),
}

let a: Shape = term Rect (1.0, 2.0)
let b: Shape = term Rect (1.0, 2.0)

let p = { name = "origin", at = (0, 0) }
let q = { name = "origin", at = (0, 0) }

debuglog (a == b, p == q, a == atom Circle)
```

Lists, sets, maps, tables, tensors, results, `data`, `error` and functions
have no equality operator,
nor does a type parameter, or anything containing one.
These are compared with library functions too.

A constructor written without a type of its own,
like `none` or `some 7`,
takes the type of the other operand.
When the constructor comes first it must be parenthesized
to make it clear whether the binop is part of the payload or not.

```datalove
let found: ?u32 = some 7

debuglog (
  found == none,
  found == some 7,
  (some 7) == found
)
```

Floats compare according to IEEE 754:
`NaN` is not equal to anything, itself included,
and positive and negative zero are equal.
This holds inside options, tuples and the other aggregates as well.

```datalove
require module sys/std/f64

let nan = f64.nan()
let zero = 0.0
let negative_zero = -0.0

debuglog (nan == nan, zero == negative_zero, (some nan) == some nan)
```

Beyond the operators,
every value in Datalove has a _total order_,
the same order sets and maps keep their keys in.
The `sys/std/ord` module exposes it through functions:
`compare`, `equal`, `less` and the other comparisons,
`min`, `max` and `clamp`,
and functions over lists such as
`sorted`, `contains`, `index_of`, `binary_search` and `deduped`.
These work on every type, including the ones the operators refuse.

```datalove
require module sys/std/ord
require module sys/std/f64

let xs = [3, 1, 2]
let ys = [3, 1, 2]

debuglog (ord.equal(ref xs, ref ys), ord.sorted(ref xs), ord.max(4, 9))

let apple = "apple"
let banana = "banana"

debuglog ord.less(ref apple, ref banana)

let nan = f64.nan()
let zero = 0.0
let negative_zero = -0.0

debuglog (ord.equal(ref nan, ref nan), ord.equal(ref zero, ref negative_zero))
```

The total order agrees with the operators everywhere but floats,
where it follows IEEE 754 `totalOrder`:
`NaN` is equal to itself and has a place in the order,
and the two zeros are different.

A generic function compares values of a type parameter
by bounding it with `is ord`,
which every type satisfies.
Generics are described in [Generics](#user-content-generics).

```datalove
require module sys/std/ord

fun largest<T>(ref items: [T]): ?T with { T is ord, }
  ret ord.greatest(ref items)
end fun

let words = ["pear", "fig", "plum"]

debuglog largest(ref words)
```




## Modules, packages, libraries and the workspace

While scripts are the entry point to all Datalove programs,
most code is written in _modules_.
Modules are contained in _packages_, and packages are contained in _libraries_.
All module dependencies are declared explicitly with the `require module` statement,
which may appear only within modules and scripts (not within functions).
They are conventionally the first statements in either,
though their names will resolve in any top-level position.
The full module graph is discovered early in the compilation pipeline with minimal effort.

Throughout this document we have used the `std` package in the `sys` library.

```datalove
require module sys/std/u8
```

On-disk, datafun modules have the `.dfm` extension, and scripts have the `.dfs` extension.
The library/package/module hierarchy is a reflection of the on-disk organization
of libraries, with the `sys` library's directory layout being &mdash; in part &mdash; as-follows:

```
sys/
  std/
    bool.dfm
    f32.dfm
    f64.dfm
    ...
    list.dfm
    map.dfm
    ...
```

The `sys` library is always available.
When distributed as a binary, the `datalove` executable itself contains the `sys` library
and it does not appear on disk.

Datalove is a whole-program compiler and can always see and monitor all inputs
needed to execute or compile a given script.
The full set of scripts and modules available to an instance of the compiler
is called the _world_.

Additional libraries can be mapped into the world as-needed,
making them available by chosen name to the `require module` statement.
By default a directory named `local` may contain packages and modules
specific to the local _workspace_.

Workspace structure is implicit and relative to the script(s)
being interpreted by the compiler,
or in the case of an interactive session,
relative to the current working directory the compiler was launched from.
There is no workspace manifest.

A workspace on disk has the following structure,
where all files and directories are optional
and the workspace root is discovered automatically.

```
<workspace root>/
  script1.dfs
  script2.dfs
  scripts/
    script3.dfs
    script4.dfs
  local/
    mypkg1/
      mymodule1.dfm
      mymodule2.dfm
    mypkg2/
      mymodule3.dfm
```

The compiler can compile and execute multiple scripts independently and in parallel
from the same instance, sharing the module world and its compilation
pipeline, with each script having its own isolated callstack and heap,
though this capability is not yet exposed through any frontend.




## Constants and compile-time evaluation

`const` binds a value that the compiler computes.
It is written like `let`,
and is valid at the top level of a script or module
and inside function bodies.
Constants are spelled `UPPER_CASE` by convention.

```datalove
const LIMIT: u32 = 10
const NAME: string = "limit"

fun clamp_to_limit(x: u32): u32
  if x .> LIMIT
    ret LIMIT
  else
    ret x
  end if
end fun

debuglog (NAME@, clamp_to_limit(50))
```

There is no separate compile-time sublanguage
and no special marking for functions that may run at compile time.
Every datafun function is pure,
so the compiler evaluates a const by running its expression
with the same machinery that runs it at run time,
and any function can be called from a const.
Constants are not limited to scalars:
strings, bigints and collections are all computed the same way.

```datalove
require module sys/std/list

fun squares(n: int): [int]
  var out: [int] = []
  var x = 0
  loop while x .< n
    call list.push(mut out, x * x)
    set x = x + 1
  end loop
  ret out
end fun

const SQUARES: [int] = squares(5)
const BIG: int = 99999999999999999999 * 99999999999999999999

let a = SQUARES@
let b = SQUARES@

debuglog (a, b, BIG@)
```

A const is borrowed wherever it is named, as a `ref` parameter is.
It can be read, compared, indexed and passed by `ref` any number of times,
but moving out of a const of a moved type takes a clone with `@`,
which is why `SQUARES` and `BIG` above are written with one.
`match`, the destructuring `if` and `let` destructuring
all move what they take apart,
so a const enum is matched as `match C@`.

A const expression is evaluated before any parameter or `let` exists,
so it may only name other consts.
Naming a `let`, `var` or ordinary parameter is a compile error.
The functions it calls bind their own parameters as usual.

In scripts and function bodies,
a const is in scope from where it is written, like `let`.
At the top level of a module it is in scope for the whole module,
regardless of where it is written.
The compiler orders evaluation itself:
a module const is evaluated after the functions it calls,
and before the functions that name it.
If a const calls a function that in turn names a module const,
the two depend on each other and the compiler reports the cycle.

Module consts are private to their module.
To share a constant, export a function that returns it.

The early-return operators, `?`, `!` and the checked arithmetic,
can be used in a const inside a function,
but if one actually returns early the program does not compile.
A script returns a result,
so the same holds for `!` at the top level of a script.
A const at the top level of a module
has no enclosing function to return from,
so cannot use them at all.

```datalove
fun ratio(): ?u32
  const A: u32 = 10
  const B: u32 = 2
  // This compiles, but would not if `B` were 0.
  const C: u32 = A /? B
  ret some C
end fun

debuglog ratio()
```

### Const parameters

A function parameter declared `const` takes a value known at compile time.
The compiler _specializes_ the function,
making a copy of it for each distinct const argument
with the value written into the body.

```datalove
require module sys/std/string

fun repeat(const n: int, ref s: string): string
  var out = ""
  var i = 0
  loop while i .< n
    call string.push_str(mut out, ref s)
    set i = i + 1
  end loop
  ret out
end fun

const THREE: int = 3
let s = "ab"

debuglog repeat(THREE, ref s)
```

The argument to a const parameter must be the name of a const binding.
Not even a literal is accepted:
`repeat(3, ref s)` is an error.
Within the specialized body the parameter is itself a const,
borrowed wherever it is named,
so it may be passed on to other const parameters,
and consts computed from it are evaluated per specialization.
`const` arguments are passed as references during compile-time evaluation
so they are written without `@`.
They may be of any type a const can hold, collections included,
but not a type parameter of a generic function.




## Generics

Datalove functions support type parameters and can be generic over their arguments
and return values.

```datalove
fun unwrap_or<T>(self: ?T, default: T): T
  if self |value|
    ret value
  else
    ret default
  end if
end fun

debuglog unwrap_or(some "this", "other")
debuglog unwrap_or(: ?string / none, "other")
```

Generic functions are compiled once,
their static types erased and instead interpreted dynamically at run time.
Most types that would normally be stored on the stack get boxed to and from the heap
when passed as generics.

Type parameters are inferred from the caller's arguments,
where the first encountered type argument decides it.
There is no explicit caller-side syntax for specifying type parameters.
The following is an error.

```datalove
fun unwrap_or<T>(self: ?T, default: T): T
  if self |value|
    ret value
  else
    ret default
  end if
end fun

// Error: type `T` must be string because `some "this"` decides it.
debuglog unwrap_or(some "this", 1)
```

todo is it possible to write list.empty()?

Aggregate and collection types may include interior generic types.

```datalove
require module sys/std/index
require module sys/std/list
require module sys/std/option

fun zip<A, B>(ref self: [A], ref other: [B]): [(A, B)]
  var built: [(A, B)] = []
  let n = list.len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if option.zip_option(
      list.get(ref self, i),
      list.get(ref other, i)
    ) |pair|
      call list.push(mut built, pair)
    else
      break
    end if
    set i = index.add_wrapping(i, : index / 1)
  end loop
  ret built
end fun

debuglog zip(ref [1, 2, 3], ref ["a", "b", "c"])
```

All generic types are always clonable and movable, but never copyable,
thus generic bindings always move, and producing a copy requires `@`.
All generic types can be stored, returned, passed to other functions,
and debug-printed with `debuglog`.

Some capabilities require bounds on type parameters.
Type parameter bounds are specified within `with` blocks that follow the function header.

```datalove
require module sys/std/fixedint

import fixedint.zero

fun unwrap_or_zero<T>(self: ?T): T with { T is fixedint }
  if self |value|
    ret value
  else
    ret zero()
  end if
end fun

debuglog unwrap_or_zero(: ?u8 / some 10)
debuglog unwrap_or_zero(: ?i64 / none)
```

All bounds are built-in.
There is no interface or trait mechanism for specifying generic type capabilities.

The built-in bounds are:

- `fixedint`
- `ord`
- `float`




## Scripts and interactive units
