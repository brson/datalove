# Datalove Modules, Functions, and Scripts

### 2026-01-05 - Name resolution and mutual recursion

Within a module
name resolution is bidirectional.
Top-level names may only be declared once,
and they may be mutually recursive.

```datalove
// Pretend this is a module.

fun is_even(n: u32): !bool
    if n == 0
        ret ok true
    else
        ret is_odd(n -! 1)  // Forward reference OK in modules.
    end if
end fun

fun is_odd(n: u32): !bool
    if n == 0
        ret ok false
    else
        ret is_even(n -! 1)
    end if
end fun
```

Within a script
name resolution is mostly one-directional.
Names may only refer to previous declarations,
except that functions are visible throughout the script,
so they too may be called before their definition and be mutually recursive.
A function may use type aliases declared later in the script,
but cannot see the script's `let` and `var` bindings at all.

```datalove
// Pretend this is a script.

debuglog is_even(7)!  // OK: functions are visible throughout.

fun is_even(n: u32): !bool
    if n == 0
        ret ok true
    else
        ret is_odd(n -! 1)  // OK: forward reference to a function.
    end if
end fun

fun is_odd(n: u32): !bool
    if n == 0
        ret ok false
    else
        ret is_even(n -! 1)
    end if
end fun

let a = b             // ERROR: `b` not yet defined.
let b: u32 = 1
```




### Function return types

Functions with return types require `ret` with value,
on every path out of the body.
Reaching the end of the body without one is an error.

```datalove
fun choose(a: u32): bool
  if a .< 10
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
  if a .< 10
    ret
  end if
end fun
```




### Pure functions + mutable-reference argument modes

Four argument modes: `in`, `out`, `ref`, `mut`

- `in` - the callee takes ownership and may consume the value
- `ref` - the caller keeps ownership; the callee only reads
- `mut` - the caller keeps ownership; the callee may modify it
- `out` - the callee initializes it; the caller receives the value

```datalove
fun f(
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

Call sites repeat the mode of every non-`in` argument,
so borrowing and mutation are visible where the call is written:

```datalove
var b: int
let c: int = 2
var d: int = 3
call f(1, out b, ref c, mut d)
```




