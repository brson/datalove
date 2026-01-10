## The Datalove Decree

Datalove on various language design topics.




### 2026-01-08 - Module memoization tests

We focus our memoization testing at module granularity
because module-level memoization is impactful
and easy to reason about in tests.

We care about testing the changing
over time of three module-level values:
the ast,
the typecheck results.
the content hash,

We test the changes in these values
after discreet actions:

- add-module - adds a module that doesn't already exist
- remove-module - removes a module that exists
- change-module-ws - replace an existing module source that changes the whitespace,
  (but not the newlines in the whitespace)
- change-module-ast - replace an existing module with source
  that produces a different ast does not change the typechecking
- change-module-ty - replace an existing module with source
  that changes the ast and produces a different typecheck result

This table indicates whether we expect recalculation
of the directly changed module, or its transitive dependents,
after each of the actions has been taken.

| action            | direct-ast | direct-ty | direct-hash | depend-ast | depend-ty | depend-hash |
|-------------------|------------|-----------|-------------|------------|-----------|-------------|
| add-module        | y          | y         | y           | n/a        | n/a       | n/a         |
| remove-module     | y*         | y*        | y*          | y          | y         | y           |
| change-module-ws  | y          | n         | y           | n          | n         | y           |
| change-module-ast | y          | y         | y           | n          | n         | y           |
| change-module-ty  | y          | y         | y           | n          | y         | y           |

> *: removed

Our test suite is a worldfile variant with the following sections:
`module`, `module-add`, `module-change-ws`, `module-change-ast`, `module-change-ty`.
Note that the test must trust the user that they have got the "change-*" semantics correct -
it just knows its changing a module.

Each contains the source of a module with its canonical lib/pkg/module path.
The test harness first loads all `module` sections into the module world,
parses and typechecks.

Then for each of the action sections in turn:

- merge the module into the module world (or remove it)
- run the parser and typechecker, calculate content hashes
- for each module that remains, calculate `changed_ast`, `changed_ty`, `changed_hash`,
- compare the results to our expected results based on the table above
- add the observed change analysis plus their expected results to the "actual" output

The pass/fail-ness of the test is determined by the blessed "expected" files;
the calculated analysis is just to help guide is to a fully-working memoization system.




### 2026-01-08 - Field projections

Datalove structs and tuples support
reading and writing of fields.

```datalove
let a: { x: int, y: int) = (1, 2)
let b = a.x   // this is a move, `a` is partially destructured
let c = a.y   // another move

let a: (int, int) = (1, 2)
let b = a.0
```

Copy-types copy their projections;
move types move.

Partial moves:
ompiler must track which struct fields have been moved,
deny any further uses of the field or aggregate type,
handle precise partial destruction at later drop points.

The kind of projection depends on the destination:
if the destination is a `ref`, `mut`, or `out` params,
or operator operands,
then the projections become ref projections,
the aggregate remains fully constructed after
the call.

#### Reinitialization

previously-deinitialized struct fields _can_
be projected into `out` params, after which
they become reinitialized; potentially making
the aggregate fully-constructed again
and able to be used in aggregate.

Within loops,
what happens with moved projections of outer fields?

todo todo




### 2026-01-07 - Logic operators

Booleans support `and`, `or`, `xor`, and `not`.
todo say more




### 2026-01-07 - Module content hashes and memoization

The module graph forms a DAG.
We use this to create strong content hashes
for every module instantiation.

This can be used as a key for various caching purposes.
We specifically use it to verify correct memoization:
Functions on modules should only rerun if their
module's content hash has changed.

This content hash includes
the source code of a module,
and the configuration of that module
including which modules the requires/import demands are bound to.




### 2026-01-05 - Name resolution and mutual recursion

Within a module
name resolution is bidirectional.
Top-level names may only be declared once,
and they may be mutually recursive.

```datalove
// Pretend this is a module.

fun is_even(n: u32): !bool
    if n == @0
        ret ok @true
    else
        ret is_odd(n -! @1)  // Forward reference OK in modules.
    end if
end fun

fun is_odd(n: u32): !bool
    if n == @0
        ret ok @false
    else
        ret is_even(n -! @1)
    end if
end fun
```

Within a script
name resolution is one-directional.
Names may only refer to previous declarations.

```datalove
// Pretend this is a script.

// In scripts, `is_odd` must be defined before `is_even` can call it.

fun is_odd(n: u32): !bool
    if n == @0
        ret ok @false
    else
        ret is_even(n -! @1)  // ERROR: `is_even` not yet defined.
    end if
end fun

fun is_even(n: u32): !bool
    if n == @0
        ret ok @true
    else
        ret is_odd(n -! @1)  // OK: `is_odd` already defined.
    end if
end fun
```




### Unconditional and conditional loops

Unconditional loops are spelled `loop`.
They require a `break` or `ret` to exit.

```datalove
var x = 0

loop
  if x = 0
    set x = 1
  else if x = 10
    set x = 11
    break
  end if
  set x = x + 1
end loop

debuglog x
```

Conditional loops with `loop while`:

```datalove
var x = 0

loop while x != 10
  if x = 0
    set x = 1
  else if x = 10
    set x = 11
  else
    set x = x + 1
  end if
end loop

debuglog x
```




### Function return types

Functions with return types require `ret` with value.

```datalove
fun choose(a: u32): bool
  if a < 10
    ret true
  else
    ret false
  end if
end fun
```

Functions can have void return types
and allow `ret` statements without values.

```datalove
fun foo()
  // no ret required
end fun

fun choose(a: u32)
  if a < 10
    ret
  end if
end fun
```




### Option and result handling

The option and result types are prefixes like zig:

```datalove
// option
let a: ?u32 = ...
let b: ?[u32] = ...
// result
let c: !u32 = ...
let d: ![u32] = ...
```

Option and result construction
is done with the `some`, `none`, `ok` and `er` keywords.
Note the awkward `er error` construction which
constructs an `er` `result` varriant out of an `error` value.

```datalove
let a: ?u32 = some 3
let b: ?u32 = some u32.min_value()
let c: ?u32 = none

let a: !u32 = ok 3
let b: !u32 = ok u32.min_value()
let c: ?u32 = er error "oops"
```

Destructuring option and result is like zig:

```datalove
let a: ?u8 = 1

var c: u8 = 0
if a |value|
  set c = value
else
  set c = 255
end if
```

```datalove
let a: !u8 = 1

var c: u8 = 0
if a |value|
  set c = value
else |error|
  ret error
end if
```

Branching on error values is not like zig
and uses reflection, not shown here.

Early return with postfix `?` and `!`:

```datalove
fun transform_option(val: ?u32): ?u32
  let val = val? // early none return
  ret val +? 1   // early none return on overflow
end fun

fun transform_result(val: !u32): !u32
  let val = val! // early error return
  ret val +! 1   // early error return on overflow
end fun
```




### Math ops and error handling

Datalove is especially strict about overflow and error
cases in machine-size integers and floats.

Floats support all bare math ops,
`+ - * /` and unary `-`.

Bigints (`int`) support all but div:
`+ - *` and unary `-`.
For div we must use a checked variant to handle divide-by-zero.

Fixed ints support all but div:
`+ - *` and unary `-`,
but _widen_ to bigints.

Fixed ints support checked early-return varieties:

```datalove
// Checked optional arithmetic, early-returning `none`.
let a = 1 +? 1
let a = 1 -? 1
let a = 1 *? 1
let a = 1 /? 1
let a = -?a     // early-return negation

// Checked result arithmetic, early-returning `er`.
let a = 1 +! 1
let a = 1 -! 1
let a = 1 *! 1
let a = 1 /! 1
let a = -!a
```

`?` early-returns `?T` option types and `!` early-returns `!T` result types.

These either result in the same type as the input types or early return -
they do not result in option or result types. Their enclosing function
must be the correct optionr/result type.

Bigints support the early-return division but not the others.
Floats don't support early-return math.

If we decide to let funs panic we'll also add panicking variations.




### Comparison operators
#### Floats and total ordering

All pure data types support a total order,
which is used for maps and sets.

Floats use the typical ordering, like Rust's `total_cmp`:

> -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN

Equality, less than, greater than, etc. behave
the standard way wrt float zeros and NaNs.










### Pure functions + mutable-reference argument modes

Four argument modes: `in`, `out`, `ref`, `mut`

```datalove
fun (
  a: int,     // default `in`
  out b: int,
  ref c: int,
  mut d: int,
)
  // `out` args must be assigned on all code paths
  set b = a
  set d = c + d
end fun
```




### Allocations

Besides encoding heaps in types,
having local and global heaps,
and enforcing a light syntax
on allocating conversions,
Datalove is not concerned with providing
control over the allocator;
but instead only with providing data access patterns
and metadata with which the allocator can perform optimally.




### Error handling

The result type is a language type,
and is dedicated the `!` sigil &mdash;
if you see `!` you are looking at error handling.

The error type is a dynamic type that can hold
any type (an existential type).




### REPL

The script is REPL-first and intends to advance the
state of the art in REPLs.
With pure data statements and virtualized I/O the repl
supports undo/redo and record-and-replay.

The repl is a modern Ratatui application.
The datafun interpreter runs on the web
as a egui_ratatui application.




### Trailing commas and separators

Allowed and optional in all sequence forms of course.




### Generics

Functions can have type parameters:

```datalove
fun unwrap_or<T>(self: ?T, default: T): T where {
  T is move,
}
  if self |value|
    ret value
  else
    ret default
  end if
end fun
```

By default type parameters have no capabilities,
`clone`, `move`, `etc`. `where` clauses are
usually needed.

Data structures can not have type parameters.

TODO: How are generics translated?




### Type aliases

Datafun has a structural type system.
We can asign names with `typealias`.

```datalove
// `Contact` is an alias for a struct.
typealias Contact: {
  name: string,
  age: int,
}
```




---



## Wanted

- operator precedence
