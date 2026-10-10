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

1. Mandelbrot, NBody and Sieve: the floating-point and array-store coverage
   `benchvs` lacks, a page each, with AWFY's Java and JavaScript as they are.
2. Richards: the first application-shaped benchmark, and the test of whether
   arena-and-enum style datalove holds up against Java and V8 on that shape.
3. Permute, Queens, Bounce and Towers, which are cheap once the first are in.
4. Havlak and Json, for the collections and the string parsing.
5. Recursive types, then List and Storage.

The AWFY Java and JavaScript versions can be taken as they are, under their
licenses, with the harness reduced to a loop over `benchmark()` and a check of
the result. Python versions exist in AWFY too. Julia has none, and the four
existing `benchvs` benches are the only ones it would be in.
