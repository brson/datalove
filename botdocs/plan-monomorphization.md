# Monomorphizing Generics

A plan to let any generic function be translated two ways: erased, as every one
is today, or monomorphized -- lowered again for one set of concrete type
arguments, into ordinary concrete IR -- with the call sites that reach it
monomorphized too, so that nothing is converted at the boundary. Which one is
used is a policy, per function and per call site, and the policy can be
anything: all erased, all monomorphized, a heuristic, or a seeded coin per
decision, as the chaos dispatcher already is for the jit. The eventual consumer
is the running program itself: the interpreter and the jit asking for an
instance when a generic call gets hot.

Background: [Generics: how it works](generics.md) is the erased implementation
this builds beside; [Generics and Specialization](plan-generics.md) is the
reasoning that chose erasure, and its "Layer 2: monomorphize what is hot" is
what this makes concrete. The motivation is measured in
[Are We Fast Yet for benchvs](research/research-awfy-benchmarks.md): after
everything else, `list.repeated` is about 85% of jit Sieve, and by cause it is
packing a bare `T` into a `data` and out again (~34%), borrowing a wrapped
`[T]` on every call (~10%), and the layers of calls between (~32%). The first
two are what every generic pays, and both go away in an instance.

## Contents

- [What an instance is](#user-content-what-an-instance-is)
- [Why lowering again rather than copying IR](#user-content-why-lowering-again-rather-than-copying-ir)
- [Lowering under a substitution](#user-content-lowering-under-a-substitution)
- [Ownership analysis per instance](#user-content-ownership-analysis-per-instance)
- [Call sites](#user-content-call-sites)
- [The instance plan](#user-content-the-instance-plan)
- [Policy](#user-content-policy)
- [Identity](#user-content-identity)
- [Salsa](#user-content-salsa)
- [Scripts and the REPL](#user-content-scripts-and-the-repl)
- [What the backends need](#user-content-what-the-backends-need)
- [Testing](#user-content-testing)
- [Asking at run time](#user-content-asking-at-run-time)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## What an instance is

An **instance** is a generic function lowered with its type parameters bound:
`(function, type arguments)`, the arguments being concrete typechecker types in
the function's declaration order. `repeated[bool]` is `list.repeated` with
`T := bool`. The erased version is not an instance; it is what every generic
always has, and what anything not monomorphized calls.

An instance is a complete, ordinary function. Its parameters and return have
their concrete types, it takes no descriptors (`descriptor_params` and
`descriptor_shapes` are empty), and its body holds no `data` that the source
did not write as `data`. To every backend it is indistinguishable from a
function written without type parameters, which is most of why this is
feasible: the engines need to *find* instances, not learn anything about them.

**Partial instances** -- some parameters bound, the rest left erased -- fall out
of the same machinery, because lowering already erases whatever `Var` is left.
They are not needed for anything below and are not planned for a first
version; [Open questions](#user-content-open-questions) says where they might
earn their place.

## Why lowering again rather than copying IR

The compiler already makes copies of functions: `const` specialization
(`compiler/src/specialize.rs`, phase 5c) copies a lowered unit per tuple of
constant arguments and points the `ComptimeCall`s at the copies. That is IR
copying, and it is the right model for *where copies live and how calls are
pointed at them*, but not for *what is in them*: by phase 5c a generic body is
already erased. Its values of `T` are `data` in `value_types`, its containers
are wrapped, its literals carry descriptor shapes, its borrows go through
`DataBorrow`, and its call sites have `Erase` and `Reify` in them. Undoing all
of that on IR is a second, worse lowering.

So an instance is made from the source: the typechecked body, lowered by the
ordinary `lower_function_body` with a substitution in force. The typechecker
does not run again. A generic body is checked once with `T` opaque, which is
strictly fewer capabilities than any type it admits -- `==` on an unbounded
`T`, literals against `T`, non-copy projection are all refused -- so a body that
checked generically is well typed under any binding its bounds allow.
Substituting into the stored expression types is sound.

## Lowering under a substitution

Erasure happens at one conversion: `IrType::from_datalit` turns `Var` into
`Data` (`ir/src/lib.rs`, the `Var` arm, and `datalit_is_container_of_var`).
Every generic special case in lowering keys off the result being `Data`, or off
a type hint naming a type parameter. Substitute before the conversion and they
do not fire:

| Special case | Keys off | Under a substitution |
|---|---|---|
| erased literal (`ListNew { descriptor: Some }`), `build_shape`, `built_shapes` | `shape_of` finding a `Var` | plain `ListNew`, no shape |
| `wrapped_shapes`, `container_shape`, `open_container` / `DataBorrow` | operand type `Data` | never `Data`, never fires |
| dyn arithmetic on a bounded `T` | backends seeing `Data` operands | ordinary `BinOp` on `f64`, which lowering always emitted |
| `drop_discarded_checked_result`, `checked_failure_message`, `lower_set_compound` | `Data` | concrete paths |

So the substitution goes in `LowerCtx`, as a binding map applied with
`substitute_type_params` (`datafun-common/src/generics.rs`, which already
exists and handles every type form) at each place a typechecker type enters
lowering:

- `expr_type` and `expr_source_type` (`lower/src/context.rs`);
- `shape_of`, `erased_element`, `container_shape`;
- the resolved parameter and return types the compiler hands
  `lower_function_for_module`, which today come from `from_tycheck` on the
  export signature and so are erased;
- each nested call's `type_args` (below).

The signature path in `lower/src/func.rs` reads the AST type hints with the
function's `type_params` and decides erasure from them
(`type_hint_mentions_param`, `erased_param_type`, `descriptor_params`, the
parameter `wrapped_shapes`). In an instance those branches are skipped
outright: parameter and return types are the substituted resolved types,
`descriptor_params` is empty, nothing is wrapped, and `ctx.body.type_params` is
empty, so a `Var` that escaped the substitution fails loudly in `shape_of`
rather than becoming a `data` nobody described.

One hint path needs fixing anyway: an uninitialized `var x: T` is typed from its
hint with `from_type_hint` (`lower/src/stmt.rs`), which knows no type
parameters and makes `IrType::Error`. It should take the typechecker's type.

**Nested calls compose.** Inside a generic the `type_args` recorded at a call
site are written over the caller's own `Var`s: `repeated` calls `push` with
`T := Var(T)`. In `repeated[bool]` each of them is substituted with the outer
binding first, giving `push` with `T := bool`. Every consumer of `type_args`
goes through the composition; one that forgets produces `shape_of(Var)` in a
body with no type parameters, which is a loud failure, not a wrong answer.

## Ownership analysis per instance

This is the subtle part. Ownership analysis sees `IrType`s, a `T` is a `Data`,
and `IrType::is_copy` says a `Data` is not copy -- so in the generic, every
binding of type `T` is tracked, scheduled for drops and checked for moves. In
`repeated[u32]` those bindings are `u32`s, which are copy, and copy-ness decides
about a dozen things in `datafun-ownership`: the tracking category, scope-exit
and branch drops, the before-return and try-return lists, loop drop
candidates, `mark_moved` and the D001/D002/D007/D008/D014 checks.

Reusing the generic's analysis is therefore wrong, not merely slow: it would
schedule drops of `u32`s and track bindings that cannot be moved, and lowering's
own copy checks would disagree with it. Running it again on the substituted
types is right and sound -- copy is more permissive, so a body that passed with
`T` non-copy passes with `T := u32`, and binding allocation does not depend on
types, so `BindingId` and `StmtKey` order are unchanged.

So an instance gets its own `FunctionAnalysis`, from `analyze_module`'s
per-function work with the expression types substituted before
`convert_expr_types`. Two paths there read AST type hints rather than
expression types (the fallback when no resolved types are given, and an
uninitialized `var x: T`) and need the substitution as well.

## Call sites

No erasure shell: a call into an instance is lowered as a concrete call, and an
instance is entered only that way. This is where the gain is -- an instance
calling an instance converts nothing -- and it is also what keeps the design
honest, since the boundary conversions are exactly the code the instance exists
to remove.

Today the conversion is the caller's: lowering a call to a generic
(`lower/src/expr.rs`, the call arm) reads the callee's source signature, decides
`param_is_erased` per parameter, emits `Erase` (or `EraseTracked`, moving a
non-copy binding out first) before each owned erased argument, gives the call
the erased return type, and emits `Reify` after it and after each erased `out`
parameter. Shape descriptors are resolved onto the `Call` later.

A call site that the policy monomorphizes does none of that. With its
`type_args` composed and all concrete, it:

- names the instance as its callee (see [Identity](#user-content-identity));
- takes its parameter and return types from the callee's signature under the
  substitution -- concrete, so every argument already has the right type and no
  `Erase`, `Reify` or out-parameter round trip is emitted;
- records no `type_args` and gets no `shape_descriptors`;
- passes borrowed arguments as they stand, with no descriptor, because the
  instance asked for none.

A call site the policy leaves erased is lowered exactly as today, whether its
caller is erased, concrete, or itself an instance. That is the property that
makes per-site choice possible: the two kinds of call are independent, and a
caller can hold both, to the same callee.

A call whose `type_args` still mention a `Var` after composition -- an erased
caller forwarding its own `T` -- can only be erased. The policy is not asked.

## The instance plan

Lowering a call site as monomorphized commits the program to an instance
existing. So which instances exist has to be known before callers are lowered,
and it has to be finite.

The plan is computed from the typechecker's output alone, without lowering
anything: `CallTargets` already records, for every call, its callee and its
type arguments. Starting from every call site whose type arguments are concrete
and that the policy monomorphizes, each new instance's body contributes its own
call sites, with their type arguments composed under its binding; the policy is
asked about each; the closure is the plan. Its output is

- the set of instances, each `(function, type arguments)`;
- per call site *per containing function or instance*, whether it is
  monomorphized -- a call site inside `repeated` is a different decision in
  `repeated[bool]` than in `repeated[string]`.

**It has to terminate.** A generic that calls itself at a strictly larger type
-- `f[T]` calling `f[[T]]` -- would instantiate forever. `close_shapes` already
decides exactly this for descriptor shapes, from the same call graph: an edge
from callee type parameter to caller type parameter, *strict* when the binding
wraps it in a type constructor, and a cycle holding a strict edge grows. The
plan reuses that answer: a call site on a growing cycle is erased, whatever the
policy says. A cap on the number of instances is the backstop, as
`MAX_INSTANTIATIONS` is for consts; a call site the cap refuses is erased too.
Both are decided in the plan, before any lowering, so lowering never discovers
an instance it was not promised.

## Policy

```rust
enum GenericsPolicy {
    /// Every call site erased: today's behaviour.
    Erased,
    /// Every call site that can be monomorphized is.
    Monomorphized,
    /// A seeded coin per call site, the same seed giving the same choices.
    Chaos { seed: u64 },
    /// What the compiler thinks pays.
    Default,
}
```

A policy answers one question: monomorphize this call site, of this callee at
these type arguments, in this function or instance? Per-function choice is the
special case of answering the same for every site that reaches a callee.
`Chaos` hashes the seed with the site and the containing function, so its
choices are stable across a run and reproducible from the seed, and two call
sites of one callee in one caller can differ -- which is the case most likely
to expose a boundary bug.

`Default` is a heuristic and starts conservative: monomorphize where the
compiled backends are building the whole program (AOT, C) and the instance is
small, erase in the interpreter and jit until [run time asks](#user-content-asking-at-run-time).
It is the one policy whose choices are about speed; the others exist to test.

The policy is part of the compilation's input (an `AnalysisOptions` field and a
salsa input, as `skip_specialization` is), so changing it invalidates the plan
and nothing else.

## Identity

An instance needs a name every layer can use: a `CodeRef` in the IR, a
`FuncIdentity` in the interpreter and jit, a symbol in each backend.

Const copies take fresh `FuncId`s after the source functions'. That works
because phase 5c runs after lowering, when the ids are settled. Instances are
named *during* lowering -- a caller's call site names its instance before the
instance is lowered, possibly in another module -- so the name has to come from
the key, not from an allocation. The plan is the natural numbering: it is
computed before lowering and is deterministic, so it can give each instance an
index within its module.

- **IR:** a new `CodeRef::Instance { module, id: InstanceId }`, where
  `InstanceId` indexes the module's instances in the plan. A distinct variant
  rather than ids after the source functions', so nothing that matches on
  `CodeRef::Module` mistakes an instance for a source function, and the
  registry's lookup can say which table to read.
- **Registry:** `ModuleFunctionRegistry` gains a per-module instance table
  beside its functions. It stays immutable after module compilation, as now.
- **Interpreter and jit:** `FuncIdentity::Instance { module, id }`. The jit's
  `EncodedFuncKey` has two unused kinds; one of them is this. The layout cache,
  code cells, stubs and stats key on `FuncIdentity` already.
- **Symbols:** `__mod_{m}_{name}__i{id}` in the Cranelift AOT and C backends,
  and the jit's `unique_symbol` extended the same way. A name is not enough on
  its own -- `push` has an instance per element type -- and the type arguments
  are too long to mangle, so the index carries the identity; the debug name can
  spell the types out (`push[bool]`).

## Salsa

Lowering is per module today (`lower_module_functions`, keyed on the module and
its typecheck and analysis), with no per-function query. Instances need finer
grain: a new instance should lower one function, not its module.

- `instance_plan(graph, policy)` -- the closure above, over the graph's
  typecheck `CallTargets`. Its output is per module, so each module's lowering
  depends only on its own part.
- `analyze_instance(module, function, type_args)` -- ownership analysis under
  the substitution.
- `lower_instance(module, function, type_args)` -- one `IrCodeUnit`, in the
  callee's module, with that module's `ExprTypes` and `CallTargets`. An instance
  belongs to the callee's module because only that module's tables can lower
  the body; the caller only names it.

`type_args` is interned, so the key is cheap and two call sites asking for the
same instance share it. Which instances a module has depends on its *importers*
-- `list.push[bool]` exists because Sieve calls `repeated` with a `bool` -- so
an importer adding a call site at a new type adds one `lower_instance`, not a
relowering of `sys/std/list`.

Shape closure (`close_shapes_over_calls`) runs over erased functions only.
An instance has no shapes, and its erased call sites have concrete type
arguments, so their descriptors are all `Static` and `resolve_call_descriptors`
settles them on the instance alone.

Const specialization runs after, unchanged: an instance's `ComptimeCall`s are
copied like anyone else's, and its copies are instances' copies.

## Scripts and the REPL

A script unit cannot add to a module: the module registry is immutable once
built, and a REPL line arriving later is exactly the case erasure was chosen
for. Phase 5c already has the answer for consts: `specialize_script_unit` copies
the module function into the script unit's own `nested_units`, reached by
`CodeRef::Local`, so a new instantiation in line 12 compiles one function into
line 12 and touches nothing shared.

Instances demanded by a script unit do the same: lowered with the callee
module's tables, placed in the unit's `nested_units`. The cost is duplication --
two lines using `push[bool]` lower it twice and tier it twice -- which is what
const copies cost today too. A session-wide instance table, append-only like
`UnitFunctionRegistry`, would remove it, and is the same structure
[run-time requests](#user-content-asking-at-run-time) need; it can come with
them.

Erasure stays what makes the REPL cheap. A new type at a REPL line compiles at
most the instances the policy asks for, and with an erased policy, nothing.

## What the backends need

Nothing about instances themselves: an instance is a concrete function. What
each needs is to *find* one.

- **Interpreter:** `ExecutionContext::get_unit` resolves the new `CodeRef`
  through the registry's instance table; `resolve_identity` maps the
  `FuncIdentity` back. The bytecode's planned-call caches validate a callee by
  registry pointer and code epoch, which covers instance callees as they are.
- **Cranelift (jit and AOT):** declare instance units with the module's other
  functions (`cranelift-aot` declares from the registry; the jit's
  `compile_entry` gathers callees by `CodeRef`). Instance call sites carry no
  descriptors, so the signature code is untouched.
- **C:** the same declarations, `emit_module_declarations`.

Two facts found on the way, independent of this but in its path:
`collect_call_targets` in the jit (`cranelift-jit/src/compiler.rs`) collects
`Call` but not `ComptimeCall`, so a function compiled with a surviving
`ComptimeCall` panics on an undeclared callee (reachable under
`skip_specialization`); and the typechecker's `is_copy_type` calls a `Result`
of a copy type copy where `IrType::is_copy` never does.

## Testing

The two translations are each other's oracle. Any program means the same thing
erased and monomorphized, at every call site independently, so every fixture
can run under each policy and be compared against the reference, the way
`noconst` and `nospec` compare against it today.

New engine-test variants (`Differential` in `datafun/tests/engine_tests.rs`):

| Variant | Policy | Engine |
|---|---|---|
| `mono` | `Monomorphized` | bytecode |
| `monochaos` | `Chaos { seed }`, the fixture's seed | bytecode |
| `monojit` | `Chaos { seed }` | jit through the chaos dispatcher |

and the AOT and C runs gain a monomorphized build, since that is where
`Default` will monomorphize first. The leak check runs under all of them: a
drop schedule computed for the wrong copy-ness leaks or double-frees, which is
the failure this design is most exposed to, and the leak check is what sees it.

The erased implementation has had several silent faults, two of them identical
in all four backends, which comparing backends cannot catch. Comparing
translations can: a monomorphized `[(string, u32)]` computes its strides from
its type, with no descriptor in sight, so an erased path that gets one wrong
disagrees with it. The new variants are a test of erasure as much as of
monomorphization.

IR text in fixtures stays the erased lowering, under the default policy; the
variants compare behaviour.

## Asking at run time

The end state: the interpreter or jit notices a generic call is hot and asks
for an instance. With no erasure shell, what it asks for has to be a call site
that is monomorphized -- and a call site is part of its caller's IR. So the
request is not "an instance of `push` at `bool`" but "the caller, lowered again
with this call site monomorphized".

When the hot call is in a concrete function, its type arguments are static, the
plan could have monomorphized it at compile time, and asking amounts to the
policy changing its answer for that site: lower the caller again under the new
answer, lower the instance it now names, and install the new caller. When the
hot call is inside an erased generic, its type arguments are known only from
the descriptors that arrived, and the request moves up a level: monomorphize
the generic *itself* at those arguments, from its own caller's site, and so on
up to a concrete caller -- which is how Julia specializes, on the types a call
is made with, from where they are known.

What that needs, none of which exists yet:

- **A compiler at run time.** The interpreter today runs on registries. A
  `Specializer` installed the way a `CallDispatcher` is, holding the database,
  that turns a request into new units. The CLI and the REPL both have the
  database; an AOT executable does not and never asks.
- **Types from descriptors.** A request from an erased frame knows its type
  arguments as `TyDesc`s, and an instance is keyed on typechecker types. The
  tydesc table is built from `IrType`s; whether every concrete `IrType` maps
  back to one typechecker type (struct and enum names, aliases) needs checking.
- **Replacing a running function.** A new caller has to replace the old one
  for future calls -- the jit's code cells and the bytecode's per-site caches,
  validated by code epoch, are the mechanism -- while frames already running the
  old one finish in it. OSR into the new version is a later refinement.
- **An append-only instance table** shared by the session, which the REPL
  wants anyway.

This is the last phase and the largest, and every phase before it stands on
its own.

## Phases

Each phase leaves the default behaviour unchanged and the suite passing.

1. **Substitution plumbing.** The binding map in `LowerCtx`, applied at every
   entry point in [Lowering under a substitution](#user-content-lowering-under-a-substitution),
   with an empty map everywhere. The `var x: T` hint fix. Ownership analysis
   taking substituted types. No behaviour change: the empty map is today.
2. **Instances, lowered and named.** `lower_instance` and `analyze_instance`;
   `CodeRef::Instance`, the registry's instance table, `FuncIdentity::Instance`,
   symbols in the three code generators. A test hook that lowers an instance
   by hand and calls it, to check an instance alone before any call site
   reaches one.
3. **Call sites and the plan.** `instance_plan` with the growth check and cap;
   concrete call-site lowering; the `Monomorphized` policy; the `mono` variant.
   Done when every fixture passes under it, leak check included.
4. **Chaos.** `Chaos { seed }`, `monochaos` and `monojit`, and the AOT and C
   monomorphized builds.
5. **Scripts and the REPL.** Instances in script units' `nested_units`.
6. **`Default`.** The heuristic, starting with AOT and C, measured on benchvs
   and the store demo; code size and compile time recorded alongside speed.
7. **Run-time requests.** The `Specializer`, types from descriptors, replacing
   callers, the session instance table.

Phase 3 is the one that proves the design: `repeated[bool]` calling
`push[bool]` with nothing converted.

## Open questions

- **Partial instances.** Binding some type parameters and erasing the rest
  falls out of the substitution, and a call site in an erased caller with one
  concrete argument could use one. Worth it only if a measurement shows such
  sites hot.
- **Inlining.** An instance of `push` is a one-line function around a native
  call. Inlining it into `repeated[bool]` removes a layer the plan cannot.
  There is no function inliner today; instances make one more valuable, since
  an instance is concrete enough to inline into anything.
- **Code size and compile time.** Unbounded under `Monomorphized` except by the
  cap. `Default` needs a measure of both before it monomorphizes much.
- **Descriptors back to types**, for run-time requests: see above.
- **Instances of natives.** A generic native (`std.list_push`) is not lowered,
  so it has no instance; an instance calls it with concrete descriptors, which
  is already cheaper than erased, since `list_push_erased` finds nothing to
  unwrap. A typed fast path in the runtime for a known element type is a
  separate question.
- **Whether `descriptor_params` survives.** If enough of the program runs as
  instances, the erased path's borrowed-descriptor machinery serves less of it.
  It stays as long as erasure does, which is as long as the REPL wants it.
