# Control Flow


## If / else

```datalove
if condition
    // then branch
else
    // else branch
end if
```

The else branch is optional.
Chained conditions use `else if`:

```datalove
if x ≡ 0
    debuglog "zero"
else if x < 10
    debuglog "small"
else
    debuglog "big"
end if
```


## If with binding

`if` can unwrap an option or result, binding the inner value.

```datalove
let o: ?int = some 42

if o |value|
    debuglog value
end if
```

For results, the else branch binds the error:

```datalove
let r: !int = ok 42

if r |value|
    debuglog value
else |e|
    debuglog e
end if
```


## Loop

Unconditional loops require `break` or `ret` to exit.

```datalove
var x: int = 0

loop
    if x ≡ 10
        break
    end if
    set x = x + 1
end loop
```

Conditional loops with `loop while`:

```datalove
var x: int = 0

loop while x < 10
    set x = x + 1
end loop
```

`break` exits the innermost loop.
`continue` jumps to the next iteration.


## Match

`match` destructures an enum value.

```datalove
match color
case atom Red
    debuglog "red"
case atom Blue
    debuglog "blue"
end match
```

Term cases bind the payload to a variable:

```datalove
match shape
case atom Circle
    debuglog "circle"
case term Rect dims
    debuglog dims
end match
```

Match consumes (moves) its input.
Without a default, the match must be exhaustive.
Use `case default` for a catch-all:

```datalove
match shape
case atom Circle
    debuglog "circle"
case default
    debuglog "other"
end match
```


## Return

`ret` returns a value from a function:

```datalove
fun double(x: int): int
    ret x + x
end fun
```

Void functions may use bare `ret` for early exit:

```datalove
fun log_positive(x: int)
    if x ≤ 0
        ret
    end if
    debuglog x
end fun
```


## Conditional initialization

Uninitialized vars can be initialized in branches,
but must be initialized on all paths before use.

```datalove
var result: i32

if condition
    set result = 1
else
    set result = 2
end if

debuglog result   // ok -- initialized on all paths
```

This won't compile:

```datalove
var result: i32

if condition
    set result = 1
end if

debuglog result   // error -- may be uninitialized
```
