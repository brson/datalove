# Specializing Generics

Written October 2026, after generics by reference landed. Implemented as a
proof: instances, and a compile-time pass that points call sites at them one
at a time, tested on the IR walker and offered to nothing else. A run-time
version in the JIT was built, measured and taken out; see [What a run-time
experiment found](#user-content-what-a-run-time-experiment-found).
This replaces an earlier plan written under erasure, which had to lower a
generic again from source to get a concrete copy. Under the current design it
does not.

A generic function is compiled once and run on descriptors ([Generics: how it
works](generics.md)). Every `T` is read through a pointer, every move copies
`desc.size` bytes, every call fills a descriptor table or compares against its
one-entry cache, and every bounded operation is a call into the runtime. That
is the right default for a language with a REPL, but it is what a hot generic
pays on every call, and it is what keeps the AOT backends from producing the
code a concrete function gets. The answer is a pass that makes an **instance**
-- the generic's IR with its type parameters substituted -- as an optional,
additive optimization, chosen per call site.

The pass is small because the IR already keeps type parameters as types.
`IrType::Param(i)` sits inside a function's value, slot and parameter types
and inside each call's `bindings`. No instruction says it is generic; a
backend decides from the operand types. Substitute the parameters and what is
left is a concrete function, which every backend already compiles.
`ir::inline` already does this substitution when it splices a generic callee
into a caller.

## Contents

- [Why the old plan no longer applies](#user-content-why-the-old-plan-no-longer-applies)
- [An instance is a substitution](#user-content-an-instance-is-a-substitution)
- [Why substitution is sound](#user-content-why-substitution-is-sound)
- [Additive: every call site is optional](#user-content-additive-every-call-site-is-optional)
- [Ahead of time](#user-content-ahead-of-time)
- [Cleaning up after substitution](#user-content-cleaning-up-after-substitution)
- [What exists](#user-content-what-exists)
- [Testing](#user-content-testing)
- [What a run-time experiment found](#user-content-what-a-run-time-experiment-found)
- [Open questions](#user-content-open-questions)

## Why the old plan no longer applies

The erased design converted at the boundary. A `T` was a `data` in the IR,
containers of `T` were wrapped, and calls had `Erase` and `Reify` around them.
By the time a function was lowered its generic-ness was spread through the
instructions, so the earlier plan argued for lowering again under a
substitution, running ownership analysis again per instance, and adding
per-instance salsa queries. Because a call site lowered as monomorphized had
no erasure shell, the set of instances also had to be planned before lowering.

Generics by reference removed every one of those reasons:

- The IR of a generic is the IR it would have if `T` were a concrete type.
  Only the types differ: `Param(0)` where `u32` would be.
- A dynamic value's calling convention is a pointer, and so is a concrete
  aggregate's. There is no conversion to remove at a boundary, so an instance
  and the generic can be mixed freely at any call site.
- `descriptor_table` is computed from the unit (`ir::descriptor_table`), so a
  rewritten unit has its table worked out again, and for an instance it is
  empty.

So a specialization pass works on IR, beside const-parameter specialization
(`crates/datalove-datafun-compiler/src/specialize.rs`), not inside lowering.

## An instance is a substitution

```rust
/// `generic` with every type parameter bound: a concrete function.
pub fn instantiate(generic: &IrCodeUnit, bindings: &[IrType]) -> IrCodeUnit
```

in `crates/datalove-datafun-ir`, beside `inline`. It clones the unit and
applies `IrType::substitute(bindings)` to:

- `FunctionContext::param_types` and `return_type`;
- `value_types` and `slot_types`;
- each `Call` and `ComptimeCall`'s `bindings`, which are written over this
  function's parameters, so `repeated[bool]`'s call to `push` with
  `T := Param(0)` becomes `push` with `T := bool`.

Then it sets `type_param_count` to 0 and recomputes `descriptor_table`, which
comes out empty. It asserts that no `Param` survives anywhere. Every
`bindings` is concrete, so every call the instance makes has static
descriptors. The name gets the bindings for debugging: `push[bool]`.

These are exactly the places `inline::splice` substitutes, and the reason is
the same: instructions do not carry `IrType`. Those that name a type carry a
`TypeRef`, which is a shape (`Tuple(n)`, `Option`, `List`), not a type over
parameters. If an instruction later gains an `IrType` field, both passes need
it. That is worth a test that substitutes into a unit holding every
instruction kind.

`substitute` panics on a missing argument, so partial instances (some
parameters bound, the rest renumbered) need a variant of it. They are left to
[Open questions](#user-content-open-questions).

## Why substitution is sound

A generic body was typechecked with `T` opaque, which allows strictly less
than any type the bounds admit. The concern is the decisions lowering made
on the assumption that a `T` is not a copy type, now that it may be one:

- **Instruction choice.** Lowering picks `Move` over `Copy`, `SlotStoreMove`
  over `SlotStoreCopy`, and schedules `Drop`s and tracked slots for each `T`
  binding. Each of these is still correct for a copy type: a move of a `u32`
  copies it, and the backends treat a drop of a copy type as nothing
  (`compile_drop` returns early on `is_copy`). The IR is redundant, not
  wrong. [Cleaning up](#user-content-cleaning-up-after-substitution) removes
  the redundancy.
- **Arithmetic.** A bounded operation on `T` is an ordinary `BinOp` or
  `BinOpChecked`, and lowering picks between them from the operator, not the
  type. Under substitution it is the instruction a concrete function would
  have, and the backend compiles it natively in place of `dyn_ops`.
- **Literals and projections.** `ListNew`, `Pack` and `GetField` take their
  descriptor or offset from the operand type. A static type gets a static
  descriptor and a static offset.
- **`in` arguments.** For a dynamic argument, the caller decides at run time
  from `TyDesc::PLAIN` whether the argument was moved. For a static one, the
  backends decide from the static type. A substituted argument has a static
  type, so the static path applies, and it gives the same answer.

Unlike the old plan, ownership analysis does not run again. It ran on the
generic, and its output is IR that stays correct under any binding.

The interpreter needs nothing new. An instance is a function whose types
mention no parameter, and that is all the walker or the bytecode looks at.

## Additive: every call site is optional

As with const specialization, the generic stays. An instance is an extra
function, and a call to the generic at bindings `B` means the same as a call
to the instance at `B`. Therefore:

- Any call site can be pointed at an instance or left alone, one site at a
  time and in any combination. A caller can hold both kinds of call to one
  callee.
- Refusing is always safe. A site on a growing cycle or over the instance
  cap stays a call to the generic.
- Nothing has to be planned before lowering. Instances can be made after the
  fact, by the module graph, by a script unit, or by anything else holding a
  generic's IR and a binding.

The policy says, for each call site, yes or no:

```rust
pub enum GenericsPolicy {
    /// None: every generic runs on descriptors.
    Off,
    /// Every call whose bindings are concrete.
    All,
    /// A coin per call site, seeded: the same seed makes the same choices.
    Chaos { seed: u64 },
}
```

## Ahead of time

The compile-time pass is phase 5c's twin and runs after it. It is
graph-wide, since it creates functions, and runs only when
`CompilerOptions::generics` is not `Off`.

1. **Seed.** Collect each `Call`, in any function, whose callee is generic
   (`type_param_count > 0`, not `inline`) and whose bindings are concrete,
   and ask the policy about each.
2. **Instantiate.** For each new `(callee, bindings)`, `instantiate` the
   callee into its own module with a fresh `CodeUnitId` after the module's
   others, as const copies get. Repoint the call.
3. **Close.** An instance's own calls to generics now have concrete bindings.
   Treat them as in step 1, and repeat until nothing new appears.

**Termination.** `f<T>` calling `f<[T]>` would instantiate forever. Track the
chain of instances that led to each request. A request for a callee already
on its own chain, at a binding that strictly contains the earlier one, is
refused, and so the site stays generic. Generics by reference handles that
call at run time by building one more descriptor, as it does now. A
per-function cap (`MAX_INSTANTIATIONS`, as for consts) and a total cap are
the backstop.

**Interactions.**

- *Const specialization.* A const copy keeps its type parameters
  ([IR: passes](ir.md#user-content-13-passes)), so it can then be
  instantiated like any generic. Running 5c first means instances never see
  a `ComptimeCall` that 5c could have resolved.
- *Inlining.* Inline functions are never instantiated. Inlining runs after
  this pass, so a call that inlining exposes -- an inline generic's body
  calling a non-inline generic, now at the caller's concrete binding -- stays
  a call to the generic.
- *Natives.* A generic native has no IR to instantiate. An instance calls it
  with static descriptors, which is already the cheap path.

**Names.** An instance is a function with an id and a name,
`{generic}__g{id}`, as a const copy is `{function}__ct{id}`; the bindings are
not spelled out.

**Scripts.** A script unit runs the same pass over its own calls and its
functions', putting instances of module functions into its `nested_units`,
the way `specialize_script_unit` does for consts.

## Cleaning up after substitution

An instance carries what the generic needed for a non-copy `T`: tracked slots
and drop flags for bindings that are now `u32`, `Drop`s that do nothing,
`Clone`s that go through `dtlv_rti_clone_local` to copy a word. All of it is
correct, and `ir::instantiate::clean_copy_types`, run on every instance,
takes it out:

- a drop of a value, slot or parameter of a copy type goes;
- a `Clone` or `Move` of a copy type becomes a `Copy`;
- a tracked slot of a copy type is untracked: its tracked stores and loads
  become copies and its tracked drops go.

Inlining had the same leftovers, and there they were a bug. `inline::splice`
substituted a call's binding into a generic callee's types and kept the
body's drops of each `T`; once `simplify` had replaced the copy of a caller's
value that an `in` parameter was bound to by the value itself, that was a
drop of the caller's own, and the interpreter's liveness checks in a debug
build took the value to be gone (`184_inline_generic_reads_after`: a function
passing its own `f64`s to `float.lerp` and reading them after). The splice
now binds the callee with `substitute_types` and cleans it with the same
pass first.

## What exists

- `crates/datalove-datafun-ir/src/instantiate.rs`: `instantiate`,
  `substitute_types`, `clean_copy_types`, with unit tests on hand-built
  units.
- `crates/datalove-datafun-compiler/src/instances.rs`: the pass, the
  `Instantiator` that both pipelines drive, and `GenericsPolicy` -- `Off`,
  `All`, and `Chaos { seed }`, which hashes the seed with the call site
  (function name, block, instruction), the callee and the bindings. The
  policy is asked per site before an existing instance is reused, so two
  sites calling one instance can go different ways. Unit tests cover chains,
  recursion at one binding, a growing type, and chaos splitting sites.
- The module graph's phase 5c'' (`tracked_lower.rs`) and the script
  compiler's phase 5' (`ScriptCompiler::set_generics_policy`), with the
  policy on `CompilerOptions` and `AnalysisOptions`.

The policy is `Off` by default and nothing outside the tests turns it on.
Instances are ordinary functions, so every backend could run them; only the
IR walker is held to it so far.

## Testing

The generic and its instances are each other's oracle: any program means the
same at every policy and at every call site independently. So, on the IR
walker:

| Where | Policy |
|---|---|
| `engine_tests`, `spec` | `All` |
| `engine_tests`, `specchaos` | `Chaos { seed }`, the fixture's seed |
| `std_engine_tests`, `spec` | `All`, the standard library compiled a second time under it |
| `std_engine_tests`, `specchaos` | `Chaos { seed }` from the script, for the script's calls |

each compared with the unspecialized reference, with the leak check on. The
standard library's suite is where most of the coverage is: `sys/std` makes
no call to a generic at a concrete binding inside its own modules -- its
generics forward their `T` -- so the module graph's pass makes nothing
there, and the chains form in script units, which instantiate `push`, `of`,
`unwrap_or` and the like. The engine corpus has few generics.

This tests generics by reference too. Most faults there have been silent,
some identical in all four backends ([Where the tests
are](generics.md#user-content-where-the-tests-are)), so comparing backends
did not catch them. An instance computes strides and offsets from static
types without any descriptor, so a descriptor-path fault shows up as a
disagreement.

## What a run-time experiment found

In October 2026 instances were also made at run time, in the JIT, and the
compile-time pass was offered to the AOT backends, then measured on benchvs
and the store demo; commits `59d5a680` to `47f6f611`. All of that was taken
out again, to keep specialization out of the backends until it is better
understood. What it showed:

**Ahead of time it pays.** `aot-compile` with every call at a concrete
binding pointed at an instance: sieve, the case
[AWFY](research/research-awfy-benchmarks.md) points at, 28% faster;
`ordered` and the store demo 5-6%; the other benchvs programs within noise;
executables at most 35 KB larger of 5-10 MB; compile time unchanged.

**In the JIT the mechanics work and the policy is hard.** The JIT read a
call's binding back out of its descriptors, instantiated, and compiled the
instance; compiled callers called instances directly and planned bytecode
sites entered them. Correct on every fixture, and 28% faster on sieve. But
deciding *when* to specialize went wrong in ways that cost more than
specializing saves:

- Counting each instance's calls on its own meant a generic called at four
  bindings warmed up four times as long in the interpreter: `ordered` was
  26% slower than not specializing.
- Compiling an instance as soon as its generic was hot compiled instances
  that were not hot.
- A tier between -- the generic compiled over descriptors once hot, an
  instance compiled once hot itself -- got `ordered` to 2%, at the cost of
  a second compile, a new site policy, and counting machinery in the
  dispatcher, the bytecode and the trampoline.
- And it still threw hotness away along call chains. Counting happens only
  while a callee has no compiled code, so once a generic is compiled, calls
  from generic code passing its own `T` count toward nothing; a chain
  `A<T> -> B<T>` specializes one level per warm-up, each starting from zero
  though the caller already shows the callee is hot at that binding.

So per-function counters are the wrong shape for specialization, which is a
property of call chains: hotness has to flow down from a caller hot at
concrete types to what it calls at them. That is the question to answer
before trying the JIT again.

## Open questions

- **How the JIT should decide.** See above. Two directions: an instance
  reached from compiled code at concrete types compiles on its first call,
  taking its caller's hotness as its own; or compiling a hot function at
  concrete types compiles the closure of instances beneath it, as the
  compile-time pass does.
- **Exposing it to the backends.** Ahead of time it is a clear win and needs
  nothing of the backends, which already compile instances as ordinary
  functions; what it needs is a decision to turn it on, and tests on each.
- **Inlining order.** Inlining runs after this pass, in `optimize::finish`,
  so a call that inlining exposes stays a call to the generic. Many of the
  standard library's generics are `inline`.
- **Script bodies are not inlined.** A script unit's own body is not
  finished by `optimize::finish` -- only its `nested_units` are -- so its
  calls to `inline` functions are real calls.
- **Partial instances.** Binding some type parameters and renumbering the
  rest is a small change to `substitute`. A generic caller forwarding its `T`
  with a second parameter concrete could use one.

The idea holds: a generic and its instance can stand on either side of any
call site, chosen one site at a time, and the programs mean the same. What is
open is deciding when an instance is worth its compile, which ahead of time
is easy and at run time is not.
