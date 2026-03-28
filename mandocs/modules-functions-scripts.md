# Datalove Modules, Functions, and Scripts

### 2026-01-05 - Name resolution and mutual recursion

Within a module
name resolution is bidirectional.
Top-level names may only be declared once,
and they may be mutually recursive.

```datalove
// Pretend this is a module.

fun is_even(n: u32): !bool
    if n ≡ 0
        ret ok true
    else
        ret is_odd(n -! 1)  // Forward reference OK in modules.
    end if
end fun

fun is_odd(n: u32): !bool
    if n ≡ 0
        ret ok false
    else
        ret is_even(n -! 1)
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
    if n ≡ 0
        ret ok false
    else
        ret is_even(n -! 1)  // ERROR: `is_even` not yet defined.
    end if
end fun

fun is_even(n: u32): !bool
    if n ≡ 0
        ret ok true
    else
        ret is_odd(n -! 1)  // OK: `is_odd` already defined.
    end if
end fun
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




