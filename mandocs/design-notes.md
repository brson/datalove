## The Datalove Decree

Datalove on various language design topics.



### Function return types

Functions can have void return types
and allow `ret` statements without values.

```
fun foo() {
}

fun choose(a: u32) {
  if a < 10
    ret
  end if
}
```

Functions with return types allow `ret` with value.

```
fun choose(a: u32): bool {
  if a < 10
    ret true
  else
    ret false
  end if
}
```



### Option and result handling

The option and result types are prefixes like zig:

```
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

```
let a: ?u32 = some 3
let b: ?u32 = some u32.min_value()
let c: ?u32 = none

let a: !u32 = ok 3
let b: !u32 = ok u32.min_value()
let c: ?u32 = er error "oops"
```

Destructuring option and result is like zig:

```
let a: ?u8 = 1

var c: u8 = 0
if a |value|
  set c = value
else
  set c = 255
end if
```

```
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

```
fun transform_option(val: ?u32): ?u32
  let val = val? // early none return
  ret val +? 1   // early none return on overflow
end

fun transform_result(val: !u32): !u32
  let val = val! // early error return
  ret val +! 1   // early error return on overflow
end
```




### Math ops and error handling

Datalove is especially strict about overflow and error
cases in machine-size integers and floats.

Floats support all bare math ops,
`+ - * /` and unary `-`.

Bigints (`int`) support all but div:
`+ - *` and unary `-`.
For div we must use a checked variant to handle divide-by-zero.

Fixed ints do not support any bare bath ops, not even unary `-`.

Fixed ints support early-return varieties:

```
let a = 1 +? 1
let a = 1 -? 1
let a = 1 *? 1
let a = 1 /? 1
let a = -?a     // early-return negation

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

`-?` unary op is not defined for unsigned ints -
it has a sensible semantic but is a pure footgun.

Bigints support the early-return division but not the others.
Floats don't support early-return math.

If we decide to let funs panic we'll also add panicking variations.




### Numeric widening

Fixed ints automatically widen, up to bigints:

```
let a: u8 = 1
let b: u16 = a
let c: int = b
```

Same for signed fixed ints:

```
let a: i8 = 1
let b: i16 = a
let c: int = b
```

Unsigned and signed ints never automatically coerce to each other.

Widening also apllies to bare / unchecked math, which
widens to `int`:

```
// this checks to `int` because the `*` binop,
// forcing the literals to be int
let a = 1 * 2
// locals also get coercions
let b: u32 = 1
// another `int`
let c = b * b
```



### † Pure functions + mutable-reference argument modes

Datalove at first does not look pure functional because
it has mutable by-reference arguments:

```datalove
```

etc.


### Allocations

Besides encoding heaps in types,
having local and global heaps,
and enforcing a light syntax
on allocating conversions,
Datalove is not concerned with providing
control over the allocator;
but instead only with providing data access patterns
and metadata with which the allocator can perform optimally.


### Garbage collection

Datalit and Datafun do not require a GC.
Datalove may experiment with interior pointers and GCs in the future.


### Error handling

The result type is a language type,
and is dedicated the `!` sigil &mdash;
if you see `!` you are looking at error handling.

The error type is a dynamic type that can hold
any type (an existential type).




### Panicking and exceptions

In the pure-functional sublanguage Datafun,
functions are _total_,
and there is no mechanism for panicking,
exceptions or unwinding.

These cases are handled with `?`.

Full Datalove has panics, design TBD.




### Threading and concurrency

For simplicity and OS-mechanical sympathy,
Datalove is a multithreaded language;
no lightweight tasks or async-await.
We may experiment with callback-based
asynchrony and Gleam-style inline continuation syntax.




### Floats and total ordering

All pure data types support a total order,
which is used for maps and sets.

Floats use the typical ordering, like Rust's `total_cmp`:

> -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN

Equality, less than, greater than, etc. behave
the standard way wrt float zeros and NaNs.




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
