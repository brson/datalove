# Const Parameter Implementation

How const parameter specialization works, and what is left to do.

The survey of the literature that led to the original design is in
[Const Parameter Specialization](const-param-specialization.md). The reasoning that
rejected this machinery as a foundation for generics is in
[Generics and Specialization](plan-generics.md).

This document has been rewritten twice: once when union-branch was replaced as a plan,
and again when the replacement was built. The union-branch record is kept in an appendix.

## Table of Contents

1. [What is built](#user-content-what-is-built)
2. [Why monomorphization](#user-content-why-monomorphization)
3. [Two faults, and where they went](#user-content-two-faults-and-where-they-went)
4. [What the tests cover](#user-content-what-the-tests-cover)
5. [Constraints](#user-content-constraints)
6. [What remains](#user-content-what-remains)
7. [Appendix: the union-branch record](#user-content-appendix-the-union-branch-record)

---

## What is built

**Front end.** `FunParam::is_comptime` carries the `const` modifier from the parser.
`TypeFunction::param_comptime` carries it through the type system. `synthesize.rs:1202`
enforces what a comptime argument may be: the name of a `const` binding, and nothing
else. Not a `let`, not a parameter, not a literal, not an expression. The value a const
parameter takes has to be one the compiler already holds, and a binding is the one form
that says so on its face; every other shape would mean the compiler deciding case by case
which ones it can see through, which is how a rule stops being one. A const parameter
counts, being a const binding within the body, which is what lets one comptime function
pass its parameter to another. Covered by `tycheck_world/05_comptime_arg_errors.world`.

**Call sites.** `lower/expr.rs:653` emits `Instruction::ComptimeCall` instead of `Call`
when the callee has comptime parameters, carrying the original arguments and the comptime
parameter indices. Every consumer handles it: the interpreter, the Cranelift backend, the
C backend, the inliner, DCE, and the IR printer. Where nothing rewrites it, it is a plain
`Call` to the original function.

**Specialization.** `specialize_comptime_functions` in `tracked_lower.rs` runs as phase
5c, after const evaluation and before assembly, over three functions in `specialize.rs`:

1. `collect_instantiations` scans the lowered units for `ComptimeCall` and resolves each
   comptime argument operand against the `Const` instruction that defines it, grouping the
   distinct value tuples per callee. The callee is keyed by `(IrModuleId, CodeUnitId)`,
   and modules are visited in `IrModuleId` order, so the numbering is deterministic.
2. `monomorphize_function` builds one copy per instantiation: the comptime parameters are
   dropped from the signature, the rest are renumbered, and a `Const` per comptime
   parameter is prepended to the entry block and substituted through the body. The body's
   own drop of what used to be the parameter becomes a drop of the const, so the ownership
   accounting carries over unchanged.
3. `rewrite_comptime_calls` points each call site whose instantiation was built at the
   copy, drops the const arguments the copy does not take, and leaves everything else
   alone.

Making a copy also evaluates that function's const bindings for the instantiation.
A const naming a const parameter has a value per instantiation rather than one, so phase
5b leaves it alone -- there is nothing to evaluate while the parameter is still a
parameter -- and `evaluate_instantiation_consts` seeds the parameters with what the
instantiation passes and runs the same CTFE every other const goes through.
`inline_function_consts` writes the results into the copy, which also simplifies a branch
whose condition has become constant. So `const` means the same thing inside a comptime
function as outside one: `const PLUS: int = n + 1` and `const VIA_FUN: int = helper(n)`
are constants in the copy, not the instructions that would compute them, and either can
be passed on as a const argument.

Steps 1 and 2 run a round at a time, up to `MAX_ROUNDS`. A comptime function only says
what it passes to another one once its own const parameters have been substituted, so a
round of copying can uncover instantiations the scan before it could not see. It
terminates on its own -- the callees are the program's functions and each is capped -- and
the round cap is a backstop rather than a limit anything reaches. Running out of rounds
leaves call sites unspecialized rather than wrong, which is the same state a call site the
pass cannot see is in.

**Script units** are specialized separately, by `specialize_script_unit`, called from
`ScriptCompiler::phase_specialize`. A script unit compiles after the module graph and one
at a time, so it cannot add to a module; instead its copies go in its own `nested_units`,
reached by `CodeRef::Local`, which is the same vehicle a script-local function already
uses and which every backend already handles. A script line may therefore name an
instantiation no module call site asked for and get its own copy, without the module it
came from changing. The callee is read out of the module registry, or out of the script's
own nested units when the comptime function was defined beside the script.

This runs after the shape descriptors are resolved, so a copy arrives with the ones its
own module worked out rather than having them recomputed against a script that has no type
parameters of its own. It needs the const arguments to have survived as `Const`
instructions, so under `skip_const_inlining` nothing specializes and the calls run the
original -- which is correct, just not specialized.

**Substitution** is `replace_params_in_instruction` and `replace_params_in_terminator` in
`datalove-datafun-ir/src/params.rs`, shared with the inliner, which needs the same thing
for a different reason. Both matches are exhaustive: a variant that fell through would
leave a reference to a parameter the copy no longer has, and nothing downstream reports
that. Moving it there found two variants, `UnitEndDrop` and `UnitEndDropTracked`, that the
inliner's copy had grouped in with the operand-free ones and so never substituted.

**Identity.** `compute_func_id_map` is `#[salsa::tracked]` and assigns `FuncId`s from
source statement order, which cannot cover functions that are in nobody's source. Copies
take ids after the highest source-derived one in the unit they go into, and are named
`{original}__ct{n}`. The name has to be an identifier because both AOT backends use it as
a linker symbol -- Cranelift as `__mod_{module_id}_{name}` for a module function and the
bare name for a nested one, the C backend as `__mod_{module_id}_{name}` and
`__local_{name}`.

Module copies number `n` by instantiation, which is unambiguous because the symbol carries
the module. Script copies number it by unit id instead: a nested unit's symbol carries
nothing to tell it from another unit's, so two callees that happened to share a name would
otherwise produce two copies that did. Nothing can currently name two such callees from
one script -- imports cannot be aliased -- but the copies are made distinct rather than
left resting on that.

Module copies are registered in `ModuleLoweredFunctions`, which `lower_module` reuses
wholesale, so they reach assembly, the runtime registry and the IR dumps without further
plumbing.

**Limit.** `MAX_INSTANTIATIONS` is 64. Over that, the function is left unspecialized and
the module gets a lowering error naming it, which travels the channel module const
failures already use.

## Why monomorphization

The size and compile-time case against union-branch is recorded in
[Generics and Specialization](plan-generics.md) and `compiler-guide.md`:
`build_dispatch_blocks` cloned every body block once per instantiation, so union-branch
was monomorphization plus a `Switch` on a value every call site passed as a literal, and
fusing instantiations into one symbol defeated the per-function counting in
`optimizing.rs`.

The argument that decided it is a different one:

**Monomorphization is additive. Union-branch was destructive.**

Union-branch rewrote the callee's signature in place. Every call site in the program then
had to be found and rewritten to match, or it would call a function that no longer took
what it was passing. That is a whole-program obligation, and this compiler cannot
discharge it: script units compile *after* the module graph, one at a time, against
modules already specialized. A REPL line introducing a new instantiation arrives too late.

Keeping the original beside the copies means a call site nobody specialized still calls a
function that exists with the signature it had. Those calls are not specialized -- which
is the same amount of specialization the script path got before, but correct by
construction rather than by accident.

## Two faults, and where they went

Both were live in an on-by-default path, and both are gone with the mechanism that caused
them.

**Same-named consts panicked the compiler.** The old plan resolved a call site's comptime
argument by the *name* of the const binding the typechecker had recorded, against a map
with a first-wins unqualified alias. Two function-level consts sharing a name in one
module resolved to the same value, so the second instantiation was never planned for, and
`rewrite_comptime_calls` -- which resolved the real value from the IR -- found no
discriminant and hit its own `panic!`. The plan and the rewrite derived the same fact two
ways and were free to disagree. Now there is one derivation. Fixture:
`specialize_differential/022_shared_const_name`.

The names were carried by `ComptimeCallSiteRegistry`, which nothing else read, so it went
with them, along with `ComptimeCallSite` and the fields holding both on
`SingleModuleTypecheckResult` and `ModuleGraphTypecheckResult`. Those are salsa-tracked,
where an unread field is not free: it takes part in the equality that decides whether
typechecking can be reused. The typechecker still enforces what a comptime argument may
be; it just records nothing. See `salsa-patterns.md`.

**A function called from both a module and a script miscompiled.** The module call site
specialized the callee; the script call site was never rewritten, so it passed its const
value into what had become the tag. The `Switch` matched no case, fell to `default` --
wired to the last variant -- and ran the wrong branch with the wrong constant, leaking the
argument on the way. Keeping the original removes the condition entirely. Fixture:
`specialize_differential/023_module_and_script_call`.

## What the tests cover

`specialize_differential_analysis.rs` runs a worldfile twice on fresh databases, with
specialization on and off, and compares both `debug_output` and `output` for every
section. That harness is sound. What it was pointed at was not, and still is only partly.

Of the 23 fixtures in `fixtures/specialize_differential/`, 21 specialize, and their
expected IR is where the `__ct` copies appear. The two that do not --
`000_no_comptime` and `007_str_no_comptime` -- have no const parameters to specialize,
which is what they are for.

No `comptime_call` survives in any fixture's expected IR. Every call site either reaches a
copy or is one of those two.

`016_nested_comptime` is the one that needed the rounds: a const naming a const parameter,
and a comptime function calling another with it, over both a module caller and a script
one. It used to be the fixture that would not lower.

`026_shared_instantiation` covers two call sites naming one instantiation, which reach
one copy, along with a fixed-width and a string const parameter.

`024_module_side_positions` covers the parameter renumbering: a comptime parameter last,
two of them, and two interleaved among three ordinary ones, all called from module
functions. Before it, every live fixture had a single comptime parameter at index 0, so
`param_remap` was never exercised away from the identity.

`013_comptime_i32` is the only live fixture whose body has more than one block. Its
checked multiply early-returns on overflow, so the copy has to keep the branch and its
targets, and it covers a fixed-width const value rather than a bigint. It had never
typechecked before -- bare arithmetic on a fixed integer is not permitted, and it used a
checked operator in a function that did not return a result.

`aot/084_comptime_specialized` is the first fixture to run specialization and AOT
together. `module_interp/064_const_param_limit` covers the instantiation limit; it lives
there rather than in the differential suite because the limit is a refusal that only
exists when specialization runs, so the two sides are *meant* to differ.

## Constraints

**Reading a const parameter does not consume it.** It is a constant, not a place holding
the only copy of a value, which is the rule a `const` binding has always had -- stated in
`lower/context.rs` and enforced by `BindingInfo::is_const` in ownership analysis. A const
parameter simply was not registered as one, so passing a linear one to two calls was a
use-after-move, and `n@` could not fix it because a const argument must be a name.

A read that would consume takes a copy of its own; a borrow takes none, since it never
consumed anything. That distinction matters: cloning in a borrow context too costs an
allocation and a free per operand of every `n + n`, for nothing.

This does not reach inside an aggregate. `PAIR.0` on a `const PAIR: (int, int)` is
`NonCopyFieldProjection` whatever the root is, and `PAIR.0@` is refused as well, so a
const of an aggregate type can be passed along whole and not read into --
`017_comptime_tuple` says as much in its own comment. Destructuring is not refused:
`if MAYBE |val|` on a `const MAYBE: ?int` unwraps a fresh copy and works. So the two
disagree, and the projection rule is where it would have to be settled: it exists to stop
a non-copy field moving out of a place someone still holds, which is not what reading a
constant does.

**Const parameters and generics do not combine, and are refused.** `lower/expr.rs:653`
takes the `ComptimeCall` branch before the `type_args` computation and so never computes
them, and `rewrite_comptime_calls` emits empty `type_args` and `shape_descriptors`, so a
function with both would lose the descriptors its type parameters need and read its values
at the wrong type. `TypeError::ComptimeParamOnGeneric` reports it at the definition, which
catches the function whether or not anything calls it. Lifting the refusal means computing
type arguments on the comptime branch too, and carrying them through the copy.

## What remains

- **Dead copies.** A function all of whose call sites were specialized keeps an original
  nobody calls. Leaving it is deliberate -- a later script line may call it -- but for an
  AOT build, where there is no later script line, it is dead weight.
- **Decide whether the tag ever earns its place.** Monomorphization is right when the
  value is known at the call site, which the rule about const arguments guarantees. If
  that restriction is ever lifted far enough that a call site can pass a value chosen at
  runtime from a known set, the dispatch that union-branch built becomes the right shape
  for that case. It is not the right shape for this one.

## Appendix: the union-branch record

Kept because the reasoning should stay recoverable.

Union-branch generated one function with N branches dispatching on a tag, instead of N
copies. It was adopted on this comparison:

| Aspect | Full mono | Union-branch |
|--------|-----------|--------------|
| Code size | O(N x func) | O(func + N x branch) |
| Compile time | O(N x func) | O(func + N) |
| Icache | Poor (N copies) | Good (1 function) |
| Branch cost | None | ~2-5 cycles |

Every row of which is wrong, because a branch is not cheaper than a copy of the body -- it
*is* a copy of the body. Corrected:

| Aspect | Full mono | Union-branch |
|--------|-----------|--------------|
| Code copies | N bodies | N bodies, plus a dispatch block |
| Instruction cache | N copies | The same N copies in one symbol |
| Branch prediction | No branch | A `Switch` on a per-call-site constant |
| Inlining | Each copy inlinable | One oversized body, inlined whole or not at all |
| Compile time | O(N x size) | O(N x size) plus dispatch |
| JIT tiering | Per instantiation | All instantiations share one call count |

The last row is specific to this compiler: `optimizing.rs` inlines at 50 calls and
JIT-compiles at 100, counted per function, so fusing instantiations meant a hot one could
not tier without dragging the cold ones with it.

The extension to type parameters, once planned here as phases B through D, does not exist.
Union-branch held one signature by removing the const parameter; a type parameter *is* the
type of other parameters and of the return, so there was nothing for the branches to agree
on. Generics went to erasure and type descriptors instead. The full argument is in
[Generics and Specialization](plan-generics.md).

Problems found and fixed while the union-branch implementation was built, which were real
and are not affected by any of the above: CTFE leaking heap values returned through a call
and leaking collections with linear elements; calls in binary operand position panicking
`emit_expr_temp_drops_since`; dead code elimination not running on the two function
lowering paths, which orphaned the instructions computing an inlined const; module-level
consts, implemented by stratifying lowering into two passes; and a widening fault where a
fixed-width integer passed to an `int` parameter produced zero. Fixtures: interp 944, 945,
946, dual 423, module_interp 055 and 056.

One piece of the union-branch implementation survives in spirit. `ComptimeCall` was added
so that a later pass could find the call sites in the IR, and degrades to `Call` in every
backend. That is what makes the additive design possible, and what lets the plan be built
from the IR rather than from names.
