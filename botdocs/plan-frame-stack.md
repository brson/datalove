# A Frame Stack for the Interpreter

A plan for making interpreter calls cheap: frames that are only bytes, on one
stack of their own, and bytecode calls that do not recurse on the Rust stack.
These are items 1 and 2 of "Calls" in [plan-bytecode.md](plan-bytecode.md),
which has the measurements this starts from.

## Contents

- [Why](#user-content-why)
- [Where things stand](#user-content-where-things-stand)
- [Part 1: a frame is its bytes](#user-content-part-1-a-frame-is-its-bytes)
- [Part 2: bytecode calls without recursion](#user-content-part-2-bytecode-calls-without-recursion)
- [Part 3: unwinding](#user-content-part-3-unwinding)
- [Order of work](#user-content-order-of-work)
- [Open questions](#user-content-open-questions)

## Why

A bytecode call costs about 380 instructions, of which the callee's two ops are
a handful. Stepped through: 90 in `Frame::enter`, 40 taking and giving back the
pooled frame, 25 pushing parameters into a vector, 60 in `fast_call`'s own
prologue, epilogue and lookups, about 100 in the callee's `run_bytecode` around
its ops. Wordfreq makes 44 million calls; fib is nothing but calls, and is
where CPython 3.11, whose calls are a frame pushed on a contiguous stack inside
one loop, is still twice as fast.

The same two changes are what the interpreter needs to stop aborting on deep
recursion (see "Deep recursion aborts the process" in [issues.md](issues.md)):
frames that can be walked, and a recursion depth bounded by something the
interpreter checks rather than by the Rust stack.

The target is a bytecode-to-bytecode call of a few dozen instructions: bump a
pointer, write the arguments, clear the tracking bytes, switch the loop's
registers; and a return that copies the result and switches them back.

## Where things stand

A function frame today is a `Box<Frame>` from `FramePool`:

- `data`, the frame's bytes, laid out by the shared `FrameLayout`: a parameter
  region of one pointer per parameter, values, slots, tracking bytes. The
  Cranelift and C backends use the same offsets for values, slots and tracking
  bytes; they pass parameters as SSA values or C arguments and leave the
  parameter region unused.
- `layout: Rc<IrLayout>`.
- `params: Vec<Value>`: each parameter's pointer and descriptor, so that the
  arguments are a `&[Value]` to hand a dispatcher. `enter` gives owned (`in`,
  `out`) parameters the callee's descriptor and copies every pointer into the
  parameter region, where the bytecode reads them.
- `shape_descriptors: Vec<*const TyDesc>`, for the shapes a generic function
  declares, which no argument carries.
- `value_tydescs: Vec<*const TyDesc>`: what a reference value points at, where a
  projection inside a generic found out at run time; null where the layout's
  static descriptor is right. Cleared on entry.
- `is_script` and `liveness`: per-binding flags, kept for function frames only in
  debug builds, as a check on the ownership analysis.

Script frames are different and stay so: built by `Frame::new`, run only by the
IR walker, kept in the `FrameStore` for later units to read and move out of,
liveness always kept.

Things that constrain any redesign, from a survey of the crate:

- **Addresses are stable and are relied on.** A parameter is a pointer into the
  caller's frame (an owned `in` argument lives physically in the caller's frame
  while the callee owns it); the return destination is in the caller's frame;
  reference values hold raw pointers into other frames; `run_bytecode` keeps
  `base` across nested calls. A frame must never move while it is live.
- **Nothing is destroyed at frame exit.** The IR drops everything a function
  owns explicitly, parameters included, so a normal return leaves nothing
  behind. On error -- an `InterpError` propagated with `?` -- nothing is
  destroyed at all: owned values in every frame on the unwound chain leak.
- **The dispatcher interface takes slices of the callee frame.** `dispatch_call`
  gets `callee_frame.params()` and `shape_descriptors()` while `&mut
  IrInterpreter` is lent to it in the same context. That type-checks only
  because the callee frame is a local `Box`, not something the interpreter
  owns.
- **The JIT re-enters the interpreter** through `__jit_dispatch_call`, which
  builds a `Vec<Value>` and calls `call_in_context_with_shapes` with arguments
  pointing into JIT stack memory.
- **Layouts can be replaced while a frame runs on one.** The layout cache
  replaces an entry when the inliner adds values; today the running frame's
  `Rc<IrLayout>` keeps the old layout, and its bytecode, alive.

## Part 1: a frame is its bytes

Everything a function frame keeps goes into its bytes, frames come off one
stack, and `Frame` becomes a handle.

### The layout

The layout splits in two: a core every backend shares, and an extension that
is the interpreter's.

**The core** is `FrameLayout`, and holds what every backend uses: values,
slots, tracking bytes. The Cranelift and C backends read exactly these offsets,
`frame_size` and `frame_align`. The parameter region moves out of it -- no
compiled backend reads its offsets, parameters being SSA values or C arguments
there -- so compiled frames shrink by a pointer per parameter. Nothing in the
core may point into the extension, so that the core's offsets are the same in
every engine and every build.

**The extension** is laid out by `IrLayout` after the core's `frame_size`,
aligned, and the interpreter's frame is the two together. Its regions vary with
what only the interpreter cares about -- liveness bytes come and go with debug
builds -- without moving anything the compiled backends see.

- **Parameters are a pointer and a descriptor each**, 16 bytes, laid out as a
  `Value`. With `Value` `#[repr(C)]`, the parameter region *is* a `[Value]`,
  and `params()` is a slice of the frame. The bytecode keeps reading the
  pointer where it does now; `Desc::Param` reads the word after it.
- **A shape region**: a descriptor word per declared shape.
- **A descriptor word for each reference `resolve_ref_descriptors` names**,
  replacing `value_tydescs`. That analysis, in the IR crate, is what the
  Cranelift and C backends already use to decide which references need a
  descriptor at run time -- those rooted in a borrowed generic parameter or a
  `DataBorrow`, and projections further in from them -- and every other
  reference is described by its static type. The interpreter adopts the same
  answer instead of working out its own: `IrLayout` computes the set once and
  gives each such value a word; its projection, which always finds the
  descriptor at run time, always writes the word; and the lowering resolves
  every other reference to its layout's descriptor, so that it costs no word,
  no store and no test. Nothing is cleared on entry. The interpreter's own
  derivations -- the override vector, the bytecode's `Desc::Ref` -- go.
- **Liveness bytes**, a byte per value, slot and parameter, present where
  liveness is kept: always for script frames, in debug builds for function
  frames. The `Liveness` vectors go.

All four are the extension's; the core is unchanged but for losing the
parameter region.

### The stack

`FrameStack`, owned by the interpreter: a list of chunks that never move, each
allocated once and kept for reuse. A frame is bumped onto the current chunk, or
onto the next one if it does not fit; frames never straddle chunks. A pop puts
the top back where the frame's header says it was. Chunks rather than one
reserved region of virtual memory, because the interpreter is the engine for
targets without a JIT, wasm among them, where there is no reserving address
space; and because a chunk list grows without a limit fixed up front.

A frame on the stack is a small header and then its bytes:

- the top before it, for the pop;
- its layout, a strong reference held as a raw pointer, released on pop, so
  that a replaced layout outlives the frames still running on it;
- in part 2, the caller's registers.

The total size is capped, by a setting with a generous default; exceeding it is
an error, not an abort. That is the interpreter half of the stack-overflow
issue.

`FramePool`, `Box<Frame>` and the frame's vectors go. The frame's bytes are
reached through raw pointers into chunks the interpreter allocated, not through
a borrow of the interpreter, which is what lets a dispatcher be handed slices
of the callee frame alongside `&mut IrInterpreter`.

### `Frame` as a handle

`Frame` becomes `Copy`: a base pointer and a layout pointer. Its accessors keep
their names and signatures -- `value`, `slot`, `param`, `value_deref`,
`mark_*` -- and read the frame's bytes. Script frames keep owning their buffer
in the `FrameStore`, which hands out the same handle for them, so the IR
walker, the bytecode and everything that reads an external value see one kind
of frame.

### A call

The caller pushes the callee's frame and writes each argument straight into the
callee's parameter region -- pointer, and descriptor (the callee layout's own
for an owned parameter, the argument's for a borrowed one, which is what
`enter` does now after the fact) -- and the shape descriptors into the shape
region. Then the tracking bytes are cleared, and in debug builds the liveness
bytes set. The frame is entered.

The IR walker takes this too; it stays recursive (it is the checked engine,
and simple), but its calls get the cheaper frame.

## Part 2: bytecode calls without recursion

Today a bytecode call is `run_bytecode` → `fast_call` → `run_bytecode`: a fresh
Rust activation of the loop per datalove call, with its prologue, its
epilogue, its own copies of the loop's registers, and about 900 bytes of Rust
stack.

Instead one activation of `run_bytecode` runs every frame from the one it was
entered with to the deepest. The loop's state is a handful of registers: the
ops, `pc`, `base`, the body, its context. A call to a bytecode body:

1. pushes the callee's frame, with a header holding the caller's registers --
   ops, `pc` after the call, `base`, body, context, code reference -- and where
   the result goes;
2. writes the arguments into it, as in part 1;
3. sets the registers to the callee's and continues the loop at its entry.

A return copies the result to where the header says, pops the frame, restores
the caller's registers and continues; returning from the frame the activation
was entered with returns from `run_bytecode`.

What still nests on the Rust stack, by design:

- **Natives**, and forwarders to them (`fast_call`'s forwarding): leaves, so
  bounded.
- **Calls the bytecode hands to the general path** -- `Op::Call`, an instruction
  run on the IR walker that calls, any call made while a dispatcher is
  installed. These go through `execute_call_site` and `run_frame` and start a
  new activation of the loop on the same frame stack. Each activation's frames
  are a contiguous run of the stack; an activation returns only once its entry
  frame does.
- **JIT and interpreter interleaving**, which is native calls both ways.

So part 2 bounds the Rust stack for code that is bytecode end to end, and
leaves mixed chains to a depth guard at the crossings (`run_frame`,
`call_in_context_with_shapes`, the JIT's dispatch trampoline): a counter, or a
check of the stack pointer against a limit, that fails as an error.

Details:

- **Borrowed `data` arguments** are read through their wrapper, into scratch
  space when they have no address of their own (`BorrowScratch`, a vector of
  boxes today, an allocation per such argument). The scratch goes on the frame
  stack, between the caller's frame and the callee's, and goes with the callee.
- **Lifetimes the Rust frame held** move into the header: the layout reference
  (which keeps the bytecode alive), the call site's cached state. A module body
  stays valid while its registry is the one in use; the registry is swapped
  only between units.
- **Errors** propagate out of the loop as now, after popping every frame the
  activation pushed -- tearing each down, once part 3 exists.
- **Tail calls** become easy -- a call in tail position replaces the frame --
  though nothing asks for them yet.

## Part 3: unwinding

Not needed for the speed, but this is what turns "an error" into "a safe
teardown", and parts 1 and 2 are what make it possible: with frames that can
be walked, each saying where it is, an error can destroy what every frame
between it and the handler owns.

What a frame owns at a given point is fixed by where it is, for everything
untracked -- that is what "precise" means in the ownership analysis -- and by
the tracking bytes for the rest. So for each call site, the bindings live and
owned across it can be computed once, from the IR's own definitions and drops
(a forward analysis per body; the analysis that placed the drops knows it
already, and could hand it over). An unwind is then: for each frame from the
deepest up, look up its call site's list, destroy those, destroy the tracked
bindings whose bytes say live, pop. That fixes the leak on error that exists
today, and is what stack overflow, a native's error and, later, any trap need.

## Order of work

1. `Value` `#[repr(C)]`; the parameter region out of `FrameLayout`; `IrLayout`'s
   extension with 16-byte parameters, the shape region, reference descriptor
   words and liveness bytes; `Frame` reading all of them
   from its bytes, still in a `Box` from the pool. Every engine's tests pass
   unchanged. This is where the interpreter changes over to
   `resolve_ref_descriptors`.
2. `FrameStack`; `Frame` as a handle; the pool and the vectors go; the size cap.
   Callers write arguments into the callee's frame. Measure.
3. The bytecode loop without recursion. Measure.
4. A depth guard at the crossings; spawn `--jit` and the REPL worker with large
   stacks. With 2, deep recursion fails as an error.
5. Unwinding (part 3).

Each step is useful alone. Steps 1-3 are the speed; 2, 4 and 5 are the stack
overflow issue.

## Open questions

- **Copies of references.** `resolve_ref_descriptors` follows projections
  only. A reference copied, moved or passed as a block argument is described
  by its static type in every engine, the interpreter included (a copy does
  not carry `value_tydescs` either), so if the IR ever does that with a
  reference whose descriptor is found at run time, every engine reads it
  wrongly. Whether the IR produces such copies is not known; a fixture would
  say, and the fix, if one is needed, is in the analysis, where every engine
  gets it.
- **Chunk size and the cap.** A first chunk of 64 KiB, doubling, and a default
  cap in the hundreds of megabytes is a guess; the cap probably wants to be a
  per-interpreter setting so that CTFE can be strict and a script lenient.
- **Where unwinding information comes from**: recomputed from IR in the
  interpreter, or carried from the ownership analysis.
- **The JIT's frames.** They stay native, and a JIT-to-JIT recursion overflows
  the native stack long before the interpreter's cap. Compiled code checking a
  depth or the stack pointer in its prologue is the usual answer, and costs
  every compiled call a compare.
