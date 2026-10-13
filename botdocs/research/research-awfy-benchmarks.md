# Are We Fast Yet for benchvs

Which of the [Are We Fast Yet](https://github.com/smarr/are-we-fast-yet)
benchmarks (AWFY) would cover what `benchvs` does not, and what each needs of
datalove: written as it is, or with modest additions. Surveyed at AWFY commit
`74306fe` (March 2026), October 2026.

AWFY is a suite for comparing language implementations: each benchmark is
written in Java, JavaScript, Python, Ruby, Lua, Smalltalk and others, under
rules that keep every port to a common core -- objects, arrays, closures,
strings, a few collections of its own (`Vector`, `Dictionary`, `Set`) and its
own `Random` -- so a difference is the implementation's rather than the port's.
The Java and JavaScript versions come with it, which is what `benchvs` compares
against. Its licensing is mixed: Richards and DeltaBlue are under the revised
BSD license and the rest under the repository's; a port should say where it
came from.

## What benchvs covers now

| Bench | Covers |
|---|---|
| `fib` | Calls: recursion, nothing else |
| `sum` | A loop into a bigint |
| `primes` | Integer arithmetic in a loop, and a call per number |
| `wordfreq` | Strings, a map, sorting |

Nothing in it uses floating point, writes list elements in a hot loop,
mutates structs held in a list, passes a collection `mut` through recursion,
dispatches on an enum, or is shaped like an application: many functions, a
long tail of code run once. The store demo (`demos/store`) is the one
application-shaped program, and it is not in `benchvs`.

## What datalove can express

Checked against the language as of this survey:

- **No objects, shared mutable references or recursive types.** A struct
  cannot name itself (`type Node: { v: u32, children: [Node] }` is F064,
  unknown type), and values are owned, so a linked structure is an *arena*: a
  list of structs, linked by `?index`. A struct field in a list element is
  written in place with `set xs[i]!.f = v`, and reading another element in the
  same statement is fine.
- **No closures or function values** (on the roadmap). Polymorphism is an
  enum and a `match`; a callback is a loop.
- **No panics.** AWFY's `throw` in a can't-happen check becomes `ret er` or is
  left out; every fallible index needs `!` or `?`.
- **What is there:** `f64` with `sqrt`, `sin` and `cos`; fixed-width integers
  with bit operations; lists, maps and sets; strings by byte (`get_byte`,
  `len`, `slice`, `push_char`); generics; recursion; `mut` parameters.

A probe confirmed the patterns the simpler benchmarks need: an NBody-style
pairwise update of structs in a list, a swap through a `mut` list in a
recursion (Permute's 8660 came out right), and walking an arena list linked by
`?index`.

## The benchmarks

**Ready to port, and each covers something new.** Each is a page or two.

| Bench | AWFY lines (JS) | New coverage | Notes |
|---|---|---|---|
| Mandelbrot | 120 | Floating point in a tight loop, integer bit operations | Nothing in benchvs has a float. Direct port |
| NBody | 188 | `f64` and `sqrt`, structs in a list mutated in place, a pairwise loop | Bodies are a `[Body]`; the probe's pattern |
| Sieve | 54 | Writing list elements in a hot loop | A `[bool]`; the simplest of all |
| Permute | 64 | Recursion through a `mut` list, swaps | Probed: gives 8660 |
| Queens | 87 | Backtracking recursion over `mut` `[bool]`s | Like Permute |
| Bounce | 89 | Structs in a list, branches, AWFY's `Random` | `Random` is a 16-bit LCG, a few lines |
| Towers | 96 | Stacks pushed and popped in a recursion | Disks as an arena linked by `?index` keeps AWFY's shape; three `[u32]` stacks would be simpler but not the same benchmark |

These are AWFY's micro benchmarks. Several are sized by AWFY's inner
iterations to run for microseconds, so for `benchvs`'s whole-process timing
each runs its `benchmark()` in a loop to take a few hundred milliseconds in
the fastest implementation.

**Ready to port with work, and the most valuable.** No language change, but an
arena for every object graph and an enum for every class hierarchy.

| Bench | AWFY lines (JS) | New coverage | What porting takes |
|---|---|---|---|
| Richards | 438 | Application-shaped: a scheduler over tasks and packets, branchy, optional links, struct mutation | Tasks and packets in arenas linked by `?index`; the four task kinds' closures become an enum matched per step. The canonical macro benchmark |
| Havlak | 663 | Collections in a real algorithm: lists, sets and maps of node numbers, union-find, many allocations | Blocks are already numbered, so the graph is indices by nature; `forEach` closures become loops. Exercises the runtime library the store demo's profile found to be the bottleneck |
| Json | 563 | Parsing a string byte by byte, building a tree | The parser ports directly over `string.get_byte`; the tree of values is recursive, so it becomes an arena of nodes holding child indices |

**Waiting on a language feature.** Porting these now would change what they
measure.

| Bench | Needs | Why |
|---|---|---|
| List | Recursive types | It builds linked lists recursively and recurses down them; over an arena it becomes index chasing |
| Storage | Recursive types, or values taken back out of `data` | A tree of arrays nested seven deep, as an allocation benchmark; there is no type for it, and `data`, which could hold one, cannot be read back (`issues.md`, "Nothing takes a value back out of data or error") |
| DeltaBlue | Objects, or a large arena rewrite | A constraint graph of variables and constraints pointing at each other, over a six-way class hierarchy; as arenas and enums it would be a different program |
| CD | Objects, or a large arena rewrite | A red-black tree with parent links, plus vector arithmetic; 854 lines, the largest |

## Ported so far

Mandelbrot, NBody and Sieve are in `benchvs` (October 2026), each in datalove,
Python, Julia, Java and JavaScript. Java, JavaScript and Python follow AWFY's;
Julia's is written to match. Each runs AWFY's verified size -- Sieve to 5000
(669), Mandelbrot at 750 (50), NBody for 250,000 steps (-0.1690859889909308)
-- a number of times and prints the total, so that a JIT cannot discard the
earlier runs. All eight implementations print the same total, NBody's
included: every one does AWFY's floating-point operations in AWFY's order, and
prints the shortest round-trip form of a double.

What the first timings showed, in ms (mean of three):

| Bench | interp | JIT | AOT | Python | Julia | Java | V8 |
|---|---|---|---|---|---|---|---|
| Sieve (x10,000) | 6883 | 2036 | 1885 | 3935 | 300 | 109 | 155 |
| Mandelbrot (x4) | 6817 | 2606 | 537 | 5239 | 414 | 392 | 276 |
| NBody (x6) | 8511 | 511 | 421 | 5045 | 280 | 108 | 125 |

And why, from a profile of each:

- **Sieve, 19x Java.** A list element written in compiled code is a call to
  the runtime (`dtlv_rti_element_write_local`), not a store; and
  `list.repeated`, generic, boxes each of the 5000 flags into a `data` and out
  again, about a third of the run. (Since: the runtime's element write is
  gone, a list store being one checked `ListSet`, which Cranelift and C inline
  as a store for a copy element. `repeated` still goes through `data`, which
  is what [Monomorphizing Generics](../plan-monomorphization.md) is for.)
- **NBody, 4.7x Java (fixed: now 1.6x).** Close to half of the compiled
  `advance` was calls to the runtime's generic destroy (`any_destroy_local`),
  though every value there is an `f64` or a struct of them, and owns nothing:
  the Cranelift codegen destroyed the old value before every store through a
  reference, into a field or into a parameter, whatever its type. It now
  emits no destroy for a copy type; the C backend already checked, but for a
  drop and an `out` argument. NBody went from 511 to 169 ms under the JIT and
  from 421 to 172 ms AOT; `wordfreq` and the store demo gained about 4%. The
  decision has since moved into the IR: no store destroys, and lowering emits
  a drop in front of one only where the type owns something.
- **Mandelbrot, 1.4x Java compiled, but 6.7x tiered.** With every function
  compiled at its first call it runs in 546 ms; tiered, in 2606. Its two inner
  loops need what their enclosing loop defines, so OSR refuses them, and the
  outer loop runs 750 times a call, under the threshold of 1000: the first call
  or so is interpreted. This is the program OSR's refusal of inner loops was
  waiting for. (Fixed: OSR now enters an inner loop, carrying what the outer
  one makes as a variable; tiered runs in 537 ms, the same as compiling
  everything first.)

## A generics benchmark of our own

AWFY has no benchmark that would push datalove's generics. Its generic code is
its own collection library (`som/Vector`, `Dictionary`, `Set`) holding object
references and taking closures (`forEach`, `sort(Comparator)`), used by CD,
DeltaBlue, Havlak and Json. Ported, Havlak, the most portable of them, would
run on the built-in lists, sets and maps of `u32` node numbers, through thin
`sys/std` forwarders bound to `u32`: generics in name only.

So `benchvs/ordered` (October 2026) is written for benchvs, in all five
languages. Generic ordered-collection algorithms, each written out: a binary
min-heap used as a heapsort (sift up, sift down, swaps), a check that the
result is sorted, a lower-bound binary search for a stream of probes, and a
count of distinct elements. Each runs at four element types in turn from one
call site -- `u32`, `f64`, `string` and `(string, u32)` -- over 1000 values
from AWFY's `Random`, narrowed so that about half repeat: plain scalars, a
value that owns memory, and a compound that needs a built descriptor. The
generic functions call each other, so descriptors are forwarded and the
one-entry table caches see alternating bindings. Java is generic over
`Comparable`, boxing its numbers as real Java generic code does; JavaScript
passes a comparison function; Python compares by duck typing; Julia
specializes on `isless`. All print 400769264 at the committed 200
iterations.

It replaced `benchvs/generics.dfs`, a datalove-only loop of three trivial
generic calls, and found two things on its first run:

- **A JIT bug.** A planned call from interpreted bytecode into compiled code
  handed over, as the return pointer for a result of a dynamic type, the
  frame word holding the value's pointer rather than the storage it points
  at. The result's tag landed in the pointer, and the caller read an error out
  of garbage or crashed. It had been there since phase 5 of generics by
  reference; the fixtures enter compiled code with every function compiled at
  once, or from code that is not generic. Fixed, with a JIT path test.
- **`sys/std/list.swap` was O(n).** It was written in datalove as two
  removes and two inserts, there being no other way to exchange two elements
  of a `[T]` without a placeholder, so each swap shifted the rest of the list.
  Under AOT `memmove` was over half the profile: the heap's swaps made the
  heapsort quadratic in data moved. It is now a native over a runtime entry,
  `dtlv_rti_list_swap_local`, exchanging the two elements' bytes in place.

Timings, 200 iterations, ms (mean of three), with the swap as it was and as
it is:

| | interp | JIT | AOT | Python | Julia | Java | V8 |
|---|---|---|---|---|---|---|---|
| O(n) swap | 7132 | 2533 | 2291 | 1611 | 748 | 388 | 343 |
| native swap | 4152 | 766 | 600 | 1605 | 677 | 373 | 347 |

What is left is what generics cost. Under AOT the profile is comparisons:
each `ord.less` calls the rider's `ord_compare`, which calls the runtime's
`cmp_total`, which checks that the two descriptors agree (`eq_tydesc`, 5%)
and then walks the values by descriptor (`cmp_value`, 13%); together with
`less_at` and the `less` wrapper, over 40% of the run. Java and V8 compare
through one virtual or inline-cached call. Comparing a scalar `T` without
leaving compiled code, and not re-checking descriptors a generic's table
already guarantees agree, are where the next factor of two is.

### Why the interpreter is slow on it

At 4.15 s against CPython's 1.6, interpreted `ordered` was investigated
(October 2026). Three things were the bytecode's own gaps, now closed:

- **Generic bodies ran half on the IR walker.** Lowering sent every
  instruction touching a dynamic value to the walker, so a `[T]`'s bounds
  check and element reference -- every `h[i]!` -- ran there, 80 million times
  a run, though a list's header is the same whatever it holds. `loc` now
  reaches a dynamic value or slot through its pointer, `desc` gives its table
  entry, and the two instructions are lowered (`ListBoundsCheck`,
  `ListElementRefRt`).
- **`mul_wrapping_index` and `shr_index` had no ops**, so the heap's child
  and parent arithmetic and the binary search's midpoint ran on the walker;
  they are `MulWrapIndex` and `ShrIndex` now.
- **An `i8` comparison had no op.** `ord.less` and the rest compare
  `ord_compare`'s `i8` with zero, which went through the generic binop
  routine at about 120 instructions; it is `CmpI8` and `BrCmpI8` now.

Together, 4.15 s to 3.27. What is left is calls. Per comparison, Python
evaluates `h[i] < h[j]` inline; datalove makes two bytecode calls,
`less_at` and then `ord.less`, at about 270 instructions for a call and its
return, and a native call to `ord_compare`, at about 300 more: a third of
that the native boundary and the rest the runtime's comparison by
descriptor. A loop that calls a one-op function costs about 300
instructions more an iteration than the same loop with the comparison
inline. Being generic adds little on top: about 160 instructions a call,
the table cache check and shadow stack space. So the interpreter's next
gains on code like this are in the planned call itself, and in making
`ord.less` a forwarder straight to a native, which would save one of the
two calls.

## Written through sys/std

The benches and the store demo called intrinsics directly (`icall
add_wrapping_u32` and the like) until October 2026, when they were moved to
the `sys/std` functions wrapping them, with those functions marked `inline`.
A remainder is `u32.rem_checked`, an option, unwrapped before it is compared.

Before (intrinsics) and after (`sys/std`, inlined), seconds, mean of five:

| Bench | interp | JIT | AOT |
|---|---|---|---|
| fib | 3.67 / 3.70 | 0.46 / 0.46 | 0.35 / 0.35 |
| sum | 0.79 / 0.80 | 0.40 / 0.40 | 0.38 / 0.30 |
| primes | 3.40 / 9.47 | 0.34 / 1.56 | 0.34 / 1.55 |
| wordfreq | 0.95 / 1.35 | 0.59 / 0.63 | 0.53 / 0.60 |
| sieve | 4.09 / 5.28 | 1.00 / 1.00 | 0.70 / 0.73 |
| mandelbrot | 3.11 / 3.63 | 0.57 / 0.56 | 0.55 / 0.56 |
| nbody | 3.06 / 3.19 | 0.20 / 0.20 | 0.16 / 0.16 |
| ordered | 3.27 / 3.19 | 0.77 / 0.68 | 0.59 / 0.59 |

Compiled, the calls cost nothing but in `primes`, whose inner loop is a
remainder: the inlined `rem_checked` tested for zero, built `some r`, passed
it through a block parameter -- an aggregate Cranelift copies through memory
-- and unwrapped it. The interpreter paid for what a splice leaves: a copy per
`in` argument, two jumps and a block argument, about five ops where an
intrinsic was one.

A cleanup after inlining (`ir::simplify`) now forwards those copies,
substitutes a block parameter with one way in, bypasses an option built only
to be unwrapped, merges straight-line blocks and skips empty ones. Seconds,
mean of five, intrinsics / inlined / inlined and cleaned up:

| Bench | interp | JIT | AOT |
|---|---|---|---|
| fib | 3.90 / 3.63 / 3.69 | 0.46 / 0.46 / 0.46 | 0.35 / 0.35 / 0.35 |
| sum | 0.79 / 0.79 / 0.77 | 0.40 / 0.40 / 0.39 | 0.30 / 0.30 / 0.30 |
| primes | 3.40 / 9.44 / 5.64 | 0.34 / 1.57 / 0.35 | 0.33 / 1.56 / 0.34 |
| wordfreq | 0.96 / 1.31 / 1.08 | 0.59 / 0.63 / 0.58 | 0.53 / 0.62 / 0.53 |
| sieve | 4.04 / 5.22 / 4.70 | 1.01 / 1.00 / 0.99 | 0.71 / 0.73 / 0.72 |
| mandelbrot | 3.13 / 3.62 / 3.29 | 0.56 / 0.56 / 0.56 | 0.55 / 0.55 / 0.55 |
| nbody | 3.03 / 3.25 / 3.10 | 0.20 / 0.20 / 0.17 | 0.16 / 0.16 / 0.17 |
| ordered | 3.24 / 3.14 / 2.64 | 0.77 / 0.68 / 0.63 | 0.58 / 0.59 / 0.51 |

Compiled, writing through `sys/std` now costs nothing, and `ordered`, whose
comparisons and swaps were calls already, is faster than before. Interpreted,
what is left is mostly `rem_checked`'s test for a zero divisor, two ops more
an iteration in `primes`' inner loop, which only folding the constant
divisor would remove.

## Additions that would open the rest

Modest, in the order they unlock the most:

1. **Recursive types, owned.** A type that contains itself through a list or an
   option (`type Node: { v: u32, next: ?Node }`), boxed behind the scenes:
   List, Storage and Json's tree port directly, and Towers' disks need no
   arena. Owned recursion only: it does not give the shared references DeltaBlue
   and CD need.
2. **A monotonic clock** in `sys/std`, a native returning nanoseconds. AWFY's
   harness times each iteration in one process, which is how steady-state
   performance is measured once warmup is over; `benchvs` times whole
   processes and cannot see that without one.
3. **Function values or closures.** Richards, Havlak and DeltaBlue pass
   blocks to `forEach` and store per-task functions; loops and enums stand in
   well enough, so this is for fidelity rather than possibility.
4. **Reading a value back out of `data`.** It would let Storage be written
   with `data` before recursive types exist.

The objects of the "full Datalove layer" would open DeltaBlue and CD as AWFY
wrote them, but are not a modest addition.

## Order of work suggested

1. Mandelbrot, NBody and Sieve (done): the floating-point and array-store
   coverage `benchvs` lacked.
2. Richards: the first application-shaped benchmark, and the test of whether
   arena-and-enum style datalove holds up against Java and V8 on that shape.
3. Permute, Queens, Bounce and Towers, which are cheap once the first are in.
4. Havlak and Json, for the collections and the string parsing.
5. Recursive types, then List and Storage.

The AWFY Java and JavaScript versions can be taken as they are, under their
licenses, with the harness reduced to a loop over `benchmark()` and a check of
the result. Python versions exist in AWFY too. Julia has none, and the four
existing `benchvs` benches are the only ones it would be in.
