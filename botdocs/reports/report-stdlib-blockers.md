# What Blocks Stdlib Development

What `sys/std` can and cannot express today, and what would have to change to get further.

Rewritten after the original report went stale. That version was written when the library
was six modules and 61 functions, and by the time anyone read it again the library was
seventeen modules and 699 functions with most of its stated blockers gone. Its central
claim, that linear types made `int` unwritable, was wrong when written or shortly after.
Treat the numbers here as a snapshot too, and check them before relying on them.

## Current state

Seventeen modules, 699 functions, 74 test fixtures. Each fixture runs on the interpreter
and again across interpreter, JIT and AOT for differential agreement.

| Module | Functions | Notes |
|--------|-----------|-------|
| `u8` `u16` `u32` `u64` | 55-60 each | bitwise, checked/saturating/wrapping, rotates, endian |
| `i8` `i16` `i32` `i64` | 48-53 each | as above plus `signum` and the `abs` family |
| `index` `offset` | 50, 53 | the renamed `usize`/`isize` |
| `f32` `f64` | 31 each | classification, rounding, `total_cmp`, bit casts |
| `string` | 60 | backed by native riders |
| `int` | 14 | predicates, sign, comparison, division, `pow`, `factorial` |
| `bool` | 6 | complete |
| `option` | 7 | **`?u32` only** |
| `result` | 5 | **`!u32` only** |

Nothing is stubbed. The bit operations, string operations, conversions and
checked/saturating/wrapping arithmetic the original report listed as missing runtime
intrinsics are all implemented, some as `icall` intrinsics and the string ones as native
riders.

## The real blocker: monomorphic option and result

`option` handles `?u32` and `result` handles `!u32`. Nothing else.

This is the sharpest pain in the library and it is felt hardest by the parts that work
best. `string` returns `?string`, `?index`, `?u8`, `?u32`, `?i32`, `?f32`, `?int` and
`?(string, string)`, and not one of those has an `is_some`, an `unwrap_or` or a
`to_option`. The library can produce these values and cannot help anyone use them.

Everything else on this list is smaller than this.

See [Generics and Specialization](../plan-generics.md). The conclusion relevant here is
that erasure gives `option`, `result`, `list`, `map` and `set` over any `T` without
monomorphizing anything, and that this is the step that should come before any
optimization work.

## Duplication in the numeric modules

Eight fixed-width integer modules of 48 to 60 functions each, differing mostly in a type
name and an intrinsic suffix. Substituting the type name, `u32` and `u64` differ in 178
of 511 lines, `i32` and `i64` in 208 of 463. Roughly two thirds of about 4,000 lines is
mechanical repetition.

Generics would not remove all of it, since the intrinsics are genuinely per-width, but it
would remove the two thirds that is `min`, `max`, `clamp`, `abs_diff`, `is_zero` and the
comparison wrappers written out eight times.

## Missing language features

**No closures.** Blocks every higher-order function: `map`, `filter`, `fold`,
`and_then`, `map_err`. This is the second largest gap after generics and is independent
of it. Backburner on the roadmap.

**No `for` loop.** Only `loop` and `loop while`. Iterating a collection means an index
variable and manual bounds handling, which is why no list module has been attempted even
monomorphically.

**No set operations.** Sets exist as a type with literals and no `contains`, `insert` or
`remove`. Noted in botspec appendix B.

**No `==` on strings.** Comparisons are numeric-only; string equality goes through
`string.eq` and `string.cmp`. Deliberate, but worth knowing before reaching for it.

## Corrections to the previous report

Recorded because these claims were believed for a while and shaped what was attempted.

**Linear types do not block `int`.** The previous report gave this as the highest-impact
blocker and the reason `int.dfm` did not exist. Comparisons do not move their operands,
so the pattern it said was impossible compiles as written:

```datalove
fun abs(self: int): int
  if self .< 0
    ret -self
  else
    ret self
  end if
end fun
```

`int.dfm` now exists and was written without difficulty.

**Tuple field access exists.** `.0` and `.field` are implemented, with restrictions on
projecting non-copy fields outside a ref context.

**The runtime intrinsics are done.** Bit operations, string operations, integer
conversion and the checked, saturating and wrapping arithmetic families are all
implemented.

**Not verified either way:** whether `@error` still fails to coerce to `!T` in script
context. It was listed as blocking tests of error paths and has not been rechecked.

## Known stale content elsewhere

`sys/std/result.dfm` carries a comment saying `or_result` and `and_result` "cannot be
reliably implemented due to linear semantics" and that the analysis incorrectly flags
valid patterns. The analysis bug it refers to was fixed. The comment should go and the
two functions should be written.
