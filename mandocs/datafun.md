# Datalove Functions Primer

_Datalove Functions_ is the side-effect-free sublanguage of Datalove.
We often refer to it as _datafun_.
On top of the data types defined by [Datalove Literals](datalit.md) it adds:

- Pure and (nearly) total functions.
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



## Functions

Functions have a line-oriented and statement-oriented syntax:

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

Though lines can break freely between matched braces:

```datalove
fun count_substrings(
  s: string, ref needle: string
): ?int
  ... etc ...
end fun
```

Function arguments are either passed by value, by reference (`ref`),
by mutable reference (`mut`), or as `out` paramaters,
which are mutable references that may not be read and may or may not be previously-initialized.
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


## Control flow

Loops are performed with the `loop` keyword, `break` and `continue`.
There are no conditional loops. Basic conditional control
flow is performed with `if`.

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


## Data types and destructuring

## Option and result

## Data types and their operations

## The `adapt` operator

## Constants and compile-time evaluation

## Modules and their organization

modules, packages, libraries

## The standard library

## Scripts

## Interactive script units

## Workspaces
