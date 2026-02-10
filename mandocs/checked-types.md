# Optional and Result Types and Operations in Datalove




### Error handling

The result type is a language type,
and is dedicated the `!` sigil &mdash;
if you see `!` you are looking at error handling.

The error type is a dynamic type that holds a value of any type.




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

Fixed ints do not support bare arithmetic (`+ - * /`).
Use `@` to widen to `int` first, or use checked/optional operators.

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




