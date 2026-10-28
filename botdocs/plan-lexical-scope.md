# Lexical Scope at the Top Level

Written October 2026, after a script const calling a function that names
another const was found to panic. Phase 1, lexical name resolution, is
implemented; the const walk is not.

A function body is read top to bottom: a name is in scope from where it is
bound. The top level of a module or script is not. There most declarations
are hoisted, a few are lexical, and consts that call functions are ordered by
a two-stratum scheme that differs between modules and scripts and panics in
scripts. The rule proposed here is that **every scope is lexical, and
functions are the one exception**: a function is in scope throughout its
scope, so functions may call each other in any order. A const may then depend
only on what is written above it, including through the functions it calls,
and consts are evaluated in one walk in source order, with no strata and no
fixed point.

## Contents

- [What resolves how today](#user-content-what-resolves-how-today)
- [Why the mixture costs](#user-content-why-the-mixture-costs)
- [The rule](#user-content-the-rule)
- [Consts, and the one walk](#user-content-consts-and-the-one-walk)
- [What it costs to compile](#user-content-what-it-costs-to-compile)
- [What breaks](#user-content-what-breaks)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## What resolves how today

Measured with small modules and scripts, one case each:

| Name | Module scope | Script unit | Across REPL units |
|---|---|---|---|
| `require`, and its alias | hoisted | hoisted | lexical |
| `import` | hoisted, even above its `require` | hoisted | lexical |
| type alias, where used | hoisted | hoisted | lexical |
| type alias naming another | lexical (F064) | lexical | lexical |
| function | hoisted, mutually recursive | hoisted, mutually recursive | lexical |
| const naming a const | lexical (F001) | lexical | lexical |
| function body naming a const | hoisted: sees every module const | lexical: sees those above the `fun` | lexical |
| `require data` | hoisted for bodies | lexical | lexical |
| `let`, `var` | -- | lexical | lexical |

A const may call any function, since functions are hoisted. What happens next
depends on where the const is:

- **In a module**, the functions naming no module const are lowered first,
  then the module consts are evaluated, then the rest are lowered
  (`lower_all_module_functions` twice in `tracked_lower.rs`). A const that
  reaches a function of the second stratum is refused by
  `first_uncallable_target` as "depends on a function that is not available
  yet, which means it and that function depend on each other", whether or
  not they do. `const A = 2`, `fun f() ret A`, `const C = f()` is refused,
  though nothing loops.
- **In a script**, the same two strata run per unit, but const evaluation is
  given no `callable` to check against, so a const reaching a deferred
  function panics in lowering: "local function 'f' not found - typechecker
  should catch this". The same `A`, `f`, `C` panics.

## Why the mixture costs

Each kind of name has its own answer to "where is this visible", so there is
no one sentence that says what a program means. Hoisting also makes the
dependency between consts and functions a graph to be ordered, and the two
strata are an ordering that is wrong in both directions: it refuses programs
with no cycle, and in scripts it misses the cycle it should refuse. Fixing
that within the hoisted model means computing an order -- a topological sort
or a fixed point over strata -- in const evaluation, and the same again
wherever another pass meets the same graph. Each is a small algorithm with its
own compile-time cost and its own way to be wrong.

A function body has none of this, because it is lexical. A script unit is
already very nearly a function body with declarations in it.

## The rule

**A declaration is in scope from where it is written to the end of its scope.
A function is in scope throughout its scope.** A function's signature and its
body see what is written above the function, and every function of the
scope.

This is one rule for every scope:

- **A function body** declares no functions, so it is lexical, as now.
- **A script unit** is a function body that may declare functions. They are
  hoisted within the unit.
- **A REPL session** continues one scope across its units. A unit sees the
  units before it; a function is hoisted only within its own unit. This is
  how sessions work already.
- **A module** is a script unit without `let` and `var`.

What changes is that `require`, `import`, `type` and `require data` become
lexical everywhere, and a module function's body sees only the consts above
it. A name used above its declaration is the error it would be in a function
body: F001 for a value, F064 for a type, F078 for a module alias.

Mutual recursion stays because writing without it is too awkward: helpers
would have to be ordered by hand, and two functions that call each other could
not be written at all.

## Consts, and the one walk

A const sees only what is above it. Hoisted functions are the one way past
that, so the rule has to hold through them: **a const's value may depend only
on declarations above it, including through the functions it calls.**

```datalove
const A: i64 = 2
fun f(): i64
  ret A
end fun
const C: i64 = f()     // fine: A is above C
```

```datalove
const C: i64 = f()     // C reaches D, declared after it
const D: i64 = 1
fun f(): i64
  ret D
end fun
```

A cycle is the case `D = C`. It needs no rule of its own and gets no message of
its own.

**Evaluation is one walk in source order:**

1. At each const, lower each function its initializer can reach that is not
   lowered yet, then evaluate it.
2. A function lowered there that names a const with no value yet is the
   error, reported at the const with the chain: "`C` calls `f`, which names
   `D`, declared after `C`".
3. After the walk, lower the functions no const reached.

Step 2 is what lowering does already when it meets `BindingNotAvailable`; it
stops deferring and reports instead. The walk replaces both strata, the
deferral, and `first_uncallable_target`, in modules and scripts alike.

A function lowered for one const stays valid for every const after it: it
names only consts already evaluated, or it would have been the error. So the
walk shares one visited set across all the consts, and no function is lowered
or walked twice.

Modules form a DAG under `require` (a cycle is F083), so each module's walk
needs only the modules it requires to be finished, not the whole graph.

## What it costs to compile

With *n* declarations, *K* module consts, *B* the lowered IR of all bodies and
*E* the call edges:

| | Today | Proposed |
|---|---|---|
| Name resolution and typecheck | O(n + B) | O(n + B) |
| Function lowering | up to 2B, plus work thrown away on deferred bodies | B |
| Const reachability | O(K * (B + E)): a fresh `seen` per const | O(B + E): one visited set |
| Running the consts | the same | the same |
| Ordering across modules | a barrier over the whole graph between the strata | per module, in dependency order |

Running the consts dominates whenever they compute much, and is unchanged.
These are estimates from the code; nothing has been timed.

Lexical scope makes "is this name visible here" depend on position. If
positions are plain indices, inserting a declaration near the top of a module
shifts every index below it and every memo keyed on one. The comparison
should be by stable identity in declaration order, not by index.

## What breaks

Measured by reading every `.dfm`, `.dfs`, `.repl` and `.world` file in `sys`,
`demos` and `crates` in declaration order, flagging every `require` alias,
`import`, `type`, const and `require data` used above its declaration
(functions skipped, since they stay hoisted). The scan was checked against the
known cases above, all of which it finds.

**One fixture breaks, and it was meant to.**
`crates/datalove-datafun/tests/fixtures/engines/803_type_alias_forward_ref_error.world`
says "Error: forward reference to type alias (used before defined)", but its
expected output records success: it was blessed after aliases were hoisted.
Under this rule it gets the error it was written for.

Nothing in `sys/std`, the demos or the other fixtures uses a name above its
declaration. The const rule breaks nothing by construction: in a module, a
const that calls a function naming any const is refused today, and in a
script it panics, so no passing program does either.

**Not measured:** datafun source embedded in Rust tests, in about thirty
files. The test suite finds those in phase 2.

## Phases

1. **Name resolution, lexical.** For modules, replace "collect everything,
   then check" (`resolve_names_inheriting`, the module typecheck) with a
   pre-pass collecting function names and one walk in source order, resolving
   each signature at its position and checking each body against its
   position's scope. For scripts, make `require`, `import` and `type` lexical
   within a unit. New errors for a name used above its declaration. Fix
   fixture 803.
2. **The const walk.** Replace both two-stratum schemes with the walk, the
   deferral and `first_uncallable_target` with the error at the const. This
   closes [A script const that calls a function declared after it
   panics](issues.md#user-content-a-script-const-that-calls-a-function-declared-after-it-panics).
3. **Docs.** A scope section in `botspec.md`, rewriting 8.2's ordering
   paragraph; the typing rules; the compiler guide.

Each phase leaves `just test` passing and is committed on its own.

## Open questions

- **Is an `import` lexical, or hoisted like a function?** Lexical here: it
  binds a name from elsewhere, like `require`, and imports are written at the
  top anyway. Hoisting it would be a second exception for a convenience
  nothing in the measured code uses.
- **Must a type alias in a signature be declared above the function?** Yes
  here, under the plain rule. Since aliases are structural, a signature naming
  one declared below would mean the same thing; exempting signatures would
  cost an exception and buy nothing the measured code uses.
- **Should the const check move into the typechecker?** The walk finds it in
  const evaluation, where it falls out for free. The typechecker records call
  targets, so a depth-first walk over the call graph there could report it a
  phase earlier, as the other name-resolution errors are.
