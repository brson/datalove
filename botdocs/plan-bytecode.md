# A Register Bytecode for the Interpreter

A plan for a second execution engine inside the interpreter: function bodies
lowered once from IR into a flat, fully specialized register bytecode over the
frame bytes, run by a tight dispatch loop, with the IR walker kept as the
fallback and the reference.

The short version: keep the IR exactly as it is and add an interpreter-private
lowering beside `IrLayout`. Resolve at lowering time everything the IR walker
re-derives on every execution -- operand offsets, types, sizes, payload offsets,
descriptors, tracking bytes, callee identities -- so that a handler is a few
loads, an operation and a store. Build it as a prototype function by function,
falling back to the IR walker for any body it cannot lower, and check it against
the IR walker on every fixture from the first day.

> **This is a plan, and a prototype of it.** The prototype is in
> `crates/datalove-datafun-interp/src/bytecode.rs`, off by default and turned on
> with `DATALOVE_INTERP=bc`; [What the prototype found](#user-content-what-the-prototype-found)
> says how it went. The rest records the reasoning and the facts it rests on as
> of October 2026.

## Contents

- [Why build it](#user-content-why-build-it)
- [Where the time goes today](#user-content-where-the-time-goes-today)
- [What already exists to build on](#user-content-what-already-exists-to-build-on)
- [The design](#user-content-the-design)
  - [Scope](#user-content-scope)
  - [The lowering](#user-content-the-lowering)
  - [Operands and addressing](#user-content-operands-and-addressing)
  - [Descriptors](#user-content-descriptors)
  - [The instruction set](#user-content-the-instruction-set)
  - [Control flow](#user-content-control-flow)
  - [Liveness and tracking bytes](#user-content-liveness-and-tracking-bytes)
  - [Calls](#user-content-calls)
  - [The dispatch loop](#user-content-the-dispatch-loop)
  - [Integration and fallback](#user-content-integration-and-fallback)
  - [Caching and invalidation](#user-content-caching-and-invalidation)
- [Testing](#user-content-testing)
- [Measuring](#user-content-measuring)
- [Order of work](#user-content-order-of-work)
- [What the prototype found](#user-content-what-the-prototype-found)
- [Risks and open questions](#user-content-risks-and-open-questions)
- [Things the survey turned up](#user-content-things-the-survey-turned-up)
- [Prior art](#user-content-prior-art)

## Why build it

On native targets the Cranelift JIT is the answer to execution speed: it compiles
quickly and runs the benchvs kernels 3-20x faster than the interpreter (85x on a
tight arithmetic loop). A bytecode will not catch it. Where an interpreter's
speed still matters:

- **Targets with no JIT.** A platform that forbids writable executable memory
  (iOS, some consoles, hardened sandboxes), an architecture Cranelift does not
  support, and a browser. Note that **nothing on the interpreter's path is built
  for wasm today**: the only wasm32 target is the `datalove` database crate, and
  `datalove-datafun-interp` has never been compiled for it. A browser playground
  would be the motivating case, but it is a future one.
- **Code that never gets hot.** The JIT counts calls and compiles at a threshold,
  costs about 14ms to start, and never sees a loop in a function called once.
  REPL lines, script bodies and functions called a few times are interpreted
  whatever happens. A fast tier 0 under the JIT is the conventional design.
- **Compile-time evaluation.** CTFE runs the interpreter during compilation. It
  is not performance-sensitive today -- const units are tiny and compiling is
  allocation-bound -- but its speed is compile speed.
- **Proving the architecture.** The shared frame layout, the per-body layout
  cache and the dispatcher boundary were designed so that execution engines can
  slot in beside one another without touching the IR or each other. A third
  engine that fits where the design says it should is the test of that claim.

And what it is expected to buy: the IR walker costs about 10ns per IR instruction
on `benchvs/primes`. A register bytecode with resolved operands and typed opcodes
typically runs at 1-3ns per instruction, so a rough 3-5x on arithmetic and
call-heavy code. Library-heavy code (`benchvs/wordfreq`) will gain much less: its
time is in the runtime's strings, maps and allocation, which every engine shares.

For calibration, CPython 3.14's bytecode interpreter -- with its JIT off, as it is
on this machine -- runs recursive `fib(32)` in 177ms against about 800ms for the
IR walker, and `primes` in 1.16s against 1.49s.

## Where the time goes today

Everything below is decided again on every execution of an instruction, though
it depends only on the instruction and its body:

- **Operand resolution.** An operand is a `ValueId`, `SlotId` or `ParamId`.
  Reading one goes through the frame's `Rc<IrLayout>` to `value_offsets[id]` and
  `value_tydescs[id]` (two bounds-checked lookups behind a pointer chase) and
  produces a `Value { ptr, tydesc }`.
- **Type dispatch.** `execute_binop` matches on `(*lhs.tydesc).type_tag` to pick
  among sixteen type arms; `BinOpChecked`, `UnaryOp`, `Widen`, `WidenFixed` and
  `Pack` do the same.
- **Sizes and layouts.** `copy_value` and `move_value` read the size from the
  descriptor (and switch on it to avoid `memcpy`); option and result wrapping and
  unwrapping call `compute_option_layout` / `compute_result_layout`; `Pack`,
  `GetField`, `SetField` and enum operations look up field and variant offsets in
  the descriptor.
- **Constants.** `write_const` matches on the `ConstValue` tree and, for a string
  or a big integer, calls the runtime to build it on every execution.
- **Dispatch shape.** `execute_hot` handles thirteen instructions inline; the rest
  pay `execute_instruction`'s prologue, which was a third of its time.
- **Calls.** Every call resolves its callee by reference (`ExecutionContext::get_unit`,
  a linear search for local functions), looks up the layout in a hash map, and,
  for a native, looks the symbol up by string in a `HashMap<String, _>`.
- **Liveness.** Release function frames now keep no per-binding flags, but tracked
  slots still find their tracking byte through `layout.slot_tracking` -- the chain
  that made `benchvs/sum` 6% slower when frames moved to the shared layout.

None of this is wasted in the sense of being wrong; it is the price of walking a
data structure designed for analysis rather than execution.

## What already exists to build on

- **The frame.** Interpreted frames are laid out by `ir::frame_layout::FrameLayout`,
  as compiled code's are: parameter pointers, then values, then slots, then
  tracking bytes. A bytecode addresses the same bytes, so frames, the pool,
  `Frame::enter` and the dispatcher's view of a call do not change.
- **`IrLayout` and `LayoutCache`.** A per-body structure computed once and keyed by
  `FuncIdentity`. The bytecode for a body is the same kind of thing and belongs
  next to it.
- **The call path.** `execute_call_site` resolves arguments straight into the
  callee frame, offers the dispatcher the frame's parameters, and only then calls
  `enter`. Per-parameter facts (`param_modes`, `param_moves`) are already in the
  layout.
- **Static layout facts.** `ir::layout` (`option_payload_offset`,
  `result_payload_offset`, `aggregate_field_offsets`, `enum_payload_offset`,
  `layout_of`) and `ir::resolve_ref_descriptors` (which values' descriptors only
  arrive at run time) are exactly what the lowering needs, and Cranelift already
  uses them.
- **The runtime ABI.** Every runtime call the IR walker makes is a
  `dtlv_rti_*(ptr, tydesc, ...)` call that Cranelift makes too. The bytecode makes
  the same calls with operands it resolved in advance.

## The design

### Scope

**Function bodies first, script units never (at first).**

- A script unit runs once, so lowering it buys nothing.
- Script units are where the awkward cases live: `ExternalValue`, `ExternalSlot`
  and `SlotDest::External` (reads and writes of earlier units' frames),
  `UnitEnd`, `UnitEarlyReturn`, unit-end bindings, and full liveness flags for
  REPL error recovery. Function bodies never contain external operands; Cranelift
  refuses them for the same reason and compiles functions regardless.
- CTFE runs script units too, so it stays on the IR walker. Functions it calls
  could use bytecode, but see [caching](#user-content-caching-and-invalidation)
  for why CTFE should bypass the cache at first.

### The lowering

A new module in `datalove-datafun-interp` (say `bytecode/`), lowering one
`IrCodeUnit` with its `IrLayout` into a `BcFunction`:

```rust
struct BcFunction {
    ops: Vec<Op>,               // the code, flat
    consts: Vec<u8>,            // pre-built constant bytes, aligned
    descs: Vec<*const TyDesc>,  // static descriptors this body names
    calls: Vec<CallSite>,       // resolved callees and argument plans
    // ... side tables for anything too big to sit in an Op
}
```

It runs once per body, lazily, when the body is first called -- the same moment
`LayoutCache::get_or_compute` builds the layout -- and is cached with it. The
lowering is a single pass over the blocks with the IR's types (`value_types`,
`slot_types`, the function context) and the layout in hand. It may decline: any
instruction it does not support yet makes it return `None`, and the body runs on
the IR walker (see [fallback](#user-content-integration-and-fallback)).

### Operands and addressing

An operand becomes a frame offset, resolved at lowering. Two addressing modes
cover every operand a function body has:

- **Direct**: the value is in the frame at `off`. Values and slots.
- **Indirect**: the frame holds a pointer at `off`, and the value is where it
  points. `ValueRef` (a value that stores a pointer) and parameters.

For parameters, the bytecode engine stores each parameter's pointer into the
frame's parameter region -- the space `FrameLayout` reserves and the IR walker
leaves unused -- in `Frame::enter`. A parameter is then an indirect operand like
any other. (The `params: Vec<Value>` that dispatchers see stays as it is; this is
a copy of its pointers into the frame.)

Encoding: a `u32` with the top bit as the mode. A handler that reads an operand
checks the bit, which predicts well because most operands are direct. Where a hot
op's operands are always direct (arithmetic on SSA values, for example), the
lowering can emit a variant that does not check. Start with the bit; split
opcodes only where a profile says the branch costs.

### Descriptors

A descriptor is needed by every op that calls the runtime (destroy, clone, big
integer arithmetic, collections, erase and reify, debuglog) and by nothing else.
Where it comes from:

- **Static**: the value's type is known, so the descriptor is the
  `IrTyDescTable`'s pointer for it, resolved at lowering into the body's `descs`
  table (or carried in the op).
- **Dynamic**: the value's descriptor only arrives at run time. Four sources:
  1. a `ref` or `mut` parameter, which keeps the caller's descriptor
     (`frame.params[i].tydesc`);
  2. a reference whose projection recorded an override (`set_value_tydesc`, from
     `GetFieldRef`, `ListElementRef`, `MapValueRef`, `TensorIndexRef`,
     `DataBorrow`);
  3. a shape descriptor handed over by the caller (`DescriptorRef::Own(i)`);
  4. anything typed `data`.

  `ir::resolve_ref_descriptors(func)` already says which values are which.

So a descriptor operand is a small sum: `Static(index)`, `Param(i)`,
`Override(value)` or `Shape(i)`. Outside a generic every one is `Static`, and a
lowering that sees only `Static` can emit ops that take the pointer directly.

### The instruction set

Ops are a Rust `enum` with `u32` operands, aiming at 16 bytes per op (anything
bigger goes in a side table). Families, with the IR instructions each replaces:

**Constants.** `ConstU8/U16/U32/U64 { dst, imm }`, `ConstBytes { dst, pool_off,
size }` for anything with a fixed byte image (floats, tuples of scalars, options
of scalars). `ConstHeap { dst, const_index }` for a string, big integer,
collection, `data` or `error`, which still has to call the runtime to build a
fresh owned value each time -- but from a pre-converted form rather than by
walking a `ConstValue`.

**Arithmetic, per type.** `AddCheckedU32 { dst, a, b, overflow }`,
`SubCheckedI64`, ..., `LtU32 { dst, a, b }`, `EqF64`, `BitAndU64`, `ShlU32`,
`NotBool`, `NegF32`, ... -- one op per (operation, type) the type checker admits,
generated by a macro the way `impl_int_binop` generates the IR walker's arms.
`WidenFixed` becomes `ZextU8U32`-style conversions. `Intrinsic` is already typed;
it becomes `Intrinsic { id, dst, args }` with offsets resolved, or, for the hot
few (`rem_u32`, the wrapping arithmetic), dedicated ops.

**Arithmetic through the runtime.** Big integer operations, `Widen` to `int`,
equality on strings and aggregates, and everything on `data`
(`dtlv_rti_dyn_binop` and friends) become ops that call the same runtime function
with resolved operands and descriptors: `IntAdd { dst, a, b }`,
`DynBinOp { code, dst, a, b, descs }`, `EqRt { dst, a, b, desc }`.

**Movement.** `Copy4 { dst, src }`, `Copy8`, `Copy16`, `CopyN { dst, src, size }`
(and `Move` is the same bytes). Block parameter passing lowers to these.

**Options, results, enums.** `WrapSome { dst, src, payload_off, size }`,
`WrapNone { dst }`, `UnwrapOption { dst, is_some, src, payload_off, size }`, and
the result and enum equivalents, with offsets from `ir::layout` and sizes from
the static types. Tags are constants of the op (`OptionTag::Some` is 2, `None` 1;
`ResultTag::Ok` 1, `Err` 2; an enum discriminant is a `u32` at offset 0).

**Tuples and structs.** `Pack` becomes a sequence of copies to resolved field
offsets; `GetField` of a statically-typed field becomes a copy from base plus
offset (or a clone, if the field owns memory -- matching what the IR walker's
`dtlv_rti_field_read_local` does); `SetField` resolves its whole field path to
one offset at lowering. In a generic, where the base's descriptor is dynamic, the
runtime-call forms stay (`field_read`, `field_offset`).

**Slots, parameters, references.** Stores and loads with the tracking decision
baked in (see [liveness](#user-content-liveness-and-tracking-bytes)):
`StoreSlot { off, src, size }`, `StoreSlotTracked { off, src, size, track, desc }`
(destroy the old value if its byte says LIVE, store, set LIVE), `LoadSlotMoveTracked`,
`StoreParamTracked`, `RefStore`, and the field-path forms.

**Drops.** `Drop { src, desc }` (call `dtlv_rti_any_destroy_local`) and
`DropTracked { off, track, desc }`. A drop of a type that owns nothing is
deleted at lowering, since destroying it does nothing.

**Collections.** `ListGet`, `ListSet`, `ListBoundsCheck`, `ListElementRef`, the
map operations, tensor indexing and the `*New` constructors become ops with
resolved operands, element descriptors and strides, still calling the runtime
for anything that allocates or clones. Indexing a list of scalars can read the
element inline, as Cranelift does (`load(list + 0) + i * size`).

**Calls, debug.** See [calls](#user-content-calls). `DebugLog { src, desc }`.

### Control flow

The blocks flatten into one `ops` array; a block's id becomes its first op's
index, and jumps carry indices.

- **Block parameters** become moves at each jump site, emitted by the lowering.
  This is the moment to fix the IR walker's latent hazard: `pass_block_args`
  moves arguments one at a time, so a jump whose arguments read a parameter of
  the target block it has already overwritten would see the new value. The
  lowering should sequence the moves as a proper parallel move (detect cycles,
  break them through a scratch slot).
- **`Branch`** becomes `BrIf { cond, then, else }`, or one of a small set of fused
  compare-and-branch ops (`BrLtU32 { a, b, then, else }`) for the comparisons a
  loop header makes. Fusion is an optimization for after the first measurement.
- **Checked arithmetic** can jump straight to its overflow path:
  `AddCheckedU32 { dst, a, b, on_overflow }`, which removes the overflow flag
  write, the branch op and its read. The overflow flag value only exists for that
  branch in practice; the lowering should confirm that before fusing.
- **`Switch`** becomes a jump table indexed by discriminant (the IR walker
  searches its case list linearly).
- **`Return`** copies the result to the return destination by size and ends the
  loop.

### Liveness and tracking bytes

The bytecode follows the rule the frames now follow:

- A tracked slot or `out` parameter has a tracking byte, whose offset the
  lowering knows. Ops that touch one read and write the byte directly at
  `frame + off` -- no layout lookup, which removes the chain of loads that costs
  `sum` today.
- Everything else is taken to hold something, so ops that touch it do no
  bookkeeping at all.

Debug builds keep the IR walker's checks: the bytecode engine calls the frame's
`mark_*` hooks under `cfg(debug_assertions)`, so the suite still panics on a read
of an empty slot or a disagreement between a tracking byte and its flag. A
release build pays nothing.

### Calls

A call op names an entry in the body's `calls` table, resolved at lowering:

- the callee's `FuncIdentity`, and a cache slot for its `BcFunction` and
  `IrLayout` once they exist, so that a call does not search for the callee or
  look up its layout in a hash map after the first time;
- the argument plan: for each argument, its operand and mode (from the callee's
  layout), so that resolving arguments is a loop over precomputed entries;
- the shape descriptors, as descriptor operands;
- the destination offset.

The call itself keeps every behaviour of `execute_call_site`: take a frame,
resolve arguments into it, mark moved arguments dropped, offer the dispatcher the
frame's parameters, and only if it declines, `enter` and run the callee -- as
bytecode if it has a `BcFunction`, otherwise on the IR walker. A native callee's
function is resolved to the `NativeFnImpl` itself at lowering (or on first call),
removing the per-call string lookup in `NativeFunctionTable`.

The recursion stays on the Rust stack, as it is today. A bytecode loop could
instead push frames onto an explicit stack and loop without recursing, which is
faster for call-heavy code and makes a step or depth limit cheap (see
[issues.md](issues.md), "Compile-time evaluation has no limits"). It is a second
step: it changes how the JIT trampoline and the dispatcher re-enter the
interpreter.

### The dispatch loop

```rust
let mut pc = 0;
loop {
    match code[pc] {
        Op::AddCheckedU32 { dst, a, b, on_overflow } => { ...; pc += 1; }
        Op::BrIf { cond, then, els } => { pc = if ... { then } else { els }; }
        Op::Return { src, size } => { ...; return Ok(()); }
        ...
    }
}
```

A `match` in a loop compiles to a jump table. Rust has no computed goto, so this
is the starting point; the alternative is closure compilation -- each op a boxed
closure that captured its operands, called in turn -- which often wins in Rust
interpreters at the cost of debuggability. Same lowering either way; measure the
`match` first.

What the IR walker learned about dispatch carries over: handlers must be inlined
into the loop (`#[inline(always)]` on helpers, since LLVM declines to inline into
a function this large), helpers must not copy runtime-sized values through
`memcpy`, and rare paths belong out of line.

### Integration and fallback

- **Where the engine is chosen**: in `execute_call_site`, after the dispatcher
  declines. If the callee body has a `BcFunction`, run it; otherwise run the IR
  walker. Every other entry point (`call_in_context*`, the JIT trampoline's
  callback) goes through the same choice.
- **Per-function fallback**: the lowering declines a body it cannot lower, and
  the body runs on the IR walker forever after (cached as declined). This lets
  the bytecode cover instructions incrementally and lets the prototype run real
  programs from the first day.
- **A switch**: an environment variable or interpreter option selects
  `ir` (bytecode off), `bc` (bytecode where possible, the eventual default) and
  `bc-strict` (panic on any declined body, for coverage work).
- **Coverage counters**: which bodies were lowered, which declined and on which
  instruction, printed on request. The prototype's progress is this number.

### Caching and invalidation

The bytecode is cached per body, with the layout. The layout cache is keyed by
`FuncIdentity` and notices a replaced body only by a change in value and slot
counts. That is weaker than it looks, and a bytecode cache must not inherit it.
Ways a body changes under one identity:

| Event | What changes |
|---|---|
| The dynamic inliner replaces a caller | A new body; counts usually grow, but slots need not |
| REPL `reexecute_unit` | New bodies under the same `FuncIdentity::Unit` |
| REPL truncation, then new units | Indices reused by different units |
| `set_module_registry` | New module bodies under the same `FuncIdentity::Module` |
| A unit that fails part way | Its index reused by the next unit |
| CTFE | Every evaluation is unit 0, with clones of different modules' functions under their module-local ids, in one long-lived interpreter |

The proposal: key the cache on body identity, not name. Module bodies are
`Arc<IrCodeUnit>` and inlined bodies `Rc<IrCodeUnit>`; unit functions live in
the registry's vectors. Either cache by the body's address with an invalidation
generation bumped by `replace_unit`, `truncate_units`, `set_module_registry` and
inliner insertion, or store the compiled form on the body itself. And bypass the
cache entirely in CTFE until that is settled. The layout cache has the same
exposure and should move with it.

Note that only test code enables the dynamic inliner (`OptimizingDispatcher`);
`datalove script` and the REPL never inline. The inliner is still in the
dispatcher suites, so the bytecode must handle a body being replaced.

## Testing

- **Differential from the first day.** Add bytecode variants of the interpreter
  fixture suites, the way `interp_jit_tests` runs the same fixtures under the JIT:
  `interp_tests`, `module_interp_tests`, `std_tests` and the dispatcher suites,
  each under `bc`. The IR walker is the reference.
- **`bc-strict` for coverage**: the same suites with fallback forbidden show what
  is left to lower.
- **Leak checking** stays on, as for every suite: destruction must be exact.
- **Debug liveness checks** run under the bytecode, so a lowering that gets
  ownership wrong panics in the suite rather than leaking or double-freeing.
- **Miri**: the interpreter suites run under Miri (`just test-miri-interp*`); the
  bytecode engine must stay clean there, which rules out casual pointer tricks in
  the dispatch loop.
- **Worldgen fuzzing**: the worldgen differential tests can run bytecode against
  the IR walker on random programs.

## Measuring

- **A dynamic instruction histogram first.** Count executed IR instructions by
  variant (and by operand type for the arithmetic ones) on benchvs, the std test
  corpus and `learn.dfs`, behind a feature flag. The static census of user code
  (constants, calls, drops, branches, slot stores and checked arithmetic lead) is
  not enough: which ops the lowering covers first, and which get fused, should
  follow execution counts.
- **benchvs** for wall time: `fib` and `primes` (where the gain should show),
  `sum` (tracking bytes and big integers), `wordfreq` (runtime-bound; the gain
  should be small, and its size is a check on the expectation).
- **Lowering cost**: time to lower the whole standard library, which must stay
  small next to compiling it.
- **Expected**: primes and fib 3-5x faster than the IR walker; sum perhaps 2x
  (big integer additions dominate); wordfreq 1.2-1.5x.

## Order of work

Each step leaves the tree working, because unlowered bodies fall back.

1. **Instrumentation.** The dynamic histogram, the engine switch (with only `ir`
   working) and the coverage counters.
2. **Skeleton.** `BcFunction`, the cache, the dispatch loop, the fallback path,
   and a lowering for scalar constants, typed arithmetic and comparisons, copies,
   intrinsics, untracked slot loads and stores, `Goto`/`Branch` with parallel
   moves, and `Return`. Calls go through the existing path. `primes` should lower
   completely at this point: measure it.
3. **Calls.** The call table, the argument plan, parameters in the frame's
   parameter region, bytecode-to-bytecode calls, cached natives. Measure `fib`.
4. **Options, results, enums, tuples, structs**, with offsets from `ir::layout`,
   and checked arithmetic jumping straight to its overflow path.
5. **Tracked bindings and drops**: tracked slot stores and loads, `out`
   parameters, references, field paths, the drop forms. Measure `sum`.
6. **Runtime-backed ops**: big integers, string and aggregate equality,
   collections, constants that allocate.
7. **Generics**: dynamic descriptor operands, `data` operations, erase and reify,
   shape descriptors. At this point `bc-strict` should pass the suites.
8. **Decide.** Compare against the expectations above, and choose among: keep the
   bytecode as the interpreter's default engine and the IR walker as the test
   reference; explore closure compilation or an explicit call stack; or stop.

Later, and only on evidence: quickening for generic bodies (specialize a `data`
operation on the descriptor it first sees, guarded by a pointer comparison),
superinstructions from the histogram, and on-stack replacement into the JIT from
a bytecode loop header, which the shared frame layout makes possible.

## What the prototype found

The prototype is `bytecode.rs` in the interpreter crate, run with
`DATALOVE_INTERP=bc` (and `DATALOVE_BC_STATS` for coverage, `DATALOVE_BC_DUMP`
to print each body's ops). `just test-bc` runs the interpreter-driven suites --
interpreter, module and std fixtures, both dispatcher suites, the JIT, the
cross-backend std suite and the CLI tests -- on it, against the IR walker's
expected output, with leak checking on. They pass.

**Fallback is per instruction, not per function.** The plan had a body the
lowering could not handle fall back to the IR walker whole. Because the two
engines share the frame byte for byte, an instruction with no op becomes an
`Ir` op instead, which runs that one instruction on the IR walker against the
same frame. Every body lowers from the start and coverage grows an op at a
time, which made the plan's `bc-strict` mode and declined-body cache
unnecessary. Block edges whose arguments need the IR walker's bookkeeping (a
parallel move, a slot or parameter argument) and returns of anything but a
value take the same route.

**What is lowered**: scalar constants, copies and moves of values and untracked
slots, checked add, sub and mul and comparisons for 32- and 64-bit integers
(and `index`, `offset`, `u8`, `bool` equality), the hot `u32` intrinsics,
option and result wrapping and unwrapping, tracked slot stores, loads and
drops (the tracking byte's offset baked in), drops of values, widening to
`int` and binary operations no typed op covers (big integers, floats, string
equality -- these call the IR walker's routines with operands resolved), string
constants, parameter stores, list bounds checks and element references, the
`data` operations of generic code (erase, reify, clone, fixed widening), moves
and drops of parameters, the wrapping `index` intrinsics, jumps, branches,
switches, returns, and calls. Operands are frame offsets, direct or through a pointer (parameters,
references); parameter pointers go in the frame's parameter region on `enter`.
Ops are a 24-byte enum dispatched by a `match`.

**Results** (release build, benchvs, medians):

| | IR walker | bytecode | |
|---|---|---|---|
| primes | 1604ms | 577ms | 2.8x |
| fib(32) | 661ms | 376ms | 1.75x |
| sum | 92ms | 56ms | 1.6x |
| wordfreq | 3824ms | 2430ms | 1.6x |

(The IR walker's own numbers fell during the work too, from shared changes:
fib was 848ms and wordfreq 3968ms before them.) Primes runs in half CPython's
time (1.16s with its JIT off); fib is about twice CPython's 177ms; wordfreq is
two and a half times CPython's 945ms, where it was four.

**What made the difference**, in order:

1. Typed ops over resolved offsets: primes 1.6s to 0.87s on their own.
2. Three standard lowering optimizations: a `Goto` into the next block is no
   op; scalar constants defined in a loop are written once, by a prologue that
   runs on entry (a value is defined once and owns its bytes, so this is safe
   unless something else writes it -- an `out` argument or a reference store --
   and hoisting constants outside loops only adds work to every call); and a
   comparison whose only use is the branch on it fuses into the branch. Primes
   to 0.63s.
3. A call op. First straight into the call path, skipping the IR walker's
   dispatch; then, for a call whose arguments are all SSA values, a fast path
   with the arguments' places and descriptors resolved and the callee's layout
   cached at the call site by body address. It takes the general path whenever
   a dispatcher is installed, so the JIT and the inliner see every call.
4. A wider fast call: any argument the general path would not have to record
   as consumed, checked against the callee's parameter modes on its first call
   and cached with its layout; arguments read as the IR walker reads them, with
   a borrowed `data` read through its wrapper and shape descriptors resolved,
   for generic code, and from resolved places for everything statically typed;
   natives included; a module callee cached at the call site against the
   registry it came from. This is what made library-heavy code faster: in
   wordfreq the calls taking the general path fell from 3.3 million to none.
5. An `execute_warm` tier, for both engines: the instructions common in generic
   and library code (erase and reify, parameter stores, list bounds checks and
   element references, drops, clones, fixed widening, ...) moved out of
   `execute_instruction` into a small function of their own, since entering
   the big one was most of what they cost.
6. **Boxing the pooled frames**, which helped both engines: `FramePool` moved a
   192-byte `Frame` by value into and out of the pool, two `memcpy` calls per
   call. Fib on the IR walker went from 848ms to 655ms with that alone, and on
   the bytecode from 622ms to 358ms.
7. Lowering what generic and library code does, which took wordfreq from
   2775ms to 2430ms and its instructions run on the IR walker from about
   twenty million to ten thousand. The key was that **only borrowed parameters
   and references have descriptors the layout does not know**: what a frame
   holds itself -- values, slots, parameters passed by value -- it holds
   erased, so a `data` in its static type is a `data` in its bytes, and the
   layout's descriptor is the truth. Ops over those are as static as any.
   Borrowed parameters and references inside a generic get their descriptor
   at run time (`Desc::Param`, `Desc::Ref`), for the ops that hand operands to
   IR walker routines. Two things the fast call learned on the way: a `data`
   handed to a borrowed parameter still has to be read through its wrapper, so
   it keeps off the resolved lane; and an argument moved into a call needs no
   record if it has no tracking byte, so a wrapper passing its own parameters
   on to a native (`list.push`, `map.insert`) suits the fast call too.

   The arms for the ops that call routines are out of the dispatch loop, in
   `run_routine_op`: inline, they made the loop's code worse enough to cost
   primes, which runs none of them, 7%. Out of line, about 2%.

**What it does not do**:

- **Debug liveness checks.** A frame the bytecode runs stops keeping liveness
  flags, since the bytecode keeps none for values and untracked slots. The IR
  walker remains the checked engine; the bytecode is checked against it by the
  differential suites.
- Collection construction, aggregates (pack, unpack, field projections),
  error construction and calls with `out` arguments are still IR walker
  instructions, reached through `execute_hot` and `execute_warm`. None is hot
  in the benchmarks.
- The call is still recursive on the Rust stack, and ops are 24 bytes rather
  than 16. Neither has been measured as worth changing yet.

**What is left** is the call itself. In fib about a quarter of the time is in
the fast call, a tenth in taking and entering the frame, the rest in the loop.
In wordfreq, which makes about twenty calls a word, most of them to small
library wrappers and natives, the fast call alone is a quarter of the time
(about 14ns a call), the loop a fifth, entering frames and calling natives
another tenth; the runtime (map comparisons and inserts, strings, allocation)
most of the rest. Cheaper calls, or inlining the wrappers when the body is
lowered, is what would move it next.

## Calls

Calls are now most of what the bytecode costs in library-heavy code. Wordfreq
makes about 44 million a run -- 20 million into bytecode bodies, 16 million
into natives, 12 million of the first being one-line wrappers that pass their
parameters on to a native (`string.push_str`, `list.len`, `map.get`, ...) --
and they are about half of its 37 billion instructions.

**Measured** with microbenchmarks in a loop of 20 million (counting
instructions with `perf stat`, since wall time on a shared machine is noisy,
and single-stepping one call in gdb to see where they go):

| | instructions | time |
|---|---|---|
| a loop iteration alone | ~125 | ~8ns |
| plus a call to a two-parameter bytecode function | +~440 | +~28ns |
| plus `list.len`: a generic wrapper and a native | +~930 | +~60ns |

One bytecode call, stepped through, is about 380 instructions: 90 in
`Frame::enter` (clearing tracking bytes, giving owned parameters the callee's
descriptors, writing the parameter pointers, clearing the reference
descriptors), 40 taking and returning the pooled frame (popping the box,
clearing its two vectors, swapping its layout `Rc`), 25 pushing the
parameters, 60 in the call function's own prologue, epilogue and lookups, and
about 100 in the callee's `run_bytecode` around its two ops. One native call
was 550, of which the native itself was a dozen: the bridge built its C
arguments in a `Vec`, an allocation per call, with an iterator chain (130, now
plain indexing on the stack, and 120 fewer); the native was found by hashing
its symbol (50); the arguments read through a closure and
`borrow_through_wrapper` (50); `fast_call` itself (120).

**Tried and not worth it alone**: a lane for the commonest call, statically
typed arguments into a bytecode body, with the body, layout and bytecode
cached at the call site so that it skips the lookups and `bytecode_for`. It
saved 60 instructions a call, 3% of wordfreq: the lookups are not where the
cost is. The cost is in what a call builds -- a `Frame` with its vectors and
`Rc`, the parameter `Value`s, a fresh `run_bytecode` activation -- not in
finding what to build.

**What would make a real difference**, roughly in order of payoff for effort:

1. **A frame that is its bytes.** Put everything a frame keeps beside its data
   into the data: each parameter's descriptor next to its pointer in the
   parameter region the layout already has, and a word per reference value for
   the descriptor a projection found (now `Frame::value_tydescs`). A frame is
   then a base pointer and a layout, frames for function bodies come off one
   contiguous stack by bumping a pointer, and `FramePool`, the boxed `Frame`,
   its vectors and the `Rc` go. Both engines get it. Script frames, which
   outlive their unit and keep liveness flags, stay as they are. A call should
   cost about what writing its arguments' pointers and clearing its tracking
   bytes costs.
2. **No recursion for bytecode-to-bytecode calls.** CPython 3.11 does this:
   a call pushes a frame and switches the loop's `ops`, `base` and `pc`; a
   return pops it and copies the result to where the caller asked. That
   removes the call function's and `run_bytecode`'s prologues and epilogues,
   about 160 instructions, and the Rust stack depth limit on recursion. It
   wants (1) first, since the loop cannot hold a `Frame` per activation.
3. **Calls through forwarding wrappers.** A body that is one call passing its
   parameters in order to a native, and returns what that returns, is
   recognized when the call site is first resolved, and the site calls the
   native directly: the arguments as the wrapper would have passed them (an
   owned one with the wrapper's descriptor, a borrowed one with the
   caller's), the result into the caller's destination with the wrapper's
   descriptor for it. 12 of wordfreq's 44 million calls go.
4. **A native call that is a function pointer.** Resolve the symbol to the
   rider's function pointer once per call site, keyed by body and a table
   generation, and build the C words straight from resolved places, rather
   than a boxed closure looked up by hashing its name, a `Value` slice, and a
   bridge that lays them out again.
5. **Inlining small leaf bodies** at lowering -- `next`, `u32.min`,
   `index.from_u32`: no calls, no ops that need the IR walker -- with their
   frame appended to the caller's and their parameters' pointers written by
   the caller. It wants a guard against the body being replaced, which (3)
   wants too: a module registry swap must invalidate bytecode lowered against
   the old one.

**Done since**: (4) and (3). A native is registered as the rider's function
pointer (`NativeTarget::C`, with Rust closures kept for tests) and called
through `native::call_c`; a fast call caches the target per call site, keyed
by the body and the table's generation, and writes the C words straight from
its arguments. Then a body that only forwards its parameters to a native is
recognized when a call site first resolves it (`forwarder`), and the site
calls the native itself, with the arguments as the forwarder's frame and call
would have read them. A loop calling `list.len` went from about 1050
instructions an iteration to 930 with the bridge's allocation gone, 800 with
the cached target and 450 with forwarding; wordfreq from 37.2 to 29.5 billion. (1) and (2) are designed, built and
measured in [plan-frame-stack.md](plan-frame-stack.md).

(1) and (2) are the structural change and most of the gain: fib, which is
nothing but calls, is where CPython is still twice as fast, and that is the
call. (3)-(5) are cheaper and independent.

## Risks and open questions

- **Two interpreters to maintain.** Every new IR instruction needs a bytecode
  lowering as well as an IR walker arm. The fallback makes this soft -- a new
  instruction can ship on the IR walker alone -- and the differential suites make
  any disagreement visible. If the bytecode becomes the default, the IR walker
  could shrink to a reference implementation used only in tests, or be removed.
- **Matching the IR walker's quirks.** The survey found places where the IR
  walker's ownership bookkeeping is uneven (see
  [below](#user-content-things-the-survey-turned-up)). The bytecode should match
  the IR walker exactly at first, so the differential tests mean something, and
  the quirks should be fixed in both, separately, with fixtures.
- **Descriptors in generics.** The dynamic-descriptor cases (borrowed parameters,
  projection overrides, shapes, `data`) are where the IR walker's subtleties
  accumulated (`botdocs/generics.md`). They come last in the order of work for
  that reason; until then generic bodies fall back.
- **Op size.** A 16-byte op is a target, not a given; ops with three operands
  and a jump target are already at it. Measure before packing harder.
- **Unsafe code.** Resolved offsets mean raw pointer arithmetic on the frame. The
  frame's size is known per body, so the lowering can check every offset it
  emits against it once, leaving the loop free of bounds checks without losing
  safety. Miri must agree.
- **CTFE and caching.** See [caching](#user-content-caching-and-invalidation): the
  existing layout cache is already exposed to identity reuse, and the bytecode
  cache must not be.

## Things the survey turned up

Found while surveying the interpreter for this plan, and dealt with before the
prototype so that it had one contract to follow.

Fixed:

- `Unpack`, `UnwrapOption`, `UnwrapResult`, `SlotLoadMove` and `Move` (from a
  local source) consumed their source without marking it dropped, and `ListSet`,
  `MapSetValue`, `MapUpsert` and `TensorSet` marked a consumed value only when it
  was an `Operand::Value`. Each now marks whatever it consumes, value, slot or
  parameter. None of it was reachable as a double free -- the lowering loads a
  slot into a fresh value (marking the slot moved) before any of these consume
  it -- so the effect was on the debug liveness checks and on what a second engine
  could rely on.
- `SetField` and `SetFieldTracked` did not mark their value consumed, and
  `SetFieldTracked` did not mark the slot live as the IR documents and the
  compiled backends do.
- `pass_block_args` moved block arguments one at a time, so a jump whose
  arguments read the target block's own parameters read values it had just
  overwritten. It is now a parallel move; `test_block_args_are_a_parallel_move`
  builds the case, which the lowering does not produce today.
- `GetField`'s IR doc said it consumes its source; it borrows it, since only a
  field of a copy type is read that way (F070).
- `just test-miri-interp-all` named two recipes that do not exist.

Left as they are:

- `temp_view_tensors` (rank > 1 `TensorIndexRef`) grows and is never cleared.
- A function frame that returns an error goes back to the pool without
  destroying its live values. Release function frames keep no liveness flags,
  so this would need the tracking the frame model gave up, or unwinding drops.
- The JIT trampoline's interpreted callbacks run with no dispatcher, because the
  dispatcher is taken out of the interpreter while it runs.

## Prior art

- **CPython 3.11+** (PEP 659): specializing adaptive interpreter, inline caches,
  quickening. Datalove needs the specialization but not the speculation, except in
  generic bodies.
- **Lua 5.x**: the classic register VM, whose argument for registers over a stack
  (fewer instructions, operands as frame slots) is the one made here.
- **WebAssembly interpreters (wasm3, Wasmi, the in-place interpreter of
  "A fast in-place interpreter for WebAssembly")**: a typed, statically-validated
  input lowered to an internal register form, often with fused operations -- the
  closest match to datalove's situation.
- **The JVM and V8's Ignition**: bytecode interpreters as tier 0 under an
  optimizing JIT, with the interpreter's frames designed for the JIT to enter --
  the role this would play beneath Cranelift.
