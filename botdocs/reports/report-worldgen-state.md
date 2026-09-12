# The state of worldgen

Written after being asked whether the generator could be extended to cover
generics, or whether it wanted redesigning.

**It wants extending, not redesigning.** What looked like a generator too
fragile to trust was three small bugs and one defensive clamp put in to route
around the first of them.

## What was wrong

**One-element tuple literals were printed `(x)`.** A 1-tuple needs the trailing
comma -- `(x,)` -- because `(x)` on its own is the expression `x` in brackets.
`mandocs/datalit.md` says so; the printer did not do it. A generated
`let v0: (u32) = (: u32 / 205)` then failed to typecheck against its own
declared type.

Someone met this and clamped `gen_type_hint` and `gen_type_alias` to
`TypeWeights::leaf_only()`, with the note "to ensure reliable typechecking".
That zeroes the weight of every compound type, so from then on **no generated
function signature or type alias held a list, a map, a set, an option, a
result, a tuple, a struct, an enum or a tensor**. The generator looked
trustworthy because it had stopped generating most of the language.

Measured, before and after fixing the printer, over the module graph:

| type weights | seeds failing typecheck |
|---|---|
| `leaf_only` (what it was doing) | 0 / 1000 |
| full, before the fix | 52 / 400 |
| full, after the fix | 0 / 2000 |

Of the 52, 51 were the 1-tuple. One was a knock-on arity mismatch.

**The script section imported the same name twice.** Every module names its
functions `fn0`, `fn1`, so two of them export the same name, and a second
import of a bound name is an error rather than a shadowing. `gen_module` had
worked this out and guarded against it, with a comment. `gen_script` never
did.

**A branch moved what was declared outside it.** `gen_if` saved the consumed
set, let the branch move whatever it liked, and restored -- so a linear value
moved in the then-branch and not the else, which is refused (D008). `gen_loop`
had met the same problem and solved it, by marking outer linear variables as
borrow-only for the length of the body. The same rule works for a branch.

The two of these together are what the dual-backend test was tripping over:

| | dual test, 20 seeds |
|---|---|
| before | 5 passed, 15 failed |
| after the import fix | 19 passed, 1 failed |
| after the branch fix | 20 passed, 0 failed |

None of the 15 were backend divergences. Both backends failed identically,
with a compile error, on a program the generator should not have written.

## What it found immediately

With compound types back in signatures, the dual test turns up a real
divergence within twenty seeds. Worldfile seed 2, twenty-three lines:

```
----------
module local/gen/core_0
----------

type Type0: u32
type Type1: i32

fun fn0(): string
  let v0: bool = false
  ret "with\"quote"
end fun

----------
scriptunit-fragment
----------

require module local/gen/core_0
import core_0.fn0

var v0: [bool] = [: bool / true, : bool / false, : bool / false]
var v1: [i32] = [: i32 / 73]
let v2: {} = {}

debuglog v2
```

The interpreter prints `{}` and exits; the cranelift AOT dies on a signal.
Reproduce with:

```
WORLDGEN_DUAL_TEST=1 WORLDGEN_DUAL_SEED=2 \
  cargo test -p datalove-tests --test worldgen_dual_tests
```

It needs the module: the script fragment alone agrees between the two, and so
does each piece of it on its own -- an empty anon struct, an empty tuple, an
empty list, a struct with a field. The module's function is imported and never
called. Not diagnosed further here.

## Where that leaves it

The generator covers modules, functions, control flow, cross-module imports,
type aliases and now every shape of type. What it does not cover:

- **Generics.** No type parameters, no bounds, no generic calls. This is the
  extension worth making, and it is where the compiler's bugs have been: four
  found by hand in erased-generic paths in one sitting, three of them silent
  wrong answers or memory corruption.
- **Riders and natives**, `match`, tables, `out` parameters, const parameters.

Two things about the harness itself:

- `worldgen_dual_tests` is behind `WORLDGEN_DUAL_TEST=1`, so it never runs in
  `just test`. At 5/20 it could not have. At 20/20 it could, once the seed-2
  divergence is dealt with.
- `test_1000_seeds_typecheck` is `#[ignore]`d for time. It takes about seven
  seconds.
- The typecheck-only tests do not run ownership analysis, so they cannot see a
  D008 or a leak. Only the dual test compiles and runs. That is why the
  branch-move bug survived in a generator whose seeds all "passed".
