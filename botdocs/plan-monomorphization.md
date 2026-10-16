# Specializing Generics

Written October 2026, after generics by reference landed. Phases 1 to 6
are implemented, and phase 7 measured: `ir::instantiate`, and the ahead-of-time pass under the `All`
policy (`crates/datalove-datafun-compiler/src/instances.rs`).
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
additive optimization. The same pass serves three callers: the AOT pipeline
specializing what it can see, the JIT specializing a function at the bindings
it is actually called with, and tests choosing at random.

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
- [At run time](#user-content-at-run-time)
- [Cleaning up after substitution](#user-content-cleaning-up-after-substitution)
- [Testing](#user-content-testing)
- [Phases](#user-content-phases)
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
- Refusing is always safe. A site on a growing cycle, a site over the
  instance cap, or a binding the run time cannot turn back into types stays a
  call to the generic.
- Nothing has to be planned before lowering. Instances can be made after the
  fact, by the AOT pipeline, by a script unit, or by the JIT halfway through a
  run.

The policy is the same everywhere: a function from
`(callee, bindings, caller)` to yes or no.

```rust
enum SpecializePolicy {
    /// No instances: today's behaviour.
    Off,
    /// Every call site whose bindings are concrete.
    All,
    /// A seeded coin per call site, stable for a seed.
    Chaos { seed: u64 },
    /// What pays: instances of small or hot generics.
    Default,
}
```

## Ahead of time

The compile-time pass is phase 5c's twin and runs after it. It is
graph-wide, since it creates functions, and is gated by an `AnalysisOptions`
field that is also a salsa input, as `skip_specialization` is.

1. **Seed.** Collect each `Call` in a non-generic function whose callee is
   generic (`type_param_count > 0`, not `inline`) and whose bindings are
   concrete, and ask the policy about each.
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
- *Inlining.* Inline functions are never called, so they are never
  instantiated. Inlining runs after this pass, in `optimize::finish`, so a
  call that inlining exposes -- an inline generic's body calling a
  non-inline generic, now at the caller's concrete binding -- stays a call to
  the generic. Inlining is per function and cannot make functions, so
  catching those needs this pass to run again after it, or inlining to run
  before it.
- *Natives.* A generic native has no IR to instantiate. An instance calls it
  with static descriptors, which is already the cheap path.

**Backends.** Cranelift AOT and C need nothing beyond what const copies
already need: an instance is a module function with an id and a name. The
symbol is the module's usual mangling of the id. The debug name spells out
the bindings.

**Scripts and the REPL.** A script unit runs the same pass over its own
functions, putting instances of module functions into its `nested_units`,
the way `specialize_script_unit` does for consts. The policy for the REPL
starts as `Off`: generics by reference exist so that a new binding in a REPL
line costs no compilation.

## At run time

This is the per-function, on-demand case. The interpreter and the JIT already
see, at every call, the callee's `IrCodeUnit` and the descriptors it is being
called with (`DispatchCallContext::type_params`). A generic that is hot at
some binding can be instantiated at that binding then. No caller has to
change, because the calling conventions line up.

**Descriptors back to types.** A request arrives as `TyDesc`s, and
`instantiate` wants `IrType`s. `IrType` is structural (struct fields by name,
enums by variant), and a descriptor carries the whole type, so the mapping
exists in principle. It already exists in practice for most tags as
`ir_type_from_tydesc` in `crates/datalove-datafun-interp/src/ctfe.rs`. It
moves somewhere shared and is completed for enums, terms, atoms and cursors.
A descriptor it cannot map is a refusal, and the call stays generic.

**Identity.** The registries are immutable and stay so. Run-time instances
live in a session table owned by the dispatcher: append-only and keyed by
`(FuncIdentity, interned bindings)`. They get
`FuncIdentity::Instance { base, binding }`, which takes one of the two unused
kinds in the JIT's `EncodedFuncKey`. Code cells, stubs, layout caches and
stats already key on `FuncIdentity`. When the JIT retires a function's cells
because its code changed, it retires its instances' cells too.

**Callee side: entering an instance.** When the dispatcher sees a call to a
generic, it counts per `(function, binding)` rather than per function. A hot
pair is instantiated and compiled, and later calls at that binding enter the
instance. The entry convention is the same: a compiled entry takes a pointer
per argument and an `sret` pointer, then descriptors for a generic. An
instance takes the same words minus the descriptors. A planned interpreter
call site whose bindings are all `BindingRef::Static` always calls at the
same binding, so its plan can `Enter` the instance directly. A site whose
bindings come from a table varies, and keeps asking.

This also fixes a known cost: the descriptor-table cache holds one binding
([issues](issues.md)). A generic called alternately at two hot bindings gets
two instances and fills no table.

**Caller side: compiling call sites to instances.** When the JIT compiles a
function, whether concrete or itself an instance, a call with concrete
bindings can target the instance's code cell in place of the generic's. It
is compiled against the instance's signature, which is concrete. This makes
chains specialize: once `repeated[bool]` is compiled, its call to `push`
goes to `push[bool]`, and neither one touches a descriptor.

**Fallback.** An instance's cell starts at its stub, like any other. If the
instance has not been compiled, or the JIT refuses it, the dispatcher runs
the *generic* in the interpreter with the binding's static descriptors. These
are equivalent by construction. So the interpreter never needs to resolve an
`Instance` identity, and an instance costs nothing until it is worth
compiling.

**AOT executables** have no IR and never ask. They get what the
compile-time pass gave them.

## Cleaning up after substitution

An instance carries what the generic needed for a non-copy `T`: tracked slots
and drop flags for bindings that are now `u32`, `Drop`s that do nothing,
`Clone`s that go through `dtlv_rti_clone_local` to copy a word. All of it is
correct. A short pass after `instantiate` removes it:

- a `Drop` or `DropTracked` of a copy type is deleted;
- a `Clone` of a copy type becomes `Copy`;
- `Move` and the `*Move*` slot forms of a copy type become their copying
  forms;
- a tracked slot of a copy type is untracked, along with its tracked stores
  and loads.

Then `simplify` and dead code elimination run as they do after inlining.
None of this is needed for correctness, so it can land after everything else
and be measured on its own.

## Testing

The generic and its instances are each other's oracle: any program means the
same at every policy and at every call site independently. Therefore:

| Variant | Policy | Engine |
|---|---|---|
| `spec` | `All` | bytecode |
| `specchaos` | `Chaos { seed }`, the fixture's seed | bytecode |
| `specjit` | `Chaos { seed }`, run-time requests on | jit through the chaos dispatcher |

as `Differential` variants in `crates/datalove-datafun/tests/engine_tests.rs`,
beside `noconst` and `nospec`, plus an `All` build in the AOT and C backend
fixtures. The leak check runs under all of them. The chaos dispatcher also
gets a coin for "instantiate this binding now" at run time, so the callee-side
path runs on every fixture, not only on hot loops.

This also tests generics by reference. Most faults there have been silent,
some identical in all four backends ([Where the tests
are](generics.md#user-content-where-the-tests-are)), so comparing backends
did not catch them. An instance computes strides and offsets from static
types without any descriptor, so a descriptor-path fault shows up as a
disagreement.

IR text in fixtures stays at the default policy.

## Phases

Each phase leaves default behaviour unchanged and the suite passing.

1. **`instantiate`.** Done: `crates/datalove-datafun-ir/src/instantiate.rs`,
   with unit tests on a hand-built generic. The test hook became phase 2's
   variants, which run instances on every engine.
2. **The AOT pass under `All`.** Done: `instances.rs`, the module graph's
   phase 5c'' and the script compiler's phase 5'. The variants are `spec` in
   `engine_tests` (bytecode) and in `std_engine_tests`, where the standard
   library is compiled a second time under `All` and every backend --
   bytecode, jit, OSR, Cranelift AOT, C -- is held to the unspecialized
   reference. Instance names are `{generic}__g{id}`; the bindings are not
   spelled out yet.
3. **Chaos.** Done: `Chaos { seed }` hashes the seed with the call site
   (function name, block, instruction), the callee and the bindings. The
   policy is asked per site before an existing instance is reused, so two
   sites calling one instance can go different ways. `specchaos` in
   `engine_tests` seeds from the fixture; in `std_engine_tests` it seeds from
   the script and tosses for the script's calls only, since the library is
   compiled once per worker and has no concrete call to toss for.
4. **Run time, callee side.** Done: `JitEngine::set_specialize` and
   `crates/datalove-datafun-cranelift-jit/src/instances.rs`. A call to a
   generic, offered by the interpreter (`dispatch_with`) or arriving at the
   trampoline from compiled code, is counted, compiled and entered as the
   instance at its binding, `FuncIdentity::Instance { id }`, numbered by the
   jit. The binding is read back with `interp::ir_type_of_tydesc` and checked
   by building its descriptor again and comparing (`rt::c::same_tydesc`); a
   binding that does not come back exactly, a generic at its cap, or an
   instance the backend refused, leaves the call to the generic, which the
   interpreter runs with its descriptors throughout. Instances go when their
   generic's body is replaced. `specjit` runs the engine corpus under the
   chaos dispatcher specializing, and `std_engine_tests` runs the jit and the
   bytecode jit specializing; the standard library's suite compiles about six
   hundred instances, `push`, `of`, `len` and `insert` the most.

   What it cost until phase 5: a generic is never compiled itself while its
   instances can be, so its code cell stays empty and every call to it from
   compiled code went through its stub and the trampoline to find the
   instance; and a planned interpreter call site to a generic offered every
   call to the dispatcher (`SitePolicy::EveryCall`) rather than entering
   code, since only an offered call carries its binding.

   Found on the way: a script unit's own body is not finished by
   `optimize::finish` -- only its `nested_units` are -- so its calls to
   `inline` functions are real calls, and `inline` functions are called at
   run time. An instance of one is made like any other, and is not
   `inline`.
5. **Run time, caller side.** Done. When the jit compiles a function, each
   call it makes to a generic at a concrete binding calls the instance at
   that binding (`JitEngine::instance_callees`, the codegen's
   `set_instance_funcs`): a stub and code cell of the instance's own, its
   signature, no descriptors. The instance's stub reaches the trampoline with
   the instance's identity, which counts and enters the instance, or runs
   the generic in the interpreter with descriptors built from the binding.
   And a planned interpreter call site whose binding is the same at every
   call -- every `BindingRef::Static` -- is told by `site_policy`, which now
   takes the site's descriptors, to count toward its binding's instance and,
   once that is compiled, to enter it (`SitePolicy::EnterInstance`).
   `183_generics_called_in_loops` and a "Specializing Bytecode JIT at 50" run
   in `std_engine_tests` are what reach the planned sites: a function called
   once stays interpreted while what it calls in a loop is compiled.

   What was left of phase 4's costs, a site whose binding is its caller's
   type parameters offering every call, went with phase 7's middle tier.
6. **Cleanup.** Done: `ir::instantiate::clean_copy_types`, run on every
   instance, ahead of time and in the jit. Drops of copy types go, a `Clone`
   or `Move` of one becomes a `Copy`, and a tracked slot of one is untracked.
   Measured alone on a loop calling `dup<T>(x) = (x@, x)` and a `keep<T>` with
   a `var` of `T` twenty million times at `u64`, under the jit
   (`datalove script --jit --jit-specialize`, which this phase added):

   | | running |
   |---|---:|
   | `--jit` | 735-1000 ms |
   | `--jit --jit-specialize`, no cleanup | 240-365 ms |
   | `--jit --jit-specialize` | 164-167 ms |

   Most of the cleanup's share is the clone, which in an instance is a call
   into the runtime to copy a `u64`.

   Found on the way, and fixed: inlining a generic had the same leftovers.
   `inline::splice` substituted the binding into the callee's types and kept
   its drops of each `T`; after `simplify` replaced the copy of a caller's
   copy value an `in` parameter was bound to by the value itself, that was a
   drop of the caller's own, and the interpreter's liveness checks (debug
   builds) took the value to be gone. A function passing its own `f64`s to
   `float.lerp` and reading them after failed so
   (`184_inline_generic_reads_after`). The splice now binds the callee with
   `substitute_types` and cleans it with the same pass first.
7. **`Default`.** Measured; no default changed yet. `aot-compile
   --specialize` (the `All` policy ahead of time) and `script --jit
   --jit-specialize` against the same without, release build, hyperfine, the
   mean of five runs or more; every output identical:

   | program | jit | jit, specializing | aot | aot, specializing |
   |---|---:|---:|---:|---:|
   | fib | 356 ms | 0.99 | 346 ms | 1.01 |
   | sum | 330 ms | 0.99 | 302 ms | 1.00 |
   | primes | 347 ms | 1.01 | 336 ms | 0.99 |
   | wordfreq | 549 ms | 1.00 | 545 ms | 0.98 |
   | sieve | 797 ms | 0.72 | 698 ms | 0.72 |
   | mandelbrot | 570 ms | 1.01 | 545 ms | 1.00 |
   | nbody | 185 ms | 0.99 | 167 ms | 1.02 |
   | ordered | 549 ms | 1.02 | 521 ms | 0.94 |
   | store demo, 20,000 orders | 402 ms | 0.99 | 134 ms | 0.95 |

   Ahead of time it never loses: sieve, the case
   [AWFY](research/research-awfy-benchmarks.md) points at, is 28% faster,
   `ordered` and the store demo 5-6%, and the rest within noise. The
   executables grow by at most 35 KB of 5-10 MB, and `aot-compile` takes no
   measurably longer (3.03 s against 3.12 s for the store demo, within its
   spread).

   In the jit, sieve is 28% faster too, and nothing loses more than `ordered`'s
   2%. That took a tier between the generic and its instances. `ordered`
   calls each of its generics at four bindings, and with each instance
   warming up on its own in the interpreter, a generic called at four
   bindings ran four times as long there as unspecialized, where one compiled
   generic serves them all: 1.26. (Compiling an instance on its next call
   once its generic was hot brought that to 1.06, but compiled instances that
   were not hot, and was taken out.) Now every call to a generic counts
   toward the generic, whatever its binding, as well as toward its instance
   (`JitEngine::record_generic_call`). The generic is compiled over
   descriptors once it is hot, as without specializing; a call whose instance
   is not compiled yet runs in that; and an instance is compiled only once it
   is hot itself. So the generic's code is there for every binding not hot
   enough for its own, at the cost of compiling a hot generic once more than
   its hot instances. A planned interpreter call site at one binding enters
   the generic's code meanwhile, counting its calls, and offers every so many
   to the dispatcher (`SitePolicy::EnterCounting`, a countdown on the
   bytecode's `JitPlan`), so it moves to the instance when that is compiled.
   A site that binds a generic to its caller's type parameters is planned as
   any site is, rather than offering every call as phase 5 had it.

   One thing `--jit-stats` now gets wrong: the calls an `EnterCounting` site
   makes between offers ran in compiled code, but are reported as
   interpreted, as a `Count` site's are.

   Specialization stays an experiment, off by default everywhere: the
   policy's default is `Off`, and `--specialize` and `--jit-specialize` are
   the only ways to turn it on.

Phase 2 proves the design: `repeated[bool]` calling `push[bool]`, built
ahead of time, with no descriptor in either.

What phase 2 found: `sys/std` makes no call at a concrete binding inside its
own modules, since its generics forward their `T`, so the module graph's pass
makes nothing there. The chains form in script units, which instantiate
`push`, `of`, `unwrap_or` and the like into their `nested_units`. Many of the
library's generics are `inline` (`len`, `insert`, `get`) and are spliced
rather than instantiated, which is where the inlining order above matters.
The engine corpus has few generics; the standard library's suite is where
this is tested.

## Open questions

- **Partial instances.** Binding some parameters and renumbering the rest is a
  small change to `substitute`. A generic caller forwarding its `T` with a
  second parameter concrete could use one. Is any such site hot enough to
  matter?
- **Where the run-time pass lives.** `instantiate` is pure IR and needs no
  database, so the dispatcher can call it directly. Should a run-time instance
  also go through the compile-time pipeline's later passes (inline, simplify,
  promote), which today run per module in salsa, or is a subset callable
  without the database enough?
- **The REPL's default.** Should the REPL ever specialize ahead of time, or
  only through the JIT? The JIT is not used by the CLI's REPL today.
- **Whether the cleanup pass is worth its code.** The redundant drops and
  tracking cost a byte store and a branch each. Cranelift may already remove
  some of it.

Generics by reference made the boundary disappear. Since nothing converts
there, a generic and its instance can stand on either side of any call, and
specializing becomes a substitution that any engine can run at any time.
What is still open is choosing when it pays, which is the `Default` policy,
and that is a question of measurement.
