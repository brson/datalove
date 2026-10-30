# Values in Registers for the Cranelift Backend

Written October 2026, after profiling the AWFY benchmarks under the JIT; not
implemented. Measurements are from `benchvs` at commit `80b5d68cd`.

## Contents

- [The problem](#user-content-the-problem)
- [What the frame layout requires](#user-content-what-the-frame-layout-requires)
- [Layer 1: small aggregates as words](#user-content-layer-1-small-aggregates-as-words)
- [Layer 2: a register convention between compiled functions](#user-content-layer-2-a-register-convention-between-compiled-functions)
- [Layer 3: slots](#user-content-layer-3-slots)
- [What exists, what is missing](#user-content-what-exists-what-is-missing)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

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

Missing:

- The words representation, the use analysis that chooses it, and the two
  operand accessors.
- Words through block parameters, OSR entry loads of words, and `Return` from
  words.
- The internal entry, its adapter, and stubs with the register convention.

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
