# Const Parameter Implementation

How const parameter specialization works today, where it is wrong, and the plan to
replace union-branch with monomorphization.

The survey of the literature that led to the original design is in
[Const Parameter Specialization](const-param-specialization.md). The reasoning that
rejected this machinery as a foundation for generics is in
[Generics and Specialization](plan-generics.md). This document is the implementation
plan, and it has been rewritten: the union-branch plan it used to hold described a
strategy the compiler should stop using.

## Table of Contents

1. [What is built](#user-content-what-is-built)
2. [What the tests actually cover](#user-content-what-the-tests-actually-cover)
3. [Two faults](#user-content-two-faults)
4. [Why monomorphization](#user-content-why-monomorphization)
5. [The plan](#user-content-the-plan)
6. [Constraints to respect](#user-content-constraints-to-respect)
7. [Testing](#user-content-testing)
8. [Order of work](#user-content-order-of-work)
9. [Appendix: the union-branch record](#user-content-appendix-the-union-branch-record)

---

## What is built

The feature is wired end to end and `skip_specialization` defaults to `false`, so it
runs in the real compiler.

**Front end.** `FunParam::is_comptime` carries the `const` modifier from the parser.
`TypeFunction::param_comptime` carries it through the type system. `synthesize.rs:1202`
enforces the const-binding-only restriction: a comptime argument must be the name of a
`const` binding, not a literal, a `let`, or a parameter. The three refusals are covered
by `tycheck_world/05_comptime_arg_errors.world`.

**Call sites.** `lower/expr.rs:653` emits `Instruction::ComptimeCall` instead of `Call`
when the callee has comptime parameters. It carries the original arguments, the comptime
parameter indices, and a placeholder discriminant. Every consumer handles it: the
interpreter, the Cranelift backend, the C backend, the inliner, DCE, and the IR printer.
Where nothing rewrites it, it behaves as a plain `Call` to the original function.

**Specialization.** `specialize_comptime_functions` in `tracked_lower.rs:1191` runs as
phase 5c, after const evaluation and before assembly. It does three things:

1. `build_specialization_plan` walks the `ComptimeCallSiteRegistry` the typechecker
   filled in, resolves each recorded const binding *name* against the module graph's
   evaluated consts, and groups the resulting values into instantiations per function.
2. `transform_function` rewrites each comptime function into union-branch form: the
   comptime parameters are replaced by a single `i32` tag, and the body is cloned once
   per instantiation behind a `Switch` on that tag.
3. `rewrite_comptime_calls` turns each `ComptimeCall` into `Const(discriminant)` followed
   by a `Call`, dropping the comptime arguments the callee no longer takes.

The parts of this worth keeping are the substitution helpers
(`rewrite_comptime_params_in_instruction` and its terminator twin), `build_const_value_map`,
and the call-site rewriting shell. The tag, the `Switch`, and `build_dispatch_blocks` are
what goes.

## What the tests actually cover

Less than the fixture count suggests, and the gap runs in one direction: the cases the
suite claims to cover are the cases it does not.

Specialization only fires when a call site is *in a module function* and its const
resolves out of the module graph's `resolved_consts`, which holds module-level and
function-level consts. Script-unit consts are not in that map, so those call sites are
silently skipped and the whole plan comes back empty.

Of the 18 fixtures in `fixtures/specialize_differential/`:

- **Two specialize.** `011_module_internal` and `012_cross_module` are the only fixtures
  whose expected IR contains a `switch`. Both are `(const factor: int, x: int)`: a single
  comptime parameter in first position.
- **Fourteen are inert.** Their call sites are in script units, the plan comes back empty,
  and the harness compares two identical unspecialized runs.
- **Two do not run at all.** `013_comptime_i32` fails to typecheck (`*!` used in a
  function that does not return a `Result`). `016_nested_comptime` fails to lower with
  `double_add::const 'N': lowering error: binding not available yet: n` — a comptime
  function calling another comptime function does not work. Both are recorded as passing,
  because both sides of the differential fail identically.

So `param_remap` has no live coverage for a comptime parameter anywhere but index 0.
`006_comptime_last_position`, `008_two_comptime_params` and `020_interleaved_comptime`
exercise the unspecialized path only. The AOT fixture `aot/083_comptime_simple.world` is
a script call site, and its expected IR still shows `comptime_call` — specialization and
AOT have never been run together.

The harness itself is sound. `specialize_differential_analysis.rs` runs the worldfile
twice on fresh databases and compares both `debug_output` and `output` for every section.
The problem is the fixtures, and most of it dissolves on its own once the plan is sourced
from the IR.

## Two faults

Both are live in an on-by-default path.

**Same-named consts panic the compiler.** The registry records const *names* with no
scope, and `build_specialization_plan` looks them up by bare name against a map that adds
an unqualified alias on a first-wins basis. Two function-level consts sharing a name in
one module:

```datalove
fun twice(x: int): int
    const N: int = 2
    ret scale(N, x)
end fun

fun fivex(x: int): int
    const N: int = 5
    ret scale(N, x)
end fun
```

Both resolve to `2`, so `5` is never registered as an instantiation. `rewrite_comptime_calls`
then resolves the real value `5` out of the IR, finds no discriminant for it, and reaches
its own `panic!` at `specialize.rs:642`. The plan and the rewrite derive the same fact two
different ways and are free to disagree.

**A function called from both a module and a script miscompiles.** These two paths agree
today only because neither normally fires. Give one comptime function a module-internal
call site, which specializes it, and a script call site, which is never rewritten: the
script passes the raw const value into what is now the tag parameter. The `Switch` finds
no matching case, falls through to `default` — wired to the *last* variant — and runs the
wrong branch with the wrong constant. With `int` the discarded argument also trips the
runtime leak detector. Wrong answer and a leak, from ordinary code.

Neither fault is a bug in the union-branch transform as such. The first is the two-sources
problem; the second is what makes union-branch structurally unsafe here, and is the
subject of the next section.

## Why monomorphization

The size and compile-time case is already settled and recorded in
[Generics and Specialization](plan-generics.md) and `compiler-guide.md:215`:
`build_dispatch_blocks` clones every body block once per instantiation, so union-branch
is monomorphization plus a `Switch` on a value every call site passes as a literal, and
fusing instantiations into one symbol defeats the per-function counting in `optimizing.rs`.

The argument that matters most for this compiler is a different one, and it is what the
second fault is really about:

**Monomorphization is additive. Union-branch is destructive.**

Union-branch rewrites the callee's signature in place. Every call site in the program must
then be found and rewritten to match, or it calls a function that no longer takes what it
is passing. That is a whole-program obligation, and this compiler cannot discharge it:
script units are compiled *after* the module graph, one at a time, against modules that
were already specialized. A REPL line introducing a new instantiation arrives too late to
participate.

Monomorphization keeps the original function and adds copies beside it. A call site nobody
specialized still calls a function that still exists with the signature it had. That single
property:

- removes the second fault structurally, rather than by patching the dispatch default;
- makes the script and REPL path correct instead of accidentally correct, with no new
  machinery, at the cost of those call sites staying unspecialized;
- leaves specializing script call sites as a later optimization rather than a
  prerequisite, since a script unit can hold its own copies in `nested_units`.

The original may end up dead once every call site is specialized. Leaving it is the right
default for a language with a REPL, where a later script line may call it.

## The plan

### 1. Stop the bleeding

Flip `skip_specialization` to default `true` at `module_pipeline.rs:77` and
`workspace.rs:123`. That routes everything down the unspecialized path all 18 fixtures
confirm is correct. The differential harness sets both values itself and is unaffected.

This is independent of everything below and should not wait on it.

### 2. Source the plan from the IR

Delete the name-based lookup. Build the plan by scanning lowered units for `ComptimeCall`
and resolving each comptime argument operand with `build_const_value_map`, which is
exactly what `rewrite_comptime_calls` already does. Plan and rewrite then agree by
construction.

This removes, in order: the `resolved_consts` flattening in `tracked_lower.rs:1210-1227`,
the unqualified-name alias and its ordering hazard, `comptime_arg_names` on
`ComptimeCallSite`, and with it the first fault. The registry shrinks to what the IR does
not already carry; note that `ComptimeCall` carries `comptime_param_indices` itself, so
the call-site half of the registry has little left to say.

This is also what closes the script-unit gap, whenever step 6 is taken: a script unit's
`v1 = const 2int` feeding a `comptime_call` resolves exactly as well as a module's. The
reason script call sites are skipped today is entirely an artifact of the name lookup.

> A related lesson is already recorded: `ComptimeCallSite` used to carry a
> `call_expr_id: salsa::Id` that nothing read, because call sites are found in the IR.
> A raw salsa id inside a memoized value is not free even when unread — it takes part in
> the equality deciding whether typechecking can be reused. See `salsa-patterns.md`.
> Sourcing the plan from the IR is the same observation carried to its conclusion.

### 3. Replace the transform

Delete `build_dispatch_blocks` and `remap_terminator_blocks`. Replace `transform_function`
with a copy-per-instantiation:

- Clone the unit. Drop the comptime parameters from `params`, `param_types` and
  `param_modes`; renumber what remains. There is no tag parameter, so the new indices
  start at 0 rather than 1.
- Prepend a `Const` instruction per comptime parameter to the entry block, and substitute
  through the body with the existing `rewrite_comptime_params_in_instruction` and
  `rewrite_comptime_params_in_terminator`. The body's own `drop` of what used to be the
  parameter becomes a drop of the const, which is correct.
- No block renumbering. The copy keeps the original block structure.

**Make the substitution helpers exhaustive.** They currently end in `_ => instr.clone()`
and handle about fourteen variants. Under union-branch an unhandled instruction kept a
`Param` reference that still existed, so the catch-all was survivable. Under
monomorphization the parameter is gone and an unrewritten reference dangles. This must be
an exhaustive match, which the house rule against fallback code wants anyway.

**Carry the context across.** `transform_function` currently blanks `tracked_params`,
`descriptor_shapes`, `symbols`, `const_values` and `nested_units`. The copy must keep all
of them, with `tracked_params` and `descriptor_params` remapped through `param_remap`.
`tracked_params` is what the Cranelift and C backends use for parameter tracking, and
losing it is waiting for the AOT path to be exercised.

### 4. Give the copies an identity

This is the genuinely new work. Union-branch sidestepped it by keeping one function per
source function.

**Ids.** `compute_func_id_map` is `#[salsa::tracked]` and assigns `FuncId`s from source
statement order. Specialized copies do not exist in the source and cannot come from it.
Allocate per module, starting past the highest source-derived `FuncId` for that module.
`FuncId` and `CodeUnitId` are numerically interchangeable here and the code converts
freely between them.

**Names.** Both AOT backends use `unit.name` directly as a linker symbol — Cranelift as
`__mod_{module_id}_{name}` at `cranelift-aot/src/lib.rs:215`, and the C backend as a C
identifier. So the suffix must be identifier-safe: `scale__ct0`, not `scale$$0`.

**Registration.** Add copies to `ModuleLoweredFunctions.functions` and
`func_name_to_id`, keeping `functions` sorted by `id.0` — the stratified two-pass lowering
at `tracked_lower.rs:1108` already relies on that ordering. `lower_module` reuses
`lowered_functions` wholesale when it is provided (`tracked_lower.rs:358`), so copies
added in phase 5c reach assembly without further plumbing. They must be registered before
`first_uncallable_target` (`tracked_lower.rs:863`) runs, since it validates every
`CodeRef::Module` target against the lowered list.

### 5. Rewrite the call sites

Keep the shape of `rewrite_comptime_calls`, minus the discriminant. A `ComptimeCall` whose
instantiation is in the plan becomes a `Call` to the copy's `CodeRef`, with the comptime
arguments removed from the argument list and dropped beforehand — the existing
`comptime_args_to_drop` logic is still needed, since the caller's const value is linear
and the callee no longer consumes it.

A `ComptimeCall` whose instantiation is *not* in the plan is left alone. It is a correct
call to the original function. Delete the `panic!`: under an additive design there is
nothing to panic about, and the condition it was guarding against is no longer a fault.

### 6. Optional: specialize script call sites

Not required for correctness, and worth doing only if measurement asks for it. A script
unit can hold copies in its own `nested_units` addressed by `CodeRef::Local`, which keeps
the module untouched and works incrementally. Defer until steps 1 through 5 are in and
the AOT path has coverage.

## Constraints to respect

**Const parameters and generics do not combine.** `lower/expr.rs:653` takes the
`ComptimeCall` branch before the `type_args` computation and so never computes them, and
`rewrite_comptime_calls` emits empty `type_args` and `shape_descriptors` with the comment
that a comptime call is not generic. A function that is both will silently lose its
descriptors. Either reject the combination in the typechecker or compute `type_args` on
both branches; the current state is an unstated assumption in two places.

**Float const parameters are unsound as an instantiation key.** `ConstValue` hashes `F32`
and `F64` by `to_bits` (`ir/src/lib.rs:1033`) but derives `PartialEq`, which uses float
comparison. `0.0` and `-0.0` compare equal and hash differently; `NaN` hashes equal to
itself and compares unequal. Any map keyed on `Vec<ConstValue>` inherits that. Canonicalize
the key or refuse float const parameters.

**Nested comptime does not lower.** A const binding in a comptime function's body cannot
name that function's comptime parameter (`016_nested_comptime`). Monomorphization makes
this tractable — inside a copy the parameter *is* a const — but it is a separate change
and should get its own fixture and its own commit.

**There is still no instantiation limit.** The original plan called for one and it was
never built. Under monomorphization it matters more, since each instantiation is a real
function in the object file. A limit with a clear error beats silent code growth.

## Testing

Keep the harness. It is strategy-independent and it is the valuable part of the existing
work.

Fixtures to add, none of which exist today:

- The two faults, as regressions: same-named function-level consts in one module, and a
  comptime function called from both a module function and a script unit.
- A comptime parameter in middle and last position, and two comptime parameters, with the
  call sites in module functions so they actually specialize. `006`, `008` and `020` test
  these shapes against the unspecialized path only; they need module-side twins.
- An AOT fixture that actually specializes. `aot/083` does not.
- Fix `013_comptime_i32`, which has never typechecked.

Sixteen fixtures go live on their own once step 2 lands, with no edits. That corpus is
latent coverage, not dead weight, and it is the main reason to build on this rather than
start over.

Blessing note: the expected IR for the two live fixtures will change shape, from one
function with a `switch` to an original plus copies.

## Order of work

- [ ] 1. Default `skip_specialization` to `true`. Independent, cheap, stops live
      miscompiles.
- [ ] 2. Source the plan from the IR. Removes the first fault and most of
      `build_specialization_plan`.
- [ ] 3. Monomorphizing transform, with exhaustive substitution and full context carried
      across.
- [ ] 4. Ids, names and registration for the copies.
- [ ] 5. Call-site rewriting without the discriminant; delete the panic. Re-enable
      specialization by default.
- [ ] 6. Fixtures: the two regressions, module-side twins for the parameter positions,
      an AOT case.
- [ ] 7. Instantiation limit with a clear error.
- [ ] 8. Reconsider the const-binding-only restriction. `pow(2, 10)` failing because `10`
      is a literal is a bad first impression, and once the plan comes from the IR rather
      than from pre-resolved consts, the restriction is easier to lift.

Steps 3 through 5 land together or not at all; the intermediate states do not compile to
anything coherent.

## Appendix: the union-branch record

Kept because the reasoning should stay recoverable, not because it should be followed.

Union-branch generated one function with N branches dispatching on a tag, instead of N
copies. It was adopted on this comparison:

| Aspect | Full mono | Union-branch |
|--------|-----------|--------------|
| Code size | O(N x func) | O(func + N x branch) |
| Compile time | O(N x func) | O(func + N) |
| Icache | Poor (N copies) | Good (1 function) |
| Branch cost | None | ~2-5 cycles |

Every row of which is wrong, because a branch is not cheaper than a copy of the body — it
*is* a copy of the body. `build_dispatch_blocks` clones every original block once per
instantiation. Corrected:

| Aspect | Full mono | Union-branch |
|--------|-----------|--------------|
| Code copies | N bodies | N bodies, plus a dispatch block |
| Instruction cache | N copies | The same N copies in one symbol |
| Branch prediction | No branch | A `Switch` on a per-call-site constant |
| Inlining | Each copy inlinable | One oversized body, inlined whole or not at all |
| Compile time | O(N x size) | O(N x size) plus dispatch |
| JIT tiering | Per instantiation | All instantiations share one call count |

The last row is specific to this compiler: `optimizing.rs` inlines at 50 calls and
JIT-compiles at 100, counted per function, so fusing instantiations means a hot one cannot
tier without dragging the cold ones with it.

The extension to type parameters, once sketched here as phases B through D, does not
exist. Union-branch holds one signature by removing the const parameter; a type parameter
*is* the type of other parameters and of the return, so there is nothing for the branches
to agree on. Generics went to erasure and type descriptors instead. The full argument is
in [Generics and Specialization](plan-generics.md).

Problems found and fixed while the union-branch implementation was built, which were real
and are not affected by any of the above: CTFE leaking heap values returned through a call
and leaking collections with linear elements; calls in binary operand position panicking
`emit_expr_temp_drops_since`; dead code elimination not running on the two function
lowering paths, which orphaned the instructions computing an inlined const; module-level
consts, implemented by stratifying lowering into two passes; and a widening fault where a
fixed-width integer passed to an `int` parameter produced zero. Fixtures: interp 944, 945,
946, dual 423, module_interp 055 and 056.
