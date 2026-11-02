# Values in Registers for the Cranelift Backend

Written October 2026, after profiling the AWFY benchmarks under the JIT.
Measurements are from `benchvs` at commit `80b5d68cd`. All three layers are
implemented; see [Where it stands](#user-content-where-it-stands).

## Contents

- [The problem](#user-content-the-problem)
- [What the frame layout requires](#user-content-what-the-frame-layout-requires)
- [Layer 1: small aggregates as words](#user-content-layer-1-small-aggregates-as-words)
- [Layer 2: a register convention between compiled functions](#user-content-layer-2-a-register-convention-between-compiled-functions)
- [Layer 3: slots](#user-content-layer-3-slots)
- [What exists, what is missing](#user-content-what-exists-what-is-missing)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)
- [Where it stands](#user-content-where-it-stands)

## The problem

The Cranelift backend keeps every value that is not a scalar in the frame,
at the offset the shared `FrameLayout` gives it, and passes every parameter
and every result through memory. The interpreter and compiled code agree on
that layout, which is what lets OSR enter compiled code in the middle of an
interpreted loop. But compiled code pays for the agreement on every
instruction that touches an aggregate, and on every call, including calls
that move nothing but a `u32`.

`fib(n: u32): !u32` stores `n` to the stack so it can pass its address, and
the callee loads it back. The callee writes its 24-byte `!u32` result through
`sret`, and the caller immediately loads the tag and the payload back:

```
mov    %eax,0x2c(%rsp)       ; n - 1, stored so it can be passed
lea    0x2c(%rsp),%rdx
lea    0x70(%rsp),%r13       ; the result's home
call   fib
movzbq 0x70(%rsp),%rsi       ; the tag, just stored by the callee
mov    0x8(%r13),%r12d       ; the payload, likewise
```

Each of those loads is served by a store the processor has not yet written
to the cache: a store-to-load forward, a few cycles on the critical path.
Towers moves a 24-byte disk out of a list through an option, a slot, a value,
an `ok` and the return slot, memory to memory each time, and makes about 43
such forwards per move (3.15G a run of the AOT build, against 34.6G instructions). That chain,
not the runtime, is what is left of Towers' cycles once the runtime's own
copies are inlined.

Therefore: keep values in registers wherever nobody else can see them, and
honour the frame layout only where somebody can. The frame layout does not
change. Every value keeps its home; compiled code just stops writing to it
when nothing will read it there.

## What the frame layout requires

The JIT does not speculate, so compiled code never hands a frame back to the
interpreter. OSR goes one way ([plan-jit-tuning.md](plan-jit-tuning.md)), and
nothing ever rebuilds an interpreter frame from compiled state. That leaves
three places where compiled code has to put a value where the layout says:

1. **OSR entry.** The entry block reads the header's block parameters, and the
   values defined before the loop, from their homes in the interpreter's
   frame. These are reads, once.
2. **Addresses another party can see.** References the interpreter made before
   an OSR entry point into its frame, so OSR code forms every frame address off
   the interpreter's frame. Anything passed by pointer to a call or the runtime,
   or reached through `ValueRef`, `GetFieldRef` or `ListElementRef`, needs its
   bytes in memory. Slots are like this almost by definition: `mut` arguments,
   field references and tracking bytes all address them.
3. **Calls between the tiers.** `bridge.rs` calls compiled code with a pointer
   per parameter, and the stubs call `__jit_dispatch_call` the same way. This
   is a calling convention, not the frame layout: the parameter region already
   moved out of the shared core into the interpreter's extension
   ([plan-frame-stack.md](plan-frame-stack.md)).

Everything else is free. A value nobody addresses can live in registers from
its definition to its last use, and its home goes unwritten.

## Layer 1: small aggregates as words

`CraneliftRepr` has two cases, `Scalar` and `Aggregate`. A third, words, holds
a value of a fixed type of up to four words as one Cranelift value per word,
each with its offset and type: an `?T` or `!T` of scalars, a small tuple or
struct, an enum with a small payload, a list's 16-byte header. The register
allocator does the rest.

Whether a value is held as words is decided from its uses, before compiling
the body. A value is held as words unless some use needs its address. The
uses that need none are the ones that move words around:

- `Copy`, `Move`, `Pack`, `Unpack`, `GetField`, `WrapSome`, `WrapOk`,
  `WrapErr`, `UnwrapOption`, `UnwrapResult`, `EnumVariant`,
  `EnumDiscriminant` and `EnumPayload`;
- stores of the value into a slot, a field, a parameter, a reference's target
  or a list element, and `ListNew`'s elements;
- reading a list's header for `ListGet`, `ListBoundsCheck` and
  `ListElementRef`;
- block arguments, and `Return`, which writes the words to `sret`.

Measured over the eleven `benchvs` programs, almost every small aggregate
qualifies. Per program, the small fixed aggregates fall into four groups:
words only; words and then a drop, which hands the value's address to the
runtime once at the end of its life; passed to a call; and needing an address
for anything else:

| program | words only | then dropped | call argument | other address |
|---|---|---|---|---|
| fib | 15 | 1 | 0 | 3 |
| storage | 27 | 2 | 2 | 0 |
| towers | 78 | 1 | 3 | 9 |
| json | 214 | 1 | 21 | 19 |
| ordered | 121 | 1 | 10 | 4 |
| wordfreq | 123 | 2 | 9 | 17 |
| nbody | 99 | 1 | 0 | 40 |

Nearly all of the last column is the message string of an error on a cold
path, handed to `ErrorFrom`. Leaving those aside, five values in the eleven
programs need an address for anything but a call.

So the analysis will not be what limits layer 1. What limits it is where
values come from. Most small aggregates are a call's result and the unwrap
that follows: Json has 80 values defined by a `Call` and 42 by an
`UnwrapResult`, Towers 15 and 16. Layer 1 loads such a result into words once,
after the call, and every copy after that is gone. But the result still
arrives through `sret`, and that first load is still a forward from the
callee's store. Layer 2 removes it.

An operation that has not been taught words keeps asking for a pointer.
Operands go through two accessors: one gives words, and one gives a pointer,
writing a value held as words to its home first. A value is immutable, so
writing it to its home is always correct, and the conversion can go one
instruction at a time.

Block parameters carry the words as several Cranelift block parameters.
Today an aggregate block parameter is a pointer to its source, copied into
the parameter's home on entry so that a loop carrying a value into its own
home does not overwrite itself. Words have no home to alias, so the copy goes
too. An OSR entry loads the words of a header's parameters from their homes,
which the interpreter wrote.

Generic values have a dynamic layout and are never held as words.

## Layer 2: a register convention between compiled functions

Layer 1 leaves every call boundary in memory: arguments are pointers, and
results come back through `sret`. Layer 2 gives compiled code a convention of
its own:

- scalars and values held as words are passed by value, one register per
  word;
- `ref`, `mut` and `out` parameters, and aggregates too large for words, stay
  pointers;
- a result held as words comes back in registers, through Cranelift's
  multiple return values, and a larger one through `sret` as now.

The tiers still meet through pointers. Each compiled function gets two
entries: the internal one with the register convention, and an adapter with
today's convention, which loads the arguments, calls the internal entry and
stores the result to `sret`. The interpreter's `call_jit`, dispatch and AOT
exports use the adapter. A code cell holds the internal entry, since only
compiled code reads it. The stub for a callee that is not yet compiled already
has the callee's signature; with the register convention it stores its
arguments to memory and calls `__jit_dispatch_call` as before.

Neither OSR nor the natives change. An OSR entry keeps `(rt, sret, frame)`
and reads parameters through the pointers the interpreter keeps. A rider
native keeps the C convention of a pointer and a descriptor per argument
([native-abi.md](native-abi.md)).

This is the layer that removes `fib`'s forwards: `n - 1` goes in a register,
and `ok n` comes back in two.

## Layer 3: slots

A slot nobody addresses could be a Cranelift `Variable`, read from its home at
an OSR entry. But slots are where `mut` arguments, field references and
tracking bytes point, and where references the interpreter made before an OSR
entry point. The analysis has more to rule out than layer 1's, and the payoff
is smaller: loop-carried state is already block parameters, which layer 1
handles. This layer waits until the first two have been measured.

## What exists, what is missing

Exists:

- `FrameLayout`, giving every value, slot and tracking byte a home that the
  interpreter and compiled code agree on.
- Scalars as Cranelift values, including scalar block parameters.
- `copy_bytes` and `fill_bytes` (`crates/datalove-datafun-cranelift/src/codegen/bytes.rs`),
  which open-code small fixed copies, so what remains is the number of copies,
  not their cost each.
- `Instruction::for_each_operand` and `Terminator::for_each_operand`, which the
  use analysis walks.

Missing, when this was written: the words representation and the analysis
choosing it, words through block parameters and `Return`, and the internal
entry, its adapter and stubs with the register convention. All of it exists
now except words at an OSR entry; see
[Where it stands](#user-content-where-it-stands).

## Phases

1. **Words within a body.** The representation, the analysis and the two
   accessors, with every instruction still asking for pointers. Then teach
   words to block parameters, `Return`, the wrap and unwrap instructions,
   `Pack`, `Unpack`, `GetField`, `Copy` and `Move`, measuring as each lands.
   The engine fixtures run every program in all eleven configurations, OSR
   ones included, and `jit_path_tests` checks that compiled code is still
   entered where it should be.
2. **The register convention.** The internal entry and its adapter, code cells
   holding the internal entry, and stubs with the register convention. Measure
   `fib` and Towers by store-to-load forwards (`ls_stlf` on this machine) and
   instructions as well as time, since cycles under the JIT move by up to 50%
   between runs of the same binary with where the code lands.
3. **Slots**, if the first two leave enough on the table to justify it.

## Open questions

- **What does zeroing a moved value guard against?** A `SlotStoreMove` zeroes
  its source "to prevent double-free". For a source that is a slot that is
  needed. Is it ever needed for a source that is a value, which nothing reads
  after the move? A value held as words has nothing to zero, so the answer
  decides whether layer 1 changes behaviour there or only removes dead stores.
- **How large should a value held as words be?** Four words covers `!T` of a
  word-sized `T`. On x86-64, Cranelift returns two words in registers and puts
  more in an implicit return area, which is memory again. Is a three- or
  four-word result in that area cheaper than `sret`, or the same thing?
- **Should a planned call site in the interpreter enter the internal entry
  directly?** `Plan::Jit` already plans each site once; it could marshal
  arguments into registers itself and skip the adapter's loads and stores.
- **Does anything read a compiled frame?** If an error report, a debugger
  helper or a test inspects compiled frames for values, values held as words
  will not be there.

## Where it stands

All three layers were done in November 2026.

Each layer keeps something somewhere other than its home, and is only right
if it knows every use that writes the home or keeps its address. That is the
IR's to say, for every instruction: `Instruction::for_each_place_use`
(`crates/datalove-datafun-ir/src/lib.rs`) is exhaustive, so a new instruction
does not compile until it says how it uses each place it names, and both
analyses below read it rather than keeping lists of their own. Likewise the
order of a call's parameters is written down once, in `ParamLayout`
(`codegen/signature.rs`), which the signature, a call, a function's entry
block, the JIT's adapter and its stubs all place their values by.

**Layer 1** is `codegen/words.rs`. A value is held as words if its type is a
static aggregate of at most 32 bytes and no use keeps or writes through its
address (`find_word_values`): a use that changes, replaces or lends it, or a
call argument the callee takes `mut` or `out`. A collection whose element is
reached or which is walked stays in memory too, though such a use only reads
it, because each would store it to its home again. Words are cut as `bytes::pieces` cuts a copy, and a part is
shifted in or out of the piece holding it; a part that would straddle two
falls back to memory. Every use not taught words asks for an address and gets
the value's home with the words just stored there. An OSR entry holds nothing
as words, since its carried values are the interpreter's.

The zeroing question has its answer: it guarded nothing any longer. It came
in, in January 2026, when the JIT's slot store destroyed what the slot held
itself and the source was destroyed too. Since then a store destroys nothing,
a drop in front of it does, ownership of a value is precise, and a slot or
parameter that may have been moved has a tracking byte; lowering moves into a
slot only from a value, and `simplify` forwards only copies, of copy types.
The interpreter and the bytecode never zeroed. Neither compiled backend does
now: not Cranelift's slot store, nor C's wraps, `ErrorFrom` and `DataFrom`.

**Layer 2** is `signature.rs`. An `in` argument of a scalar or a type held as
words is passed by value, and such a result comes back in registers; the rest
are pointers and `sret` as before. Compiled functions use the tail calling
convention, which has eight integer return registers on x86-64 where the
system's has two, so a four-word result needs no return area. A callee keeps a
parameter passed by value in registers unless it is tracked or a use needs its
address, in which case it stores it to a home of its own at entry. The JIT
compiles an adapter with the pointer convention beside each function
(`define_entry_adapter`) for the bridge and planned calls; a code cell holds
the function itself, and a stub has its signature, putting arguments in
memory only on the way to the dispatcher. An OSR entry keeps
`(rt, sret, frame)`. AOT needs no adapters: only `main` calls in, to a
function that takes the runtime handle alone.

Measured on Towers and Storage, in cycles:

| | before | layer 1 | layer 2 |
|---|---:|---:|---:|
| Towers, JIT | 8.73G | 7.82G | 4.89G |
| Towers, AOT | 9.71G | 7.87G | 4.39G |
| Storage, JIT | 11.11G | 9.33G | 8.74G |

Store-to-load forwards on Towers went from 3.65G to 1.38G a run. Storage
is mostly instructions (33G against Java's 4.4G) and gained less.

**Layer 3** is `codegen/slot_vars.rs`, after vars stopped being tracked
when their state is fixed (`compute_tracking`; a var declared without a
value, one a loop gives a value it was entered without, and a script unit's
stay tracked). A slot is held in registers, one Cranelift variable per word,
if it is untracked, of a static type held as words or a scalar, and every use
of it is one compiled against the registers (`find_register_slots`): a read,
a drop, a slot store, a scalar's update, an `in` or `ref` argument, and a
`GetFieldRef` whose reference only an open-coded swap takes. A `mut` or `out`
argument, a field write or drop, and an update of a collection in it rule it
out. A use reading through its address gets its home written first. Writing
through the home and reading the slot back after was tried and was slower
than memory: a narrow store into the home and the wide loads after it are a
store the processor cannot forward. A scalar's update (`set i +!= 1`)
defines the variable anew, and a reference to a field handed only to an
open-coded `mem_swap` or `mem_replace` is kept as a place in the slot's
words, which the swap exchanges in registers.

The same forwarding rule turned up at the list headers. A push or pop stores
the size alone, the runtime grows a list storing size and capacity apart, and
a swap or a move reads the header as words; so a word holding a list's or
a string's two counts is loaded as the two halves and put together
(`load_words`). Loaded as the word, Towers waited on it for a sixth of its
time; stored as the word instead, Storage did.

Measured against the precise vars alone, in cycles: Mandelbrot 46% fewer
AOT, Towers 16%, N-body 13% (17% under the JIT), Sum 7%; the rest within
noise.

An OSR entry first kept its slots in memory, and the JIT missed most of
that: Mandelbrot's loop, like any long one in a function called once, is
entered by OSR. It now holds them as a function does, read from the
interpreter's frame on entry (`load_slots_from_frame`). Nothing has to be
written back: OSR code runs the rest of the call, and a call it abandons is
dropped without the interpreter looking in the frame. Under the JIT that
took Mandelbrot from 2.51G cycles to 1.34G, Sum 11% and Storage 10%; the
rest ran the same instructions. Values are still not held as words in an
OSR entry: its carried values are a variable each, an aggregate's holding
its address in the frame.
