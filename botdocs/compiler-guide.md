# Datafun Compiler Guide

Reference for the datalove-datafun compiler architecture.

## Contents

- [Crate Organization](#user-content-crate-organization)
- [Module Compilation Pipeline](#user-content-module-compilation-pipeline)
  - [Token Gluing](#user-content-token-gluing)
  - [Phase 5: IR Lowering Detail](#user-content-phase-5-ir-lowering-detail)
  - [Const Parameter Specialization](#user-content-const-parameter-specialization)
  - [Const Evaluation](#user-content-const-evaluation)
- [Generics](#user-content-generics) -- orientation; the full account is [Generics: how it works](generics.md)
- [Native Riders](#user-content-native-riders)
  - [The Runtime Kernel and What Belongs in a Rider](#user-content-the-runtime-kernel-and-what-belongs-in-a-rider)
- [The Shipped Binary](#user-content-the-shipped-binary)
  - [Checkout and Release Builds](#user-content-checkout-and-release-builds)
  - [The ABI Check](#user-content-the-abi-check)
  - [Rider Manifests and Interfaces](#user-content-rider-manifests-and-interfaces)
- [Script Compilation Pipeline](#user-content-script-compilation-pipeline)
- [IR Types](#user-content-ir-types)
  - [IDs](#user-content-ids)
  - [Code Unit References](#user-content-code-unit-references)
  - [IrCodeUnit](#user-content-ircodeunit)
  - [Type Layout](#user-content-type-layout)
  - [Parameter Modes](#user-content-parameter-modes)
  - [Enum Instructions](#user-content-enum-instructions)
  - [Intrinsics](#user-content-intrinsics)
- [Salsa Patterns](#user-content-salsa-patterns)
  - [Database](#user-content-database)
  - [Tracked Functions](#user-content-tracked-functions)
  - [Incremental Compilation](#user-content-incremental-compilation)
  - [Reusing a Compiled World](#user-content-reusing-a-compiled-world)
  - [Parallel Execution](#user-content-parallel-execution)
- [Ownership Analysis](#user-content-ownership-analysis)
  - [Tracking Categories](#user-content-tracking-categories)
  - [Auto-adapt](#user-content-auto-adapt)
  - [Error Codes](#user-content-error-codes)
  - [Drop Schedule](#user-content-drop-schedule)
  - [AOT Tracking Bytes](#user-content-aot-tracking-bytes)
- [Execution Backends](#user-content-execution-backends)
  - [Leak Checking](#user-content-leak-checking)
- [Performance Notes](#user-content-performance-notes)
- [Entry Points](#user-content-entry-points)
  - [Compiling From Roots](#user-content-compiling-from-roots)
- [Test Patterns](#user-content-test-patterns)
  - [index-64](#user-content-index-64)
  - [Worldgen Coverage](#user-content-worldgen-coverage)
- [Key Files](#user-content-key-files)

## Crate Organization

### Core Compiler Pipeline

| Crate | Responsibility |
|-------|----------------|
| `datalove-datafun-parser` | Lexer, parser, bracer |
| `datalove-datafun-ast` | AST types (`Statement`, `Expr`, etc.) |
| `datalove-datafun-common` | Types shared by resolve and tycheck: `Type`, `TypeFunction`, `ParsedModuleGraph`, `DbClone`, `generics` |
| `datalove-datafun-resolve` | Name resolution (type aliases, function signatures) |
| `datalove-datafun-tycheck` | Type checking, call resolution, type synthesis |
| `datalove-datafun-sema` | Types shared by ownership analysis and lowering: `ExprTypes`, `CallTargets`, `AnalysisError`, `DropSchedule` |
| `datalove-datafun-ownership` | Ownership analysis (move/borrow/drop tracking) |
| `datalove-datafun-lower` | AST to IR lowering |
| `datalove-datafun-const` | Const evaluation (CTFE), const inlining, dead code elimination |
| `datalove-datafun-ir` | IR types (`IrCodeUnit`, `IrType`, `ValueId`, layout, registries) |
| `datalove-datafun-intrinsics` | `IntrinsicId` and signatures for `icall` operations |
| `datalove-datafun-inline` | Function inlining pass over IR, including cross-module and dynamic inlining |
| `datalove-datafun-compiler` | Salsa-tracked pipeline, `Database`, `compile_modules()`, `lower_module_graph_with_evaluator()`, specialization |
| `datalove-datafun` | High-level facade, `ModuleCompilationPipeline`, `ScriptCompiler`, workspaces, rider build/load |

### Execution

| Crate | Responsibility |
|-------|----------------|
| `datalove-datafun-interp` | Interpreter, `CallDispatcher`, `NativeFunctionTable`, CTFE evaluator |
| `datalove-datafun-cranelift` | Shared Cranelift utilities |
| `datalove-datafun-cranelift-aot` | Cranelift AOT compilation |
| `datalove-datafun-cranelift-jit` | JIT compilation, tiering and dynamic inlining dispatcher |
| `datalove-datafun-c-aot` | AOT backend emitting C11 source linked against the runtime |
| `datalove-rt` | The runtime: C ABI (`c`), Rust wrappers (`rust`), implementation (`impls`) |
| `datalove-rtdt` | Runtime type descriptors (`TyDesc`), runtime-side layout, anypack |

### Supporting

| Crate | Responsibility |
|-------|----------------|
| `bcts` (imported as `bct`) | Base compiler toolkit: `ModuleGraph`, `Module`, `ModuleId`, source maps, text interning |
| `datalove-datafun-pkg` | Package loading, `PackageWorld`, worldfile parsing, module resolution |
| `datalove-pkg-manifest` | A package's `manifest.toml`, read by `package_load` and by `sys/build.rs` |
| `datalove-rti` | The runtime's call table as riders see it, and `ABI_VERSION` |
| `datalove-buildinfo` | `BuildInfo`: whether this binary was built from a checkout or a release |
| `datalove-paths` | `work_dir()`, where native components are built |
| `datalove-ct` | Compile-time utilities, query logging |
| `datalove-diagnostic` | Diagnostic/error infrastructure |
| `datalove-datalit` | Data literal types and typechecking |
| `datalove-exampletest` | Snapshot test harness |
| `datalove-worldgen` | Generator for random worldfiles that typecheck |
| `datalove` | Thin facade crate holding a plain salsa `Database` |

### Drivers and tests

| Crate | Responsibility |
|-------|----------------|
| `datalove-cli` | Command line driver: build, run, AOT, native component linking |
| `datalove-repl` | REPL evaluation engine |
| `datalove-repl-rat` | Ratatui REPL application and terminal |
| `datalove-sys-packages` (`sys/`) | The system library as the binary carries it: embedded sources, linked riders. The library is the crate root, so it can be packaged |
| `datalove-rider-sys-std` (`sys/std/rider`) | Native rider implementations for `sys/std` |
| `datalove-tests` | Workspace-wide test suites |
| `datalove-rt-tests` | Runtime tests, separated so the runtime need not depend on datalit |
| `datalove-bench` | Divan benchmarks |

The `sys/` tree at the repository root is the standard library: `sys/std/*.dfm`
modules, plus the `sys/std/rider` Rust crate and the `rider.dli` interface
inside it.
`sys/std/rider` is a workspace member like anything under `crates/`.

`datalove-sys-packages` sits above everything else: nothing in the compiler depends
on it, so editing a `.dfm` recompiles no compiler crate. See [The Shipped
Binary](#user-content-the-shipped-binary).

## Module Compilation Pipeline

Module compilation runs through five phases. Phases 1-4 are handled by `compile_modules()`,
phase 5 by `lower_module_graph_with_evaluator()`.

```
Source Text
    |
    v
[Phase 1: Parse]
    |   parse_module_graph_with_mode
    |   Per-module: parse_module_full [tracked]
    |   Rider sources arrive here as raw strings
    v   Output: ParsedModuleGraph (statements + resolved_riders)
    |
[Phase 2: Name Resolution]
    |   resolve_all_names_with_mode
    |   Per-module: resolve_module_names [tracked]
    v   Output: AllModuleNameResolutions
    |
[Phase 3: Typecheck]
    |   typecheck_module_graph_with_mode
    |   Per-module: resolve_module_imports, typecheck_module [tracked]
    |   An importer asks resolve_module_exports of what it requires
    v   Output: ModuleGraphTypecheckResult
    |
[Phase 4: Ownership Analysis]
    |   analyze_module_graph_with_mode
    |   Per-module: analyze_module [tracked]
    v   Output: ModuleGraphAnalysis
    |
[Phase 5: IR Lowering]
    |   lower_module_graph_with_evaluator
    v   Output: ModuleGraphLoweringResult
```

Errors propagate between phases: modules with typecheck errors skip ownership analysis and lowering.

**Integer synthesis**: bare integer and hex literals synthesize as `int` (bigint).
With an expected type from context (binding annotation, function parameter,
checked arithmetic operand), the literal checks against that type instead.
Both `datalove-datafun-tycheck` and `datalove-datalit` follow this rule.

Float literals take their precision from context the same way, `f64` being
the fallback when nothing supplies one. Both defaults are the widest of their
family, on the grounds that a type chosen without knowing what it is for
should lose the least. A negation does not stop the
expected type reaching the literal under it: the sign says nothing about the
width, so `-3.9` checks against `f64` exactly as `3.9` does, and `-5` is out
of range for a `u32` rather than merely the wrong type.

The two languages reach that by different routes - datalit carries a sign
inside the literal token, datafun parses a negation as an operator over an
unsigned literal - which is a place they can drift apart, and did.
`literal_type_equiv_tests` holds them to the same answers.

Neither takes an integer literal where a float is expected, or the reverse.
There is no implicit conversion between the two families; see
[Numeric Widening](botspec.md) in the spec, and `f64.from_int` and
`int.from_f64` for the named conversions.

### Token Gluing

Whitespace is significant inside an expression: `1 . 5` is not a float and
`a -b` is two expressions. The rules are in [botspec.md](botspec.md) §2.4,
Spacing; this is where they live in the code.

The lexer breaks words at character-class boundaries and never puts them back
together, so `1.5` arrives as three tokens, and whitespace and comments are
filtered out before either parser sees a token. That leaves the spans as the
only evidence that two tokens touched. Two tokens are glued when the first
one's span ends where the second's begins; a comment between them leaves a gap,
which is the wanted answer for free.

All of it is in `bcts/src/parser_util.rs`, so that datafun and datalit give the
same answer for the same spelling:

- `TokenStream::prev_end` and `peek_next` are the primitives. `TokenStreamExt`
  builds `glued_left`, `glued_right` and `is_infix_spacing` on them.
- `eat_number` reads a numeric literal as far as it is glued, and both parsers
  call it. It reads a number whole even where the spacing was wrong, so a
  parser makes one complaint about the number rather than meeting its pieces
  again as something else.
- `Number::complaint` words that complaint, in bcts so both languages say the
  same thing. A suffix (`1u8`) is a field rather than an error, since bcts does
  not know whether its caller has units; datalove's message refusing one is
  `datalove-datalit::parser_util::suffix_complaint`, shared by both parsers.

The literal reader runs before fixity is judged, and fixity only sees the
operators it declined. That is why `2.5e-10` is one number while `x-1`
subtracts. There is no sign rule: `eat_number` takes a glued leading `-`
whenever it is offered one, and whether it is offered one is the grammar's
business. Datafun's prefix `-` claims the sign first in expression position;
datalit has no prefix operator, so there the literal owning it is the only
reading.

| Written | Said |
|---------|------|
| `1 . 5` | a float is written without spaces in it (write it as `1.5`) |
| `2.5e - 10` | an exponent is written without spaces in it (write it as `2.5e-10`) |
| `1u8` | numeric suffixes are not supported (write a type hint, as in `: u8 / 1`) |
| `a -1` | this `-` is spaced as a prefix operator |
| `p . 0`, `x ?` | this `.` / `?` is written apart from what it applies to |

The last two come from `lopsided_operator` in the datafun parser's `expr.rs`.
Datalit has no operators to misplace, and a `-` there that no number follows is
D013, "unexpected minus sign".

### Phase 5: IR Lowering Detail

Lowering has four internal phases that handle const evaluation correctly:

```
Phase 5a: Lower all functions
    |   lower_all_module_functions walks the graph,
    |   lower_module_functions [tracked] does a module
    v   Reused for both CTFE and final assembly
    |
Phase 5b: Evaluate consts
    |   evaluate_all_module_consts (non-tracked)
    |   Uses CTFE evaluator with lowered functions
    |   Function-level consts qualified as "func_name::const_name"
    v   Output: HashMap<ModuleId, ModulePreResolvedConsts>
    |
Phase 5c: Specialize comptime functions
    |   specialize_comptime_functions (non-tracked)
    |   One copy of a callee per instantiation; see below
    v   Output: the lowered IR, with the copies in it
    |
Phase 5d: Assemble modules
    |   lower_module [tracked]
    |   Reuses pre-lowered functions
    |   Inlines evaluated const values
    v   Output: ModuleGraphLoweringResult
```

Why this structure:
- Functions are lowered once and reused - const expressions can call functions via CTFE
- Const evaluation happens outside tracked functions (uses interpreter state)
- `lower_module` is tracked with pre-resolved consts as hashable input, enabling memoization

Both tracked phases key on the module, so an edit lowers the module that
changed. What makes that hold is the arguments: `reachable_func_ids` gives each
one the entries it can actually call rather than the whole world's, and the IR
travels under a handle so neither phase copies a module to hand it on, or
hashes one to look it up. The `incremental_lowering` tests assert the property
directly.

#### `ModuleLowered`, the handle the phase passes around

Phase 5a hands back a `ModuleLowered` per module rather than the IR. It is a
tracked struct with one untracked field, the `ModuleId`, and one `#[tracked]`
field holding `Arc<ModuleLoweredFunctions>`. A tracked struct's identity is a
hash of its untracked fields, so a handle's identity is a module's, and passing
one to a query costs a word. The IR is read through the tracked field's own
dependency edge, so a consumer still re-runs when the IR moves and still
backdates when it does not.

That is what lets the passes between 5a and 5d be memoized at all. They are
functions of the whole program's IR, and keying a query on the IR itself meant
hashing every instruction to answer it -- which `lower_module` was paying, at
around a tenth of an unchanged recompile.

```
lower_module_functions [tracked]  ->  ModuleLowered per module
module_shape_inputs [tracked]     ->  one module's part of the call graph
close_shapes_over_calls [tracked] ->  ModuleLowered per module, + shape errors
merge_module_strata [tracked]     ->  one ModuleLowered from the two strata
specialized_module [tracked]      ->  ModuleLowered for phase 5c's rewrite
lower_module [tracked]            ->  takes the handle, not the IR
```

**A handle is passed through, not re-minted, when its module did not change.**
A tracked struct's identity map belongs to the query instance that created it.
`close_shapes_over_calls` is keyed on the graph, so adding a module anywhere
gives it a new key, and every handle it minted there would get a new id --
which phase 5d is keyed on, so every module in the world would assemble again.
It therefore hands a module back under the handle it came in under whenever the
closure did not write to it, which `Arc::ptr_eq` answers exactly, since the
write-back reaches for `make_mut` only when it has something to write. The
`module_memo` fixtures are what catch this; they failed on precisely that
regression while it was being built.

**Phase 5c is gated on a memoized question.** `module_has_comptime_calls` is a
tracked query per handle, and a program where no module makes one skips
specialization without reading an instruction. Working out the plan otherwise
means walking every function of every module on every compile.

**Phase 5b is gated the same way.** `graph_declares_consts` is tracked on the
parsed graph; a program with no `const` anywhere has nothing for CTFE to
evaluate, and the module registry built to have CTFE on hand is not free.

#### The registries come out of memos

There are two `ModuleFunctionRegistry`s per compile -- `ctfe_module_registry`,
for const expressions that call across modules, and the one the pipeline hands
to the interpreter -- and each has an entry per function in the world. Building
them and dropping the previous pair was about two thirds of what an unchanged
recompile of the system library cost, and about 44% of the synthetic one.

Both are tracked. The CTFE one is keyed on phase 5a's handles. The pipeline's,
`module_function_registry` in `datafun/src/pipeline/module_pipeline.rs`, is
keyed on the `ModuleGraphLoweringResult`, which is a tracked struct whose
identity is the per-module results it holds -- so it is a word to hash and it
moves exactly when some module's assembled IR does. The native rider units go
in both, by `add_native_rider_units` in `tracked_lower.rs`: a const can call a
native, directly or through a function, so the CTFE registry needs them too.
The compiler core still never spells a linker symbol; `NativeContext::new`
takes the rider and function names and the IR crate's `native_symbol` does the
spelling, which is the native ABI's business. See [the native ABI](native-abi.md).

**The pipeline's is capped at `lru = 4`.** Its key moves on every edit, so the
memo for the key before it is of no use to anyone, and uncapped it grew the
database by about 140KB an edit. Eviction runs once per revision, so nothing is
dropped underneath a compile, and the capacity only has to cover the one a
recompile asks for plus the one an edit displaces.

`SharedModuleContext` borrows the function id lookup (`func_id_lookup`, also
tracked) out of its memo rather than holding a copy of every function name in
the world.

#### What is not tracked, and what stands in the way

`lower_module_graph_with_evaluator` is still a plain function, and the two
const evaluations under it still run in full on every compile. They cannot be
tracked as they stand, because they take a `Rc<RefCell<dyn CtfeEvaluator>>` and
a trait object is not a memo key. The gates above mean a program with no consts
never reaches them, which is why an unchanged recompile no longer pays for
them; a program that does have consts still pays on every compile.

`specialize_comptime_functions` has the same obstacle for the same reason: it
evaluates a comptime function's const bindings once the instantiation is known,
so it needs the evaluator too. `module_has_comptime_calls` gates it rather than
memoizing it, so a program that does specialize re-specializes on every
compile.

#### What the shape closure costs, and why it is shaped as it is

It is a fixpoint over the call graph -- see `close_shapes` for the rule and why
it settles -- so it cannot be split per module and it is keyed on every module
at once. That means the thing deciding whether an edit re-runs it is not its key
but **what it depends on**, and the work is in what surrounds the fixpoint
rather than in the fixpoint, which measured at a twenty-fifth of the pass.

Three things, and the order they were found in is the order of how much they
were worth:

- **It does not read the IR.** `module_shape_inputs` is tracked per handle and
  carries everything the graph-wide pass needs -- each function's declared
  shapes, its calls that bind type parameters, its callees, and its name for the
  error message. Reading one module's IR in the graph pass would make the
  fixpoint depend on all of them, which is what it used to do, and then a body
  edit anywhere re-ran the closure though no shape had moved. Memoizing the
  extraction alone did nothing while that dependency was still taken first.
- **The write-back asks before it writes, and is per module.**
  `shape_closed_module` is keyed on the handle plus that module's own settled
  shapes and the settled shapes of the functions it calls. Restricted rather
  than given the global map, which is the trick: the global map moves whenever
  any module's shapes do. Empty sets are dropped from both, so a module with no
  generic near it is passed through without its IR being read.
  `resolve_call_descriptors` returns what each call should hand over instead of
  writing it, so `Arc::make_mut` is reached for only when the answer moved --
  it used to copy every function in the program on any compile where a shape
  existed anywhere, which is also why the handle pass-through never fired for
  a program with a generic in it. `set_call_descriptors` keeps the
  write-through behaviour for the script paths, which own their units.
- **The three maps** -- placement, shapes, calls -- are keyed on
  `(IrModuleId, FuncId)` and rebuilt whenever the pass runs. They are
  `FxHashMap`s. This is the one place in the tree where the hasher was worth
  changing; sweeping all of them was tried once and backed out at about 1%.

`close_shapes_over_calls` no longer appears in a profile of an edit, or in
`query_census`. On the system library, editing a module of your own went from
3.1ms to 2.0ms across these.

The `skip_const_inlining` flag skips phase 5b and the inlining phase 5d does after it,
lowering const bindings as let bindings. Phase 5a still runs: a module-level const is
resolved where its reference is lowered rather than by a later pass, so there is nothing
there for the flag to skip. Used for testing CTFE accuracy. Callers of the high-level
pipeline set `CompilerOptions::const_inlining`.

### Const Parameter Specialization

Functions with `const` parameters are monomorphized during phase 5c:

```
Phase 5c: Specialize comptime functions
    |   specialize_comptime_functions
    |   Round, until no copy is made:
    |   - Scan the lowered IR for ComptimeCall, resolving each const
    |     argument against the Const instruction that defines it
    |   - Build one copy of the callee per instantiation not yet copied
    |   Then:
    |   - Point those call sites at the copy
    v   Output: the original functions, plus a copy per instantiation
```

**Specialization is additive.** The original function is kept, with the signature it was
lowered with, and the copies are added beside it as `name__ct0`, `name__ct1` and so on. A
transform that rewrote the callee's signature in place could not do that: it would oblige
the compiler to find and rewrite every call site at once, and a call site it missed would
pass a const argument into a parameter that had become something else.

That matters because the module graph is not the whole program. Script units compile
afterwards, one at a time, against modules already specialized, so a script line may name
an instantiation no module call site asked for. `ScriptCompiler::phase_specialize` handles
those separately, putting the copies in the script unit's own `nested_units` reached by
`CodeRef::Local` -- the same vehicle a script-local function uses -- so the module it
called into does not have to change. A call whose const argument did not survive as a
constant, which is every one of them under `skip_const_inlining`, keeps its `ComptimeCall`
and runs the original.

**What a const argument may be.** The name of a `const` binding, and nothing else --
not a literal, not an expression. The value has to be one the compiler already holds, and
a binding is the one form that says so on its face; anything else would mean deciding
case by case which shapes to see through, which is how a rule stops being one. It is
also what makes monomorphization the right shape: every call site's value is known when
the call is compiled. A const parameter counts, being a const binding within the body,
which is what lets one comptime function pass its parameter to another.

**Const parameters and type parameters combine**, independently: specialization deletes
the const parameters, erasure replaces the type parameters, and a copy is as generic as
its original. One copy per const instantiation serves every type instantiation, the call
site handing over the descriptors it would have anyway. `ComptimeCall` carries
`type_args` and `shape_descriptors` exactly as `Call` does, and the rewrite carries both
across. The one refused case is a const parameter whose own type is a type parameter,
`const x: T` (`ComptimeParamOfGenericType`): the const argument would be all that says
what `T` is, and a copy is built by substituting into cloned blocks, so it cannot change
its signature or the erasure decisions in its body.

**Where instantiations come from.** They are read out of the IR. A const argument is
passed by reference, as a const parameter is lowered, and the operand at a call site is
the const itself: defined by a `Const`, or by a `StaticRef` carrying its value. So the
values the rewrite looks for are the values the plan was built from, and the two cannot
disagree.
Specialization used to resolve them instead by the *name* of the const binding, which the
typechecker recorded in a `ComptimeCallSiteRegistry`; that could disagree, because two
function-level consts sharing a name in one module resolved to the same value. The
registry is gone -- the typechecker still enforces that a const argument names a const
binding, but records nothing.

**Identity of the copies.** `compute_func_id_map` assigns `FuncId`s from source statement
order and cannot assign one here, since the copies are in nobody's source. They take ids
after the highest source-derived one in their module. The name has to be a valid
identifier, because both AOT backends use it as a linker symbol.

**Const bindings inside a comptime function.** One naming a const parameter has a value
per instantiation rather than one, so phase 5b leaves it alone. Making a copy evaluates it:
the parameters are seeded with what the instantiation passes, the same CTFE that evaluates
every other const runs, and `inline_function_consts` writes the results into the copy --
which also turns a branch whose condition has become constant into a jump. So `const`
means the same thing inside a comptime function as outside one, including through
arithmetic and through calls.

**Rounds.** A comptime function that calls another only says what it passes once its own
const parameters have been substituted, so copying can uncover instantiations the scan
before it could not see. The scan and the copying therefore repeat until a round makes no
copy.

**Limit.** `MAX_INSTANTIATIONS` caps a function at 64. Each instantiation is a whole copy
in the object file, so going over is reported rather than paid. `MAX_ROUNDS` caps the
rounds, as a backstop; running out leaves call sites unspecialized rather than wrong.

**Call site handling:** The lowering phase emits `ComptimeCall` instructions for calls to
functions with const parameters. Specialization turns the ones it can place into a
regular `Call` to the copy, leaving out the const arguments the copy does not take. They
were borrowed, so nothing is dropped. A copy defines each const parameter at entry, as a
`StaticRef` for a non-copy type and a `Const` otherwise.

**Testing:** The `skip_specialization` flag (like `skip_const_inlining`) allows differential
testing - comparing specialized vs unspecialized output to verify correctness.

**The union-branch approach, tried and dropped.** The first implementation compiled one
function per callee with a `Switch` on a tag, one arm per instantiation, on the theory
that a branch is cheaper than a copy. It is not: each arm *was* a copy of the body
(`build_dispatch_blocks` cloned every block once per instantiation), so it was
monomorphization plus a switch on a value every call site passed as a constant, in one
oversized symbol that inlined whole or not at all, and whose instantiations shared one
call count in the JIT's tiering. What decided it, though, was that it rewrote the
callee's signature in place, which obliged every call site in the program to be found
and rewritten -- and script units compile later than that. A module function called from
both a module and a script miscompiled exactly that way (fixture
`specialize_differential/023_module_and_script_call`). `ComptimeCall` survives from it.
If a call site is ever allowed to pass a value chosen at run time from a known set, the
tag becomes the right shape for that case.

**What remains.** A function all of whose call sites were specialized keeps an original
nobody calls. That is deliberate, since a later script line may call it, but it is dead
weight in an AOT build, where there is no later line.

See [Generics and Specialization](plan-generics.md) for why this machinery is not a
foundation for type parameters.

### Field Projections

A field projection borrows part of an aggregate. It does not produce a value
anyone owns, so reading one has to go through a reference:

- **A copy field** reads with `GetField` and needs nothing else.
- **A non-copy field** is refused outright unless the typechecker is in
  `ref_context` (`synthesize.rs`), which `debuglog`, `@`, and a `ref`/`mut`/`out`
  argument position each set. Outside those, `p.a` on a linear `a` is
  `NonCopyFieldProjection`, because it would move the field out of a place the
  aggregate still holds.
- **In a borrowing position** it lowers to `GetFieldRef` and is read through the
  reference: `v = getfieldref p.0` then `clone *v`, or `debuglog *v`.
- **As a `ref`/`mut`/`out` argument** the reference itself is what gets passed,
  so `lower_expression_for_ref` hands back `Operand::Value` holding a `Ref`
  rather than dereferencing it. An `out` argument drops the old value through
  the reference first.

That last distinction is why `lower_operand` and `lower_expression_for_ref` are
not the same function: one wants what the reference points at, the other wants
the reference. `lower_operand` used to send every projection through
`lower_expression`, which moves the field out and records it as a temporary to
drop -- freeing a field the aggregate still held. `@` on a linear field went
that way and produced `free() called on untracked pointer`, and on the way to it
a `copy_nonoverlapping` whose ranges overlapped. Fixture:
`interp/949_field_proj_clone`.

A tuple element is the same projection with a numeric selector rather than a
name, and goes through the same code for all of it -- read, borrow, clone,
`ref`/`mut`/`out`, assignment and nesting. `interp/949_field_proj_clone` runs
the two side by side and they agree exactly.

Only a linear field takes that route in an operand position. A copy field keeps
the direct `GetField`, having nothing to move out and nothing to free.

### Index Projections

An index step is not the same operation and does not have the same problem.
`GetField` copies the field's bytes -- shallow, and its own comment says only
copy types may go that way. `ListGet`, `MapGet` and `TensorGet` call
`dtlv_rti_clone_local`, so the element arrives owned and dropping it afterwards
is right. That is why `a[i]?@` has always been safe where `p.a@` was not, and
why routing index steps through the borrow path breaks the `list_index_*`
fixtures: it turns a clone-on-read into a borrow.

The typechecker gates the two alike -- `NonCopyIndexProjection` unless
`ref_context` -- so a linear element still needs `@` or a borrow to be read.
Writing through an index works for a list and a map, by `mut`, by `out` and by
assignment (`interp/950_index_write_forms`).

In an operand position a non-copy index is borrowed like a field, so `a[i]?@`
clones once rather than cloning through `ListGet` and then cloning that. It
holds for an erased element too: the reference carries the element's descriptor,
narrowed from the container's, so reading through it reads the bytes as what
they are. See
[Generics](generics.md#user-content-getting-further-in).

A copy element keeps the direct read: there is nothing to move out and nothing
to free, so a borrow would buy nothing.

A tensor index is refused in all three, with `ViewTypeMutBinding`: it gives a
view of a row rather than an element, and a view may not be bound mutably
(`interp/951_tensor_index_not_mutable`). Reading and cloning one is fine.

### Const Evaluation

Const evaluation in `evaluate_single_const` has three fast paths:

1. **Simple literals** - booleans, integers, floats, strings, None extracted directly
2. **Const references** - look up already-evaluated const from `resolved_so_far` map
3. **Complex expressions** - lower to a minimal script code unit, execute via CTFE, extract result

Function calls in const expressions work because `lowered_functions` are passed to the CTFE evaluator.

## Generics

A generic function is compiled once, over shapes that fit whatever the call site
supplies, with type descriptors saying what is really in those shapes.
**[Generics: how it works](generics.md)** is the full description: erasure,
where descriptors come from, how a projection keeps one, and what each backend
carries. What follows is the orientation.

`erased_param_type` in datafun-ir is the one place that decides what a position
becomes, read by the caller and the callee both, because a call site converting
into a shape the callee was not compiled for is not a mismatch anything reports
-- it is two sizes disagreeing about the same bytes. It forks on whether the
position is borrowed.

**Owned** -- `in`, `out`, the return -- is converted at the boundary:

| Written | Becomes | Cost |
|---------|---------|------|
| `T` | `data` | wraps; a narrow scalar rides in the two words |
| `?T`, `!T`, `(T, u32)`, `{ a: T }`, `term W T`, an enum payload | the same shape with the parts converted | a walk over as many parts as the type has |
| `[T]`, `#{T}`, `%{K = V}`, a tensor, a table | `data`, wrapped whole | one small allocation, elements untouched |

The third is the one worth explaining. A list is a pointer, a length and a
capacity whatever its elements are -- `IrType::List(_)` computes its layout as
`struct_layout::<rtdt::List>()`, and the `_` is the point -- so it is already
the right size and there is nothing to convert it into. What it lacks is the
element type, and wrapping is what gives it somewhere to keep one. Converting
`[u32]` into a list of `data` would mean rebuilding it element by element; that
is the O(n) the design avoids, and it is avoided by not doing it rather than by
refusing the signature.

**Borrowed** -- `ref`, `mut` -- is not converted at all. The callee's static
type for the parameter is therefore the erased one, which is a lie about the
layout, and the descriptor for what really arrived comes from the call site,
recorded in `FunctionContext::descriptor_params`.

That lie is what most of the machinery exists to handle. A projection of such a
parameter narrows the descriptor alongside the pointer: `resolve_ref_descriptors`
says what each reference points at, and field offsets, element strides and
read-through widths come from there rather than from the static type. A value
absent from that map is the ordinary case -- the offset folds at compile time
and the emitted code is what it always was, so this costs nothing outside a
generic. An owned container is opened with `DataBorrow` first, which turns it
into the borrowed case.

Converting between two shapes is `convert` in the runtime, walking two
descriptor pointers and converting at each position one of them calls `data`.
`erase_local`, `reify_local` and `clone_erased_local` are its three entry
points. Whether anything was erased is a *structural* question rather than a
size one: a `data` is two words and so is a `string`.

The pieces: `Var` in `datalit::tycheck::Type`, built only by datafun-resolve
seeding the alias map; `IrType::from_datalit` mapping it to `data`, which is
the whole of erasure; `bind_type_params` and `substitute_type_params` in
`datafun-common::generics`; `Erase` and `Reify` at call boundaries.

A generic function calling another passes its own type parameter along, and the
value is already in the erased shape when it does. `Erase` and `Reify` are
skipped in that case -- the argument's `IrType` already equals the parameter's
erased shape, and the result type already equals the erased return shape.
Erasing an erased value boxes the box, and the callee finds a `data` where the
value should be.

What it does not do -- bounds beyond `float`, `fixedint` and `ord`, so nothing
can be done to a bare `T` but move it, drop it, clone it, print it and hand it
back -- and why, is in
[What is refused](generics.md#user-content-what-is-refused). Read that before
assuming something is a bug.

## Native Riders

A rider is a set of native function signatures backed by a Rust crate. A module
writes `require rider <alias>` and then imports the names it wants:

```datalove
require rider std
import std.string_len
```

The signatures live in a `.dli` interface file (`sys/std/rider/rider.dli`), or in a
`rider <name>` worldfile section for tests. Each is a `native fun` declaration,
and they may be generic:

```datalove
native fun string_len(ref self: string): index
native fun list_push<T>(mut self: [T], elem: T)
```

How it flows through the pipeline:

- Rider sources travel as `Vec<(String, String)>` (alias, source) into
  `compile_modules`, because building a `RiderInterface` creates a
  `TypeFunction`, which is `#[salsa::tracked]` and so can only happen inside a
  tracked function. `parse_module_graph` builds them and stores them on
  `ParsedModuleGraph::resolved_riders`.
- Each distinct alias gets a synthetic `ModuleId` with path `@rider/{alias}`,
  created once in `build_resolved_riders_from_sources` and shared by every
  module that requires it. `compute_func_id_map` assigns rider functions
  `IrModuleId`s after the regular modules.
- The typechecker resolves imports from riders in
  `resolve_module_imports_internal`.
- `add_native_rider_units` registers an `IrCodeUnit` per rider function whose
  context is `CodeUnitContext::Native`, carrying the linker symbol
  `dlr_{alias}__{func}` and no blocks.

Execution:

- **Interpreter**: `NativeFunctionTable` (`interp/src/native.rs`) maps linker
  symbol to the rider's function, called through the C ABI by `call_c` (or to
  a Rust closure, which tests register). The `Call` and `ComptimeCall` handlers check it before
  the regular dispatcher. Arguments follow the interpreter's own conventions -
  `in` params are moved values, `out` params are destinations, `ref`/`mut` are
  borrowed pointers.
- **Loading**: there are two ways a rider's functions reach the table. Which
  one applies depends on whether the driver already has them in its own
  process, not on where the rider came from.
  - *Linked in.* `sys/std`'s rider is an ordinary dependency of the binary, so
    its functions are already in the process. `sys/std/rider/build.rs` reads
    `rider.dli` and generates a `symbols()` table pairing each declared name
    with `dlr_std__{name} as *const ()`, which is also what forces the linker
    to keep them. `rider_load::register_linked_natives` matches the symbols
    the compiled modules call against that table. No cargo, no dlopen. This is
    how the `datalove` binary runs `sys/std` natives under the interpreter and
    the JIT - an AOT-compiled program is a separate executable, so addresses
    in this process are no use to it and it links a built component instead.
  - *Built from source.* Every rider with a crate directory - a user
    package's, and `sys/std`'s, whether the library came off disk or out of
    the binary - goes through `pipeline/rider_build.rs`, which synthesizes one
    `datalove-native-component` crate depending on every such rider crate so
    that whatever they share is shared once. Builds are cached per work dir,
    rider set and kind. The workspace's `work_dir` is where this happens; a
    workspace with riders and no work dir - a worldfile-derived one, for
    instance - cannot build them.

    There are two kinds, and they differ in whether the runtime is inside:

    - `build_rider_dylib` gives the shared library `rider_load::load_rider_library`
      dlopens. **The runtime is not in it.** The process loading it has one
      already, and a rider reaches it through the table on the handle rather
      than by name, so the library resolves nothing at load time: it has no
      undefined `dtlv_rti_*` symbols and the host exports none. That keeps one
      `RtLocal` and one allocator in the process however many riders load, and
      it is why the library is under a megabyte rather than the forty-odd a
      bundled runtime cost.
    - `build_component_staticlib` gives the archive an AOT-compiled program
      links. **The runtime is in it**, because that program is a separate
      executable with no host to resolve against. An empty rider set gives a
      runtime-only archive, which is what a program calling no rider still
      needs: `aot::runtime_only_component`, for callers handed an object file
      rather than a workspace, builds one in `rider_build::default_work_dir`.

    Only the `dlr_*` rider functions are looked up by name, which the loader
    does for a library it has opened on every platform. Nothing is resolved
    the other way, into the host, which is the direction that needs
    `-rdynamic` on ELF and has no equivalent on Windows at all.

  Both end at `register_native`, so the interpreter sees no difference. Both
  also return the raw addresses, which the JIT needs: it calls natives through
  a trampoline built from the address rather than through the interpreter's
  table, so a driver must feed them to `JitEngine::register_native_symbol` as
  well. Missing that is not a fallback to the interpreter but a panic inside
  JIT compilation, which aborts the process.

Rider implementations follow the runtime C ABI:
`extern "C-unwind" fn(rt, arg0_ptr, arg0_tydesc, ..., result_out, result_tydesc) -> u8`,
returning 1 for Ok and 2 for Error.

### The Runtime Kernel and What Belongs in a Rider

A package has at most one rider, shared by all its modules, and `sys/std` uses the
same mechanism as any user package. The rule for what stays a `dtlv_rti_*` call the
codegen emits by name, rather than a native function reached through the module
system, is whether rider code itself needs it:

- **The kernel**: `init` and `shutdown`, memory allocation, type descriptors,
  `debuglog`, `any_destroy` and `any_clone`. A rider cannot be built without these,
  so they cannot live in one.
- **Everything else belongs in a rider**: list, map, set, string, int arithmetic,
  table and tensor operations.

That migration is incomplete. The Cranelift backends still call constructors and
domain operations as hard-coded runtime functions -- `list_create`,
`list_build_from_slice`, `list_push`, `string_from_bytes`, the `btreemap_*` and
`btreeset_*` builders, `table_*`, `tensor_init`, `int_add` and the rest of the bigint
arithmetic -- mostly because literals and operators lower to them directly. Moving one
means the codegen stops naming it and the module that wants it imports it.

## The Shipped Binary

An installed `datalove` carries its standard library. `repl`, `script` and
`script --jit` work with `sys/` deleted and cargo off `PATH`. `aot-compile`
does not: it emits a separate executable, and the runtime and riders that
executable calls have to reach it as a library the linker is given, which is
built with cargo.

The library did not always travel. The compiler used to find it with
`env!("CARGO_MANIFEST_DIR")` at three sites, so an installed binary depended
on the checkout it was compiled from still existing, unmoved. When it was
missing the failure was silent - the REPL's engine thread died and every entry
sat at "parsing..." forever.

Two things travel inside the binary, both assembled by
`sys/`, which is the library and a crate at once:

| What | How | Where it comes from |
|------|-----|---------------------|
| Module sources | `build.rs` walks `sys/`, emits a table of `include_str!` | `sys/*/*.dfm` |
| Rider interfaces | The rider crate's `INTERFACE` constant; see [below](#user-content-rider-manifests-and-interfaces) | `sys/*/rider/rider.dli` |
| Rider functions | `datalove-rider-sys-std` is a normal dependency; its generated `symbols()` gives addresses | `sys/std/rider` |

`system_library()` assembles them into a `SystemLibrary`, which is a
`PackageLibrary` of sources plus `natives: Vec<(String, *const ())>`. Drivers
hand it to `WorkspaceDescriptor::from_system_library` and to
`register_linked_natives`. The raw addresses are not `Send`, which is why
`ThreadedExecutor::spawn` takes `fn() -> SystemLibrary` and calls it on the
worker thread rather than being handed the value.

**The rider's crate is named, not embedded.** `build.rs` writes each package's
`rider/` directory into the table as `rider_crate_dir`, by the same convention
`package_load` uses when it reads a package off disk, and `system_library()`
puts it in `RiderDescriptor::crate_dir`. So the std rider reaches an
AOT-compiled program the way any other rider does: the compiler synthesizes a
native component for whatever riders the module graph holds and builds it with
cargo. See [Native Riders](#user-content-native-riders).

That is what lets a program mix `sys/` with a rider of its own. The binary
used to link a prebuilt archive containing the std rider and nothing else, so
a script importing a package with its own rider failed at link time with
undefined symbols. `datalove_paths::work_dir()` gives the build somewhere to
happen - `$XDG_CACHE_HOME/datalove/work/<ABI_VERSION>` - and cargo caches within
it, so the component for a given rider set is rebuilt only when one of its rider
crates changes.

### What this costs in the tree

The embedding sits at the top of the crate graph on purpose. Had it gone where
`load_default_sys` used to live, in `datalove-datafun`, every `.dfm` edit would
invalidate the bottom of the stack: 22 s to rebuild what `just test` compiles,
against 0 s before. From `datalove-sys-packages`, which only the CLI and a couple of
test targets depend on, the same edit costs about 7 s, nearly all of it
relinking the debug binary.

The loop that matters for stdlib work is untouched. `std_tests` and
`std_all_tests` compile `sys/` off disk through
`WorkspaceDescriptor::load_sys_dir`, so `cargo test -p datalove-datafun --test
std_tests` after editing a module recompiles nothing at all.

That only holds because the two copies cannot drift.
`sys/tests/embedded_matches_tree.rs` asserts the embedded
table is byte-identical to `sys/`, and that every `native fun` a rider
interface declares is linked in. The REPL's `engine_tests` run against
`system_library()`, so the suite covers the shipped path too.

Editing `datalove-rt`, `datalove-rtdt` or the rider crate no longer re-runs
anything at build time. It invalidates the native components instead, which
cargo rebuilds incrementally the next time a suite links one. Each work dir
carries its own target directory, so the cost is per rider set rather than per
build.

### Checkout and Release Builds

The component `rider_build` synthesizes has to name the runtime crates and
every rider as cargo dependencies, and there are exactly two ways to do it:
by path, when the sources are beside us, or by an exactly pinned version
(`=x.y.z`), when they are published. A range would only promise something
semver-compatible, which is not the claim that matters when a rider and the
runtime it loads into must agree on the layout of everything between them.

Nothing at run time can tell which applies, so `datalove-buildinfo`'s build
script decides while the answer is in front of it and bakes in a `BuildInfo`:
`Prod { version }` or `Local { git_sha, checkout }`. It does **not** ask git
whether it is in a repository -- a published crate unpacked or vendored inside
somebody else's repository is in a checkout, just not this one. It walks up
from its own manifest for a directory holding both `sys/std` and
`crates/datalove-buildinfo`, which is this tree's shape and no other's, and
uses git only for the revision, so a tarball of the tree is still the tree.
`git_sha` is as of whenever `datalove-buildinfo` last compiled, not of this
moment: watching `.git/HEAD` would relink everything above it on every commit,
for a field nothing reads yet.

`runtime_dep` reads that for the runtime crates, which have no package to say
where they came from. A rider's package does say: one with Rust source in its
`rider/` directory is named by path, and one carrying only its interface is
named by the version its manifest asks for. A `Local` binary whose checkout
has gone says so, naming the directory, rather than leaving cargo to report a
missing path. `datalove --version` prints which kind of build it is.

So `cargo install datalove-cli` from a registry is meant to work, and the
workspace is published to crates.io (0.1.0 at the time of writing). Every crate
but the test and bench ones publishes, `just publish-check` dry-runs it, and
`just local-registry` builds a registry out of the tree to try the release path
against before anything is uploaded. `datalove docs` still reads
`env!("CARGO_MANIFEST_DIR")`, being a repository tool.

### The ABI Check

A rider library and the process loading it each compile their own copy of
`datalove-rti` and `datalove-rtdt`, and nothing else would notice if the two
disagreed: a rider would call a function with the wrong arguments or read a
field at the wrong offset and carry on. So `datalove-rti::ABI_VERSION` hashes
the shape of the runtime's call table and the sizes and offsets of the `rtdt`
types that cross. It deliberately leaves out crate versions -- a release that
moves no field should not invalidate a rider, and a version says nothing about
whether `index-64` is on.

`rider_build` writes `DLR_ABI_VERSION` into every dylib component it
synthesizes, so no rider author does anything. `load_rider_library` reads it
before looking up any `dlr_*` symbol and refuses a library whose value differs
or is absent. Only a dylib is loaded, so only a dylib says. The shared work dir
is named by the same value, so two datalove binaries sharing a cache do not
build over each other. `rider_abi_tests` builds libraries that lie about their
interface and checks both lies are refused.

### Rider Manifests and Interfaces

A package with a rider has a `manifest.toml` whose `[rider]` section names the
crate and its version, read by `datalove-pkg-manifest`. Unknown keys are
refused rather than ignored, so a manifest written for a later datalove says
so instead of half working, and a package with a rider and no manifest is an
error rather than a guess. The manifest is what survives once the Rust source
is stripped from a published datalove package.

The interface lives inside the rider crate, at `rider/rider.dli`, so the
crate's own `build.rs` can read it within its package root. It reaches the
compiler as Rust, not as a file: the crate exports
`INTERFACE = include_str!("../rider.dli")`, and `sys/build.rs` writes
`rider_interface: Some(<crate>::INTERFACE)` into the embedded table. The
reason is a cargo behaviour worth knowing: **a nested `Cargo.toml` silently
removes its whole directory from the package around it, and `include` cannot
override that.** `datalove-sys-packages` is `sys/`, so `sys/std/rider/` is never
in it, and the interface arrives by dependency instead.
`embedded_matches_tree` checks that route against the file on disk.

## Script Compilation Pipeline

Scripts compile incrementally in REPL-style, accumulating exports across units.

```
[Phase 1: Parse + Resolve + Typecheck]
    |   Standard pipeline through typecheck
    v
[Phase 2: Lower functions]
    |   phase_lower_functions
    |   Lower all functions once
    v   Reused for CTFE and final assembly
    |
[Phase 3: Const evaluation]
    |   phase_const_eval
    |   evaluate_script_consts (script-level)
    |   evaluate_function_consts (function-level, uses script results)
    v
[Phase 4: Assemble IR]
    |   phase_assemble_ir
    |   Combines functions + module code
    |   inline_script_consts replaces const refs
    v   Output: IrCodeUnit with a Script context
```

Script units accumulate via `AccumulatedLowerBindings`:
- Tracks exports from prior units
- Each unit increments `accumulated_unit_specs`
- Functions reference prior units via `CodeRef::External { unit, id }`

### Ownership across units

A unit copies out of the bindings earlier units own rather than taking from
them: lowering emits `Clone` for a consuming use that resolves to an
`ExternalValue`/`ExternalSlot`. Every line of a REPL is a unit, so taking
would mean inspecting a value consumed it and binding it to a new name
emptied the old one. A whole-file script is one unit and has no such uses, so
this changes nothing about running a script.

A unit can still give away a binding it defined itself, and then the name
outlives its value. Ownership analysis reports those names in
`ScriptAnalysisData::dead_exports`, `ScriptCompiler` remembers them in
`dead_externals`, and using one is D013. Assigning to such a name revives it
(`revived_exports`), which is why `unit_end` covers every non-Copy slot rather
than only live ones - a later unit can assign to a slot this one gave away, and
the cleanup list is fixed at lowering time.

An expression unit that is a bare name computes nothing: lowering records the
name in `ScriptContext::result_name` with no result value, and the executor
reads that binding where it lives instead of copying it to print it.

## IR Types

What follows is the part of the IR a reader of this guide needs in order to
follow the pipeline. The complete reference -- every instruction and
terminator, the layout rules, erasure and descriptors, the passes and the
invariants -- is [The Datafun IR](ir.md).

### IDs

```rust
pub struct ValueId(pub u32);    // SSA value (immutable)
pub struct SlotId(pub u32);     // Mutable slot (var bindings)
pub struct ParamId(pub u32);    // Function parameter
pub struct BlockId(pub u32);    // Control flow block
pub struct FuncId(pub u32);     // Module-local function ID
pub struct CodeUnitId(pub u32); // Code unit, local to its containing scope
pub struct CallSiteId(pub u32); // Call site, unique within a code unit
pub struct IrModuleId(pub u32); // Module index (not salsa ModuleId)
```

`CodeUnitId` is the unified addressing scheme that replaced `FuncId` inside the
IR. `FuncId` survives above it, in the compiler's `FuncIdMap` from
`(ModuleId, name)` to `(IrModuleId, FuncId)`; the two are numerically the same
where both appear. `CallSiteId` stays stable across IR transformations like
inlining, which is what the JIT counts for tiering decisions.

### Code Unit References

```rust
pub enum CodeRef {
    Local(CodeUnitId),                             // Same module/unit
    External { unit: u32, id: CodeUnitId },        // Previous script unit
    Module { module: IrModuleId, id: CodeUnitId }, // Different module
}
```

`Module` uses numeric `IrModuleId` (not salsa `ModuleId`) for serializability.

**`Local` is relative, and is not a name to remember a function by.** It is a
position in whichever unit's list is in scope, and every script unit numbers its
own functions from zero, so `Local(1)` means a different function in each of
them. Resolution is fine -- `ExecutionContext` holds one unit's list at a time,
and only `CodeRef::External` swaps it -- but anything that *remembers* a
function between calls has to say which unit as well. The dynamic inliner's
optimized bodies and the JIT's compiled ones both did not, so a hot function in
one REPL line was executed in place of a different function at the same id in
the next. `FuncIdentity` (interp `dispatch.rs`) is what those key on now: it
resolves `Local` against the scope unit, which also makes it agree with the
`External` a later unit uses for the same function. `ExecutionContext` carries
the unit for this, and the JIT keys its table and its code cells on
`FuncIdentity` too.
Fixture: `interp/947_crossunit_local_id_reuse`, which the tuned and chaos
dispatcher suites run against the plain interpreter.

The same goes for anything that carries a reference out of the body it was
written in. A JIT stub encodes its callee's `FuncIdentity`, resolved when the
stub is built, because compiled code from one unit calls compiled code from an
earlier one directly and the trampoline cannot know whose code a `Local` came
from. And the dynamic inliner rewrites an earlier unit's callee's local calls
to `External` ones (`with_calls_into_unit`) before moving its body into a later
unit's function. Both are covered by the last units of
`interp/999_inline_caller_in_earlier_unit`, which the plain jit suite and both
dispatcher suites get wrong without them.

### IrCodeUnit

One type represents functions, script units and native functions. The `context`
field decides execution semantics.

```rust
pub struct IrCodeUnit {
    pub id: CodeUnitId,
    pub name: String,

    // Body.
    pub blocks: Vec<IrBlock>,
    pub value_count: u32,
    pub slot_count: u32,
    pub call_site_count: u32,
    pub value_types: Vec<IrType>,
    pub slot_types: Vec<IrType>,
    pub tracked_slots: Vec<SlotId>,
    pub const_values: Vec<(String, ValueId)>,
    pub symbols: SymbolTable,

    // Context.
    pub context: CodeUnitContext,

    // Units defined inside this one.
    pub nested_units: Vec<IrCodeUnit>,
}

pub enum CodeUnitContext {
    Function(FunctionContext),
    Script(ScriptContext),
    Native(NativeContext),
}
```

```rust
pub struct FunctionContext {
    pub params: Vec<ParamId>,
    pub param_modes: Vec<ParamMode>,
    pub param_types: Vec<IrType>,
    pub return_type: IrType,
    pub tracked_params: Vec<ParamId>,    // Out params needing runtime tracking
    pub descriptor_params: Vec<ParamId>, // Descriptors the caller supplies
}

pub struct ScriptContext {
    pub unit_end_values: Vec<ValueId>,
    pub unit_end_slots: Vec<SlotId>,
    pub result: Option<ValueId>,
    pub result_name: Option<String>,
    pub exports: Vec<(String, ExportBinding)>,
}

pub struct NativeContext {
    pub param_modes: Vec<ParamMode>,
    pub param_types: Vec<IrType>,
    pub return_type: IrType,
    pub symbol: String, // e.g. "dlr_std__list_push"
}
```

An `IrModule` is `{ functions: Vec<IrCodeUnit>, symbols: SymbolTable }`.
Lookup at runtime goes through `ModuleFunctionRegistry` (keyed by
`(IrModuleId, CodeUnitId)`), `UnitFunctionRegistry` (indexed by script unit),
or `FunctionRegistry`, which holds both.

### Type Layout

`datalove_datafun_ir::layout` is the single authority on how an `IrType` is
laid out. Both AOT backends and the CTFE evaluator compile against it, and
`datalove_rtdt::layout` computes the same layouts from runtime type
descriptors, which is what the runtime reads values back through.

Generated code writes at offsets from the first and the runtime reads at
offsets from the second, so a disagreement corrupts values rather than
failing a build. `layout_conformance_tests` walks a corpus of types and
checks size, alignment, field offsets, variant payload offsets and tag
payload offsets in both directions.

Nothing should open-code the arithmetic. Payload offsets have named
functions in both authorities:

| | `ir::layout` | `rtdt::layout` |
|---|---|---|
| enum variant | `enum_payload_offset(ty)` | `enum_payload_offset(align)` |
| `?T` | `option_payload_offset(ty)` | `option_payload_offset(align)` |
| `!T` | `result_payload_offset(ty)` | `result_payload_offset(align)` |

### Parameter Modes

```rust
pub enum ParamMode {
    In,  // Ownership transfers to callee
    Out, // Write-only, callee must initialize
    Ref, // Read-only borrow
    Mut, // Read-write borrow
}
```

Call sites repeat the mode: `ExprFunctionCall.arg_modes` holds the marker
written before each argument, `None` meaning `in`. Typechecking rejects any
disagreement with the callee's declared mode (F057), so later phases can read
the mode off the call site alone. Ownership analysis does exactly that, which
is why it needs no resolved call target to know how an argument is passed.

Reading a parameter is `Operand::Param(p)`, not an instruction. Writing has
four forms:

- `ParamStore { param, value }` - store to Mut (destroys old value)
- `ParamStoreTracked { param, value }` - store to Out (checks tracking byte)
- `ParamSetField { param, field_path, value }` - field write to Mut
- `ParamSetFieldTracked { param, field_path, value }` - field write to Out

`RefStore` and `RefSetField` generalize these to any reference-like operand,
which is what inlining needs to write straight to the caller's location.

For `out` params, the **caller** destroys the existing value before the call via `DropViaRef`.

### Enum Instructions

```rust
// Read u32 discriminant tag from enum value. Borrows src (does not consume).
EnumDiscriminant { dest: ValueId, src: Operand }

// Move payload out of enum into dest. Consumes src.
EnumPayload { dest: ValueId, src: Operand, variant_index: u32 }

// Construct enum value with given variant and optional payload.
EnumVariant { dest: ValueId, variant_index: u32, payload: Option<Operand> }
```

Match lowering emits `EnumDiscriminant` to read the tag, then a chain of
comparisons branching to arm blocks. Atom arms `Drop` the input; term arms
use `EnumPayload` to extract the binding.

### Intrinsics

`icall name(args)` compiles to a single machine operation with no call
overhead. `datalove-datafun-intrinsics` defines `IntrinsicId` with stable
discriminants for serialization, covering bitwise ops, shifts, bit counting and
casts. The interpreter implements them in `interp/src/intrinsics.rs`; the
compiled backends emit instructions directly.

## Salsa Patterns

See [salsa-patterns.md](salsa-patterns.md) for how the four salsa kinds are
used here, why expression tables are keyed on `ExprKey` rather than salsa ids,
and how to measure whether a change memoizes.

### Database

The compiler's database is in `datalove-datafun-compiler/src/lib.rs`:

```rust
#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}
```

Implements `DbClone` - shares `Arc<Zalsa>` global state, clones thread-local state.
`Database::recording(recorder)` returns one that reports every query it runs;
clones report to the same recorder, so work farmed out to rayon is recorded too.

`bcts`, `datalove-datalit` and the `datalove` facade each declare their own
plain salsa `Database` as well. Compiler work uses the datafun one, re-exported
as `datalove_datafun::Database`.

### Tracked Functions

There are 127 of them, 43 of which keep a memo for a module compile. A table
here went stale within a session, twice -- it listed fifteen and was six
queries and one signature out of date when that was noticed -- so the
inventory lives in [salsa-architecture.md](salsa-architecture.md), grouped by
granularity, with a command that regenerates it rather than a list to trust.

The thing to take from it: a query's **granularity** -- one memo, or one per
module -- says what it is allowed to depend on. A graph-keyed query may look at
each module and must not look at each function; a per-module query may look at
its own module and must not look at the rest of the world. `reachable_func_ids`
exists to make the second true of the two lowering queries.
`compile_scaling_tests` holds both.

Graph-level functions aggregate per-module results and hold handles rather than
copies. Each is still keyed on the whole graph, so an edit to one module
re-runs every one of those walks; the walks are cheap because what they call is
memoized, and they are what stands between here and pulling per module.

### Incremental Compilation

Key principle: `Module` objects are created once and reused. Updates use `set_source()`.

`IncrementalModuleWorld`:
- Stores `Module` objects in `BTreeMap<String, Module>`
- `add_module()` creates new module once
- `update_source()` preserves identity
- Graph rebuilds reuse existing modules

Memoization behavior:
- Whitespace-only changes: re-parses, but the `parse_module_ast` projection
  returns an equal value and backdates, so name resolution and typechecking do
  not re-run (see the firewall section of salsa-patterns.md)
- AST changes (same types): re-typechecks changed module only
- Type changes: re-typechecks dependents

`ModuleId` is `#[salsa::interned]`, so the same path gives the same id. It was
an input once, when every `::new()` made a distinct one, which is why the world
holds `Source` handles across an edit rather than rebuilding from paths -- a
`Source` is the input, and it is the only handle worth keeping.

### Reusing a Compiled World

**The unit of reuse is the pipeline, not the database.** A
`ModuleCompilationPipeline` owns the `Source` inputs every tracked query is
keyed on, and `descriptor.to_pipeline(db)` makes new ones, so a fresh pipeline
misses every memo however warm the database is. Measured in debug on `sys/std`:
a fresh database and pipeline, the same database with a fresh pipeline, and a
cloned database with a fresh pipeline all took about 756ms; the same pipeline
compiling a second time took 84ms (and less since).

`CompiledWorld` (`datafun/src/pipeline/compiled_world.rs`) is that pair kept
together: a `Database` and the pipeline whose inputs live in it. It is not a
cache, just the state a caller holds if it wants salsa to do its job -- owned,
no thread-local, dropped with its owner. `std_all_tests` holds one per worker,
and the REPL's `Engine` keeps its pipeline across a reset for the same reason.

**Threads.** `Storage::clone` keeps the `Arc<Zalsa>` -- every memo, interned
value and tracked struct -- and makes a fresh `ZalsaLocal`, the per-thread
query stack. So `Database` is `Send` and not `Sync` by design, and a clone is
how another thread gets a handle onto the same work; the handle must be
per-thread, what it points at need not be. Note that a clone alone buys nothing
cold, since the cost is in the inputs. Compilation is therefore single-threaded
per handle, while execution is free: `IrCodeUnit` and
`Arc<ModuleFunctionRegistry>` are `Send + Sync` and carry nothing of salsa, so
any number of executors, each with its own runtime, can run the same IR in
parallel.

**Two sources, so two worlds.** There are two standard libraries and a process
can want both: `WorkspaceDescriptor::load_sys_dir` reads `sys/` off disk, which
is what the stdlib suites use because they test it, and
`from_system_library` uses the copy embedded in the binary, which is what the
CLI and REPL use. They are not interchangeable (`embedded_matches_tree` exists
for that reason), so whatever holds a world holds one per descriptor.

Persisting a compiled world across processes was sized and not attempted.
Salsa's `persistence` feature would need about 132 items across ten crates
annotated and made serializable, and can fail late on one unserializable
field; compiling several scripts against one in-process `CompiledWorld` is the
baseline it would have to beat.

### Parallel Execution

Enabled via `DATALOVE_PARALLEL=1`:

```rust
pub fn parse_module_graph_parallel<'db>(db: &'db dyn DbClone, ...) -> ParsedModuleGraph<'db> {
    // Prepare cloned databases
    let work: Vec<_> = modules.iter()
        .map(|m| (db.dyn_clone(), m))
        .collect();

    // Warm cache in parallel
    work.into_par_iter().for_each(|(db_clone, module)| {
        let _ = parse_module_full(db_clone.as_salsa_db(), module);
    });

    // Tracked function hits cache
    parse_module_graph(db.as_salsa_db(), ...)
}
```

Works because `dyn_clone()` shares the global memoization cache.

Four of the five phases do it this way: warm, then let the sequential tracked
aggregator read the memos back. Phase 5a is the exception and worth knowing
about, because it is what the others would look like if they could. Its output
-- `ModuleLoweredFunctions` and the deferred names -- carries no `'db` brand, so
a worker hands back what it built rather than warming a cache for a second pass.
The others return branded values, which cannot escape the borrow of the clone
that produced them; that is a lifetime problem rather than a salsa one, and
salsa 0.28 supports genuinely concurrent queries (see its `tests/parallel`).

What caps all of this is the sequential half. Each phase's aggregator walks the
graph whatever was edited, so the speedup is bounded by what it does: parse sits
near 2.7x on sixteen cores and cannot go past it while the aggregator rebuilds.
Thinning `ModuleGraphTypecheckResult` took typecheck from 1.57x to 1.78x for
exactly that reason -- the restructure is what makes the parallelism worth
having, not the other way round.

`just test-parallel` runs the suite under `DATALOVE_PARALLEL=1`. CI does not.

## Ownership Analysis

Runs after typechecking, produces `DropSchedule` consumed by lowering. The
types it produces live in `datalove-datafun-sema` so lowering can depend on
them without depending on the analysis.

### Tracking Categories

```rust
pub enum TrackingCategory {
    Copy,    // No tracking/drops needed
    Precise, // State statically known
    Tracked, // May vary at runtime (needs tracking byte)
}
```

Tracked bindings: exports, `out` params, conditional moves, mutable slots.

Match arms use `ScopeKind::MatchArm`. Branch consistency (D008) is generalized
across all match arms: if a value is moved in one arm, it must be moved in all.
`if` and `match` both check it in `check_branches_agree`, over the branches that
fall through. Each branch's `BranchEnd` keeps its `moved_at` and `assigned_at`
sites, so the error can point at the move or `set` that made the branch differ,
and, for a `set`, at the move before the branches that it undoes.

**Consts are borrowed.** A const, function-level, module-level or script-level,
and a const parameter are ref bindings: a move out of one of a non-copy type is
`CannotMoveConst` (D003) unless written with `@`. A const the analysis declared
is a binding with `is_const`. One declared outside the body is only a name, and
`OuterConsts` says which: in a function body every name it cannot resolve,
since a body sees only its parameters, what it binds, consts and functions, and
a function is never named as a value; in a script or expression unit the
earlier units' consts, from `external_consts_over`, because an earlier unit's
`let` is copied out of rather than borrowed. A const argument is not a move:
the analysis reads `call_targets` to find the callee's const parameters and
skips those arguments, and lowering hands the call a copy of its own
(`lower_comptime_arg`), since the call consumes what it is given.

Because nothing consumes a const, every one of a non-copy type is built once
and borrowed through a `StaticRef`. A module or script const named from a
function is lowered that way directly. A function-local const, and a
specialized copy's const parameter, is lowered as a value defined by a `Const`
and promoted afterwards by `promote_function_consts`; see
[Consts Built Once](plan-static-consts.md#user-content-as-built).

### Auto-adapt

`AutoAdaptMode::Enabled` accepts the errors an `@` would have fixed by
supplying the `@` the source left out. Both analyses record where it belongs
as `AdaptSites`, keyed by `ExprKey`, and lowering emits what an explicit `@` on
that expression would - a widening, a clone for linear types.

| Code | What `@` fixes | Auto-adapt |
|------|----------------|------------|
| F016 | Widening along a signedness chain, `f32` to `f64`, cross-sign widening (`u8` to `i16` and up), a clone where the types already match, an atom or term into its enum -- whatever `can_clone_coerce_to` accepts | Yes |
| D001, D002 | A clone at the earlier move | Yes |
| D007 | A clone at the use inside the loop, so each iteration takes a copy | Yes |
| D013 | A clone where an earlier script unit gave the value away | No: that unit has already run |
| D003 moving out of a const | A clone at the use | Yes |
| D003 moving a parameter, D004 | Nothing; the parameter mode is wrong | No |
| D005, D006 | Nothing; there is no value to clone | No |
| F011 | Nothing; `@` needs an expected type, so `let x = v@` cannot synthesize one | No |

Arithmetic on fixed integers producing `int` where a fixed type was wanted is
deliberately not on the list: the result really is an `int`, and the fix is
checked arithmetic.

Ownership analysis records the *earlier* use, not the one that would have
errored: a value read after a move is already gone, so the repair belongs
where it was given away. The REPL avoids D013 differently, by copying out of
earlier units in the first place (see [Ownership across
units](#user-content-ownership-across-units)). Sites are keyed by expression,
so handing a body a set naming expressions from elsewhere is harmless - no
expression there matches.

The mode is set on a `ScriptCompiler` with `set_auto_adapt_mode`, which also
rebuilds its `ScriptEnv`, since the mode is part of what the per-unit queries
are keyed on. What is not done:

- **Modules.** `compile_modules` passes `AutoAdaptMode::Disabled` to both
  typechecking and ownership analysis, and `ModuleCompilationPipeline` has no
  way to ask for anything else, so the module fixtures record that their
  modules did not compile.
- **`EnabledWithReport`** behaves exactly like `Enabled`; the report is a TODO
  in the typechecker's `context.rs`.
- **No driver exposes it.** The `--auto-adapt` CLI flags and
  `DATALOVE_AUTO_ADAPT` variable once proposed were never implemented.

`auto_adapt_tests` (in `datalove-tests`, fixtures under
`tests/fixtures/auto-adapt/`) runs each worldfile with the mode off and on and
then *executes* what the mode accepted, recording the computed values. A check
that the errors went away says nothing about whether the adapted program is
the one meant, and running it is what showed the mode had once inserted
nothing at all.

### Error Codes

Variants of `AnalysisError` in `datalove-datafun-sema`.

| Code | Variant | Trigger |
|------|---------|---------|
| D001 | `UseAfterMove` | Using value after move |
| D002 | `DoubleMove` | Moving value twice |
| D003 | `CannotMoveBorrowed` | Moving `ref`/`mut`/`out` param |
| D003 | `CannotMoveConst` | Moving out of a const or const parameter without `@` |
| D004 | `CannotMutFromRef` | Passing `ref` to `mut` param |
| D005 | `ReadUninitialized` | Reading an `out` param or `var` binding before it is set |
| D006 | `OutParamNotInitialized` | Return without initializing `out` |
| D007 | `MoveInLoop` | Moving outer-scoped value in loop |
| D008 | `InconsistentBranchMove` | Value moved, or `set` again after a move, in one merging branch only |
| D009 | `OutParamPartialWrite` | Field write to `out` param |
| D010 | `AliasedMutableArgument` | Two arguments share a place root, one is `mut`/`out` |
| D011 | `CannotMutateImmutable` | `let` binding or `in` param passed as `mut`/`out` |
| D012 | `CannotMutateTemporary` | Non-place argument passed as `mut`/`out` |
| D013 | `UseAfterMoveInEarlierUnit` | Using a binding whose own script unit gave its value away |

D001, D002, D007 and D013 carry an `OwnershipRecoveryHint` suggesting an `@`;
auto-adapt can apply the first three, not D013. The `D0xx` codes in `datalove-datalit`'s parser are a
separate namespace and unrelated.

### Drop Schedule

`DropSchedule` tells lowering where to emit `Drop` instructions:

```rust
pub struct DropSchedule {
    pub then_branch_exit: BTreeMap<usize, Vec<BindingId>>,
    pub else_branch_exit: BTreeMap<usize, Vec<BindingId>>,
    pub before_return: BTreeMap<usize, Vec<BindingId>>,
    pub before_try_return: BTreeMap<usize, Vec<BindingId>>,
    pub before_set_target_early_return: BTreeMap<usize, Vec<BindingId>>,
    pub loop_body_end: BTreeMap<usize, Vec<BindingId>>,
    pub before_break: BTreeMap<usize, Vec<BindingId>>,
    pub before_continue: BTreeMap<usize, Vec<BindingId>>,
    pub match_arm_exit: BTreeMap<(usize, usize), Vec<BindingId>>,
    pub stmt_order: Vec<StmtKey>,
}
```

`before_set_target_early_return` is computed before RHS moves are analyzed, so
it includes the RHS binding, which is still live when a set-index bounds check
fails.

### AOT Tracking Bytes

Out params get tracking bytes in the frame:

```rust
let param_tracking_base = tracking_offset + tracked_values.len() + tracked_slots.len();
for (i, &pid) in tracked_params.iter().enumerate() {
    params[pid.0 as usize].tracking_byte = Some(param_tracking_base + i as u32);
}
```

Values: `UNINIT = 0x00`, `LIVE = 0x01`, `MOVED = 0x02`

## Execution Backends

All of them consume `IrCodeUnit` and agree with `ir::layout`.

- **Interpreter** (`datalove-datafun-interp`) walks the IR directly.
  `CallDispatcher` is the extension point: it can intercept a call and hand it
  to a JIT or a dynamic inliner, or return `NotHandled` to fall through.
  Native rider calls are checked before the dispatcher.
- **CTFE** (`interp/src/ctfe.rs`) is the interpreter used at compile time.
  `InterpCtfeEvaluator::with_module_registry` gives const expressions access to
  cross-module function calls.
- **JIT** (`datalove-datafun-cranelift-jit`) tiers by call count.
  `OptimizingDispatcher` tracks call sites, picks the best available IR
  (inlined if one exists), executes native code when compiled, and otherwise
  counts toward the threshold. `DispatcherMode::Tuned` uses thresholds;
  `Chaos` makes seeded pseudo-random decisions for testing.
- **Cranelift AOT** (`datalove-datafun-cranelift-aot`) compiles ahead of time.
- **C AOT** (`datalove-datafun-c-aot`) emits C11 in one pass: type descriptors,
  then functions (module functions prefixed `__mod_N_`), then
  `__script_body(void* rt)` and a `main()` that initializes the runtime. One
  file per module plus one for the script, so a C compiler produces the
  executable in a single step and there is no unlinked halfway point;
  `pipeline::c_aot` drives it and `aot-compile --c` reaches it.

  Its function ABI is its own: `RET f(rt, [sret,] p0..pn, d0..dk)`, where the
  descriptors are the ones `descriptor_params` names. It differs from
  cranelift's in returning small scalars by value where cranelift always uses
  sret, which is allowed because no program mixes the two -- the one ABI they
  must agree on is the runtime's, and that one is fixed. A rider is called
  through it the same way every backend does: `(ptr, tydesc)` per argument, the
  result through an out pair, and a status back.
- **Inlining** (`datalove-datafun-inline`) transforms IR under
  `InlineDirective`s, and also drives the interpreter's dynamic inliner.

The runtime's C API (`datalove-rt/src/c.rs`) is entirely `dtlv_rti_*`: calls
only the compiler emits, which may use whatever ABI is convenient. The crate
docs also describe a `dtlv_rt_*` family the language would call under a
restricted ABI, but none exist yet. Everything but `init` takes a runtime
handle, and every value pointer is followed by its tydesc.

### Leak Checking

The runtime allocator can record every live allocation, so that `shutdown`
reports what leaked and every `free` checks that its pointer is known (double
free, untracked pointer) and that its size, align and count match what was
allocated. `LeakCheckMode` (`datalove-rt/src/impls/alloc.rs`) is read from
`DATALOVE_LEAK_CHECK` -- `ignore`, `warn`, `panic`, `panic-backtrace` -- and
defaults to **`Ignore`**, which skips all of it. The justfile sets
`LEAK_CHECK := "panic"` on every recipe that runs tests, and children inherit
it, so it reaches the executables the AOT backends build and the `datalove`
binary the CLI tests spawn. A bare `cargo test` has it off.

It is one switch rather than two on purpose. The free-time checks read the
same allocation map the leak report enumerates, so they cannot be kept
without keeping the map, and the map is the whole cost: 13-17% of an
allocation-heavy program. Keeping the checks in release was tried and saved
nothing. Nor should a release pay it, because **datalove is safe**: a program
cannot reach `free` with the wrong size, or free a pointer twice, by being
written badly. Only the compiler can, by lowering a drop wrongly, and the test
suite is where that is caught. On the suite itself the tracking costs nothing
measurable.

Nothing fails if a recipe loses the variable; see [Nothing proves the suite
runs with leak checking
on](issues.md#user-content-nothing-proves-the-suite-runs-with-leak-checking-on).

## Performance Notes

Measured facts worth having before optimizing anything. Release builds; the
numbers drift, the proportions less so. Open work from these measurements is in
[issues.md](issues.md).

**The release profile is `opt-level = 2`.** Against `3` it measured the same
on the interpreter and the JIT, 4% slower on `typecheck-std`, a smaller binary,
and an 18% faster clean build and 22% faster incremental one. Against `"s"`,
which it replaced, it is 7-10% faster interpreting and 14% faster compiling
`sys/`.

**Startup is the standard library.** A process starts in about 3ms and a trivial
`script --no-sys` finishes in under 4ms; the same script with `sys/` is 55-60ms.
Narrowing to [roots](#user-content-compiling-from-roots) is what takes a script
that uses little of the library well under that. The largest real program in the
tree, `botdocs/learn.dfs`, adds under 10ms on top of the floor.

**Compiling is allocation-bound.** There is no hot spot -- the largest single
symbol in a profile of compiling `sys/` is about 2% -- and about 40% of the time is
malloc, `memcpy`, `hashbrown` and the kernel faulting memory back in. mimalloc as
the CLI's global allocator measured 1.33x on startup, with system time down 2.9x,
and was not adopted; there is no `#[global_allocator]` in the tree. The runtime's
own allocator is not part of that 40%: it mmaps its payloads directly.

**Front end.** Two lessons from profiling the lexer and parser recur. A
`#[salsa::tracked]` field read looks like a field access and is a table lookup, so
read fields once and carry the slices -- the lexer's `peek` and the bracer's
iterator both went through salsa per character or per token. And alternate the
variants within one run and compare medians; a block of A then a block of B
measures the machine. Two attempts that were reverted: an ASCII fast path for
classifying whitespace measured as nothing and would have been wrong, since
`char::is_ascii_whitespace` rejects `\x0B` where `char::is_whitespace` accepts it;
and a byte cursor for the lexer measured 1.48x on lexing alone and was not judged
worth keeping. Carrying a newline flag on `TokenKind::Whitespace`, so
`split_lines` need not resolve every whitespace token through salsa, is worth
perhaps 1% and is open.

**The JIT** is about 85x on a tight arithmetic loop and 2-3.5x on call-heavy code.
It costs a fixed ~14ms of startup (`JitEngine::new`: arena, ISA, registering every
runtime symbol) and compiles synchronously at `opt_level = "speed"` in the dispatch
that crossed the threshold. It counts calls only, so a loop in a function called
once is never compiled. Compiled code calls each callee through a stub with the
callee's signature, which calls the callee's code directly once it is compiled
and goes through `__jit_dispatch_call` until then; going through the dispatcher
every time had been over 80% of the time on recursive `fib`, 5x slower. About a third of stdlib-shaped execution is in the native
runtime, which the JIT cannot speed up, so it is near its ceiling of about 1.8x
there; arithmetic-shaped code is over 90% interpreter, which is where the 85x
comes from. The interpreter's call path takes frames off a `FrameStack`
(`interp/src/frame.rs`) and layouts from a `LayoutCache` (`interp/src/layout.rs`);
reusing frames and layouts made it 1.5-1.8x faster on call-heavy code and turned
inlining under the interpreter from a regression into roughly break-even.

**The interpreter's dispatch is mostly call overhead.** `execute_instruction` is
one `match` over every instruction, and its prologue and epilogue -- six saved
registers and a frame sized for the largest arm -- were a third of its time,
paid once per IR instruction. `execute_hot` handles the dozen instructions most
time goes to (constants, copies, arithmetic, slot loads and stores, intrinsics,
option and result wrapping) inlined into the block loop, and everything else
falls through. Moving an arm there is the lever: it took benchvs primes from
3.6s to 2.0s. The other lesson is that `#[inline]` is not enough for the
accessors that loop calls -- `read_operand`, `Frame::value` and kin stayed out
of line in a caller that size until they were `#[inline(always)]`, and
`read_operand` keeps only the three common operand kinds inline for the same
reason. A copy of a runtime-sized value is a call to `memcpy` unless the size
is matched to a constant first (`copy_bytes`). Together these took primes from
3.6s to 1.6s, sum from 135ms to 87ms and fib from 1.32s to 1.0s.

**The interpreter's frames are compiled code's frames.** `IrLayout` takes its
offsets from `ir::frame_layout::FrameLayout`, tracking bytes included, so a value,
slot or tracking byte is where compiled code for the same body keeps it -- the
precondition for entering compiled code from the middle of an interpreted loop.
What only the interpreter keeps comes after that layout's bytes, in an
extension compiled code never sees: the parameters as a `[Value]`, the shape
descriptors, a descriptor word for each reference `resolve_ref_descriptors`
names, and the liveness bytes where they are kept. A frame is its bytes, and
`Frame` is a pointer to them and the layout.
Liveness works as it does in compiled code: a tracked slot or `out` parameter
says from its tracking byte, and everything else is taken to hold something --
either it is precise, or, like an unwritten `var n: u32` passed `out`, its type
owns nothing and destroying it does nothing, which is why the analysis left it
untracked. The per-binding
flags the interpreter used to keep for everything (liveness bytes, now in the
frame's extension) are kept only for script frames, which need them for REPL error recovery and for
later units' reads, and for function frames in debug builds, where they check
the precise answers and the reads -- so the suite still panics on reading an
empty slot, and a release build does not pay for the check. On speed it is
about even: fib and primes 3% faster, sum 6% slower. Sum's loop runs a tracked
drop and a tracked store every iteration, and a tracking-byte check is a longer
chain of dependent loads (frame, layout, tracking table, byte) than the flag it
replaced: fewer instructions, lower IPC.

**Three tiers of instruction.** `execute_hot` (inlined into the block loop: the
dozen instructions arithmetic spends its time in), `execute_warm` (not inlined
but small: the instructions generic and library code is full of -- erase and
reify, parameter stores, list bounds and element references, drops, clones)
and `execute_instruction` (everything else, calls included). Entering
`execute_instruction` costs a frame sized for its biggest arm, so an
instruction that runs often belongs in one of the other two.

**Frames come off a stack.** Function frames are bumped onto a `FrameStack`, a
list of chunks that never move, each frame preceded by a header saying where
the top was and holding its layout. Before that a `FramePool` handed out boxed
`Frame` structs with vectors for the parameters, shapes and reference
descriptors; before the boxing it moved a 192-byte struct by value, two
`memcpy` calls per call, and boxing took recursive fib from 848ms to 655ms.
Script frames are `ScriptFrame`s, which own their bytes and live in the
`FrameStore`.

**Bytecode calls do not recurse.** A call from one bytecode body to another
pushes the callee's frame and carries on in the same loop (`run_body` in
`bytecode.rs`), saving the caller's registers in an `Activation`; a return
restores them. A call site that resolves to a statically typed bytecode body
keeps a `Plan` -- the callee's layout, bytecode and final parameter
descriptors -- so that the call is a check, a push, the arguments and a
switch. Two things learned doing it: `pc += 1` in a release build with
overflow checks costs the loop its registers (the panic path is enough), so
the loop uses `wrapping_add`; and not recursing saves little by itself -- the
saving is in what a loop that owns its frames can skip.

**A call resolves its arguments into the callee's frame.** `execute_call_site`
pushes the callee's frame (`FrameStack::push`), sets each argument straight
into it, offers the dispatcher
the frame's parameters as they stand, and only if it declines makes the rest of
the frame ready (`Frame::enter`). The parameters are one `Value` apiece so that
they are the slice a dispatcher is handed, and until `enter` they carry the
caller's descriptors, which is what a dispatcher has always been shown; `enter`
gives owned ones the callee's. What every call needs to know about a parameter
-- its mode, and whether the argument is moved, which meant walking its type --
is in the cached `IrLayout`. Before this a call built an argument `Vec`,
nulled three parameter vectors only to overwrite them, cloned a list of shapes
nothing read, and asked each parameter's mode three times; fib went from 0.98s
to 0.83s. Natives have no frame and still take a list, as does the public
`call_in_context`.

## Entry Points

### Module Compilation

**Analysis only** (`compile.rs`):

```rust
pub fn compile_modules<'db>(
    db: &'db dyn DbClone,
    input: ModuleCompilationInput<'db>,
    rider_sources: Vec<(String, String)>,
    mode: ParallelMode,
) -> ModuleCompilationOutput<'db>
```

Returns after phase 4. Check `is_successful()` before lowering.

**With lowering** (`tracked_lower.rs`):

```rust
pub fn lower_module_graph_with_evaluator<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
    mode: ParallelMode,
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
    skip_const_inlining: bool,
    skip_specialization: bool,
) -> ModuleGraphLoweringResult<'db>
```

### High-Level API

```rust
let mut pipeline = ModuleCompilationPipeline::default();
pipeline.add_module(&db, "local", "pkg", "main", source);
let compiled = pipeline.compile_fresh(&db);

// Incremental. Returns the compiled modules and the reborrowed database.
pipeline.update_source(&mut db, "local", "pkg", "main", new_source);
let (compiled, db) = pipeline.compile(&mut db);
```

`ModuleCompilationPipeline::from_sections(&db, &sections, CompilerOptions::default())`
builds one from parsed worldfile sections, which is how most tests construct it.
`compile_fresh_with_mode` and `compile_with_mode` take an explicit
`ParallelMode` instead of reading `DATALOVE_PARALLEL`.

A pipeline's `CompilerOptions` are settled when it is built. Nothing recompiles
under options it was not made with, so there are no setters for them, and a
`WorkspaceDelta` that changes them says `requires_new_pipeline` rather than
being applied to one that exists.

### Compiling From Roots

The world is every module a workspace holds, and a program using the system
library would otherwise compile two dozen modules it may touch none of.
`Roots` (`datafun/src/incremental.rs`) says which modules to start from:

```rust
pub enum Roots {
    All,                    // every module in the world
    From(BTreeSet<String>), // these, and what they reach by transitive `require`
}
```

It is a **parameter of the graph build**, not a mode: `IncrementalModuleWorld::build_graph`
takes it, and package resolution walks from it too, so modules nothing reaches
are not even parsed. `Roots::All` is bit-for-bit the compile there was before the
choice existed, so the two are one path with a different argument. Because
`ModuleGraph` is interned, the roots are part of the graph's identity, and
changing them on a live pipeline (`set_roots`) is a different graph rather than a
memo to invalidate; `roots_tests` holds that. On `sys/` plus one module, a
script requiring nothing went from about 43ms to under 7ms end to end.

**It changes what is an error.** A type error in a module nothing requires is not
reported. So the line is that compiling from roots is for running a program, not
for vouching for a world, and `All` stays the default:

- `script`, `script-ir`, `aot-compile` and `script-world` narrow, through
  `ModuleCompilationPipeline::narrow_roots_to_script`, which reads the script's
  `require`s.
- `typecheck-std`, the test suites that build their own pipelines, and the REPL
  pass `All`.

A worldfile's own modules are always roots, whether the script reaches them or
not: a module the author wrote in the file being compiled is part of what they
asked to be compiled, and a library module they do not use is not.
`narrow_roots_to_script` refuses to narrow -- leaving `All` -- when a `require`
does not resolve, because pruning on a program whose requires are wrong would
drop the module the diagnostic is about.

The REPL stays at `All` because narrowing it looks like a net loss. Its module
set is fixed for the session, compiled once at startup, and each line compiles
against it. Narrowing would save that startup compile once and then re-lower
everything reachable on every line that adds a `require`, since modules are
numbered by their place in the graph and an inserted module renumbers what
follows it. See issues.md for the numbering.

### Workspaces

`WorkspaceDescriptor` is an immutable snapshot of everything compilation reads:
system library (absent under `--no-sys`), user libraries in shadowing order,
`CompilerOptions`, and a `work_dir` the compiler may write to. It is cheap to
clone and two of them can be diffed into a `WorkspaceDelta` for incremental
recompilation. It is pure data; drivers (CLI, REPL, LSP) build one and feed it
to `ModuleCompilationPipeline`.

Two constructors supply the system library, and neither goes looking for it:

```rust
// A driver that carries its own library, which is every shipped command.
let sys = datalove_sys_packages::system_library();
let descriptor = WorkspaceDescriptor::from_system_library(&sys);

// A test compiling the tree it lives in.
let descriptor = block_on(WorkspaceDescriptor::load_sys_dir(repo_root.join("sys")))?;
```

`from_package_world` and `from_worldfile_sections` build the other kinds.

`work_dir` is output, not input, so it takes no part in `diff`. Two workspaces
compiled concurrently must not share one or their native components overwrite
each other. Only workspaces that build riders from source need one; a
descriptor built from a `SystemLibrary` has no rider crate directories and
never writes anything.

**Scripts are compiled against a workspace, not as part of it.** A descriptor
describes the module world a script can `require` and `import` from; the script
itself -- a `.dfs` file, a REPL line -- goes to a `ScriptCompiler` made from the
compiled modules, which keeps its own incremental state. A driver holds a
descriptor and its script sessions separately. Nor is the execution mode in the
descriptor, or whether riders are built as a dylib or a staticlib; those are
decided by the driver that consumes it.

`WorkspaceDelta::apply_to_pipeline` applies module additions, removals and
changes to a live pipeline, and refuses a delta that `requires_new_pipeline`
(compiler options, or any rider change). No driver consumes deltas yet; see
issues.md.

### Script Compilation

```rust
let mut script_compiler = compiled.script_compiler_default(db).expect("modules compiled");
let result = script_compiler.compile_fragment(source);
// result.typecheck, result.ownership, result.lowering
```

`script_compiler_default` uses the interpreter as the CTFE evaluator, wired to
the module registry so const expressions can call across modules;
`script_compiler(db, evaluator)` takes a different one. Both return `None` if
module compilation had errors.

`ScriptCompilationResult` has separate results for partial compilation
(typecheck can succeed while lowering fails). `compile_expr` handles a single
expression unit, `compile_fragment` a statement unit.

## Test Patterns

### Exampletest Framework

```rust
ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
    .fixture_subdir("ir_lower")
    .file_extension("world")
    .allow_errors(true)
    .run();
```

- `BLESS=1 cargo test` updates expected output. Unset `RUST_BACKTRACE` first.
- Filter: `cargo test -- filter_name`
- `just test` runs the whole suite; `just test-64` uses 64-bit collection
  indexes, `just test-parallel` sets `DATALOVE_PARALLEL=1`, `just test-slow`
  the `slow_tests` features.

`ExampleTestRunner::with_worker_context(dir, init, analyzer)` hands each
fixture a context built by `init`, for state that is expensive and cannot be
shared between threads -- `std_all_tests` keeps a `CompiledWorld` there (see
[Reusing a Compiled World](#user-content-reusing-a-compiled-world)). The runner
keeps contexts in a pool, borrowing one per fixture and handing it back,
rather than using rayon's `map_init`: `map_init` runs its initializer once per
*work split*, not once per thread, and built 80 worlds for 143 fixtures. One
context per `par_chunks` chunk builds the fewest but gives up per-fixture work
stealing, so slow chunks straggle. The pool never holds more contexts than
threads running at once.

### Test Suites

Most live in `crates/datalove-datafun/tests`.

| Suite | Purpose |
|-------|---------|
| `parser_tests` | Parse and emit AST + diagnostics (in the compiler crate) |
| `tycheck_tests`, `tycheck_world_tests` | Typecheck results |
| `auto_adapt_tests` | `@`-recoverable errors under `AutoAdaptMode::Enabled`, then run (in `datalove-tests`) |
| `ir_lower_tests` | IR lowering from worldfiles |
| `ir_lower_script_tests` | Script IR lowering |
| `ir_inline_tests` | Inlining transformations |
| `ir_serial_tests` | IR serialization round-trip |
| `interp_tests`, `module_interp_tests` | Interpreter execution |
| `interp_jit_tests`, `interp_dispatch_tuned_tests`, `interp_dispatch_chaos_tests` | JIT tiering and dispatch |
| `interp_specialize_tests`, `interp_constlet_tests` | Specialization and const bindings |
| `aot_tests`, `aot_layout_tests` | Cranelift AOT compilation and layout compatibility |
| `dual_tests`, `c_dual_tests` | Compare interp vs Cranelift AOT, and vs C AOT. Both are behind `slow_tests`, so `just test` does not run them |
| `layout_conformance_tests` | `ir::layout` against `rtdt::layout`, both directions |
| `native_rider_tests` | End-to-end native rider calls |
| `std_tests`, `std_all_tests` | The `sys/std` library, compiled from `sys/` on disk. `std_all_tests` runs every fixture through all four backends and requires agreement, and is the only suite that puts a rider call, a bigint or a type parameter through the C backend |
| `embedded_matches_tree` | The embedded stdlib against `sys/`, and every declared native linked (in `datalove-sys-packages`) |
| `module_memo_tests`, `incremental_memo_tests`, `no_op_recompile_tests`, `parse_firewall_tests` | Salsa memoization behavior |
| `incremental_lowering_tests` | That an edit lowers the module that changed and an unchanged recompile runs no query at all. Asks `QueryRecorder` what salsa ran, which the `module_memo` fixtures' thread-local log cannot see across rayon |
| `database_memory_tests` | Database growth |
| `worldgen_tests`, `coverage_tests` (in `datalove-worldgen`), `worldgen_dual_tests` (in `datalove-tests`) | Generated worldfiles typecheck, cover the AST, and run the same both ways. See [Worldgen Coverage](#user-content-worldgen-coverage) |

### index-64

The `index-64` feature makes `index` and `offset` 64 bits wide instead of 32.
`datalove-rtdt` switches `IndexRepr`/`OffsetRepr` between `u32`/`i32` and
`u64`/`i64`, and `INDEX_SIZE` and `INDEX_ALIGN` between 4 and 8, which changes
the layout of every collection, string and bigint header. The Cranelift
backends take their width from `INDEX_TYPE` and `INDEX_BITS` in
`datafun-cranelift/src/index_types.rs`. It is compile-time only, so a binary
is one width or the other; `ABI_VERSION` covers it, so a rider built at the
other width is refused.

`just test-64` runs the suite with it. Where a fixture's output differs by
width, `.out.expected.64` takes precedence over `.out.expected` when the
feature is on. Prefer fixtures that do not need one: ask for the edge rather
than naming it -- `bits()` rather than `32`, `max_value()` rather than
`4294967295`, comparing against an extreme rather than printing it.
`std_tests/106_index_math_parity`, `107_index_edges` and `108_offset_edges` are
written that way and share one expected file.

`sys/std/index.dfm` and `offset.dfm` derive their width the same way:
`const BITS: u32 = icall index_bits()`, a nullary intrinsic reporting the
configured width, with the extremes computed from it. They are `const`
bindings, so CTFE folds them before any backend sees them. Both modules used
to hardcode 32-bit constants, which under `index-64` made `max_value()` and
`bits()` disagree with the arithmetic around them.

### Worldgen Coverage

`datalove-worldgen` generates random worldfiles that typecheck.
`coverage_tests.rs` asks whether they cover the language, and takes its
checklist from the compiler rather than from a list: every variant of
`Statement`, `ExprFunKind`, `BinOp`, `UnaryOp`, `ParamMode` and datalit's
`TypeHint`, rolled and matched by one macro with an exhaustive match, so a new
variant is a compile error until someone says whether the generator writes it.
Generated worldfiles are parsed with the real parser and walked -- counting with
regular expressions over the text was tried first and was wrong in both
directions. Anything never reached must be named in `NOT_YET_GENERATED` with a
reason, and anything named there that *is* reached fails too, so the list
cannot rot either way. `ALL_SHAPES` is a hand-written list for forms that are
one variant with a field present or absent (`loop` and `loop while`, `if` with
and without `else`), and can go stale.

What it does not tell you:

- **Generated is not exercised.** `test_1000_seeds_typecheck` only typechecks,
  and is `#[ignore]`d. Only `worldgen_dual_tests` runs ownership analysis and
  executes anything, and it skips unless `WORLDGEN_DUAL_TEST=1`
  (`WORLDGEN_DUAL_SEED` reproduces a run).
- **It counts single node kinds, not combinations.** Every bug the generator
  has found was at an intersection -- a generic over a map at `data`, a tensor
  of a tuple holding a heap value -- and a roll of node kinds calls all of
  those covered.
- **Rarity is ambiguous.** Statement generators that cannot proceed fall back
  to `gen_let` silently, so a construct can be rare because it keeps failing to
  build rather than because it was weighted that way.

### Worldfile Format

```datalove
----------
rider testlib
----------
native fun int_add(a: i32, b: i32): i32

----------
module local/pkg/main
----------
require rider testlib
import testlib.int_add

fun main(): i32
    ret int_add(3, 4)
end fun
```

Section headers: `module <lib>/<pkg>/<mod>`, `module-add`, `module-remove`,
`module-change-ws`, `module-change-ast`, `module-change-ty`,
`scriptunit-fragment`, `scriptunit-expr`, `inline-directives`, `rider <name>`.
The three `module-change-*` kinds drive the memoization tests.

## Key Files

| File | Contents |
|------|----------|
| `compiler/src/lib.rs` | `Database`, `DbClone` impl, module exports |
| `compiler/src/compile.rs` | `compile_modules()`, `ModuleCompilationOutput` |
| `compiler/src/module_graph.rs` | Parsing pipeline, rider interface construction |
| `compiler/src/tracked_lower.rs` | `lower_module_graph_with_evaluator()`, `compute_func_id_map()`, three-phase lowering |
| `compiler/src/tracked_ownership_analysis.rs` | Salsa-tracked ownership analysis |
| `compiler/src/tracked_script_lower.rs` | Script lowering, `collect_const_graph()` |
| `compiler/src/tracked_script_ownership.rs` | Script ownership analysis |
| `compiler/src/specialize.rs` | Const parameter specialization: `collect_instantiations_into()`, `monomorphize_function()`, `rewrite_comptime_calls()`, `specialize_script_unit()` |
| `sema/src/lib.rs` | `AnalysisError`, `DropSchedule`, `TrackingCategory`, `ExprTypes` |
| `ownership/src/lib.rs` | Drop/ownership analysis |
| `lower/src/func.rs` | `lower_function_for_module()` |
| `lower/src/script.rs` | Script unit lowering |
| `lower/src/const_expr.rs` | `lower_const_binding()`, `try_extract_literal()` |
| `const/src/eval.rs` | `evaluate_prepared_const()` |
| `const/src/inline.rs` | `inline_script_consts()`, `inline_module_functions()` |
| `ir/src/lib.rs` | `IrCodeUnit`, `CodeRef`, `IrType`, `Instruction` |
| `ir/src/layout.rs` | The layout authority for `IrType` |
| `ir/src/frame_layout.rs` | Frame layout shared by every backend, the interpreter's `IrLayout` included |
| `ir/src/registry.rs` | `ModuleFunctionRegistry`, `UnitFunctionRegistry` |
| `datafun/src/pipeline/mod.rs` | `ModuleCompilationPipeline` re-exports |
| `datafun/src/pipeline/module_pipeline.rs` | The pipeline itself, `add_native_rider_units()` |
| `datafun/src/pipeline/script_compiler.rs` | Script compilation pipeline |
| `datafun/src/pipeline/workspace.rs` | `WorkspaceDescriptor`, `WorkspaceDelta` |
| `datafun/src/pipeline/rider_build.rs` | Native component synthesis and cargo build, for riders found on disk |
| `datafun/src/pipeline/rider_load.rs` | `register_linked_natives`, `load_rider_library` |
| `cli/src/main.rs` | `register_natives()`, which wires both the interpreter table and the JIT |
| `sys/build.rs` | Embeds `sys/` module sources and names each rider's crate and `INTERFACE` |
| `sys/src/lib.rs` | `system_library()` |
| `datafun/src/pipeline/compiled_world.rs` | `CompiledWorld` |
| `datafun/src/incremental.rs` | `IncrementalModuleWorld`, `Roots` |
| `buildinfo/build.rs` | Decides `BuildInfo` |
| `rt/src/impls/alloc.rs` | The runtime allocator, `LeakCheckMode` |
| `bcts/src/parser_util.rs` | Token gluing, `eat_number` |
| `sys/std/rider/build.rs` | Generates `symbols()` from `rider.dli` |
| `interp/src/native.rs` | `NativeFunctionTable`, the C ABI bridge (`call_c`) |
| `interp/src/dispatch.rs` | `CallDispatcher`, `DispatchResult` |
| `cranelift-jit/src/optimizing.rs` | Tiering and dynamic inlining dispatcher |
