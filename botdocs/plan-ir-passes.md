# Organizing the IR passes

A plan for running datafun's IR passes in one order, per function, memoized,
and for the compile time they cost. Written October 2026, after `inline` and
the cleanup after it (`ir::simplify`) made the passes a quarter of lowering.

## Contents

- [Where compile time goes](#user-content-where-compile-time-goes)
- [The passes as they are](#user-content-the-passes-as-they-are)
- [The order](#user-content-the-order)
- [Per function, in the module's query](#user-content-per-function-in-the-modules-query)
- [Later: per-function queries](#user-content-later-per-function-queries)
- [Later: lowering on demand](#user-content-later-lowering-on-demand)
- [Phases](#user-content-phases)

## Where compile time goes

A script requiring every `sys/std` module and calling nothing, release build,
timed inside the pipeline:

| Phase | ms |
|---|---:|
| Parse, resolve, typecheck, ownership | 7.0 |
| Function ids and setup | 1.9 |
| 5a: lower every function | 5.0 |
| 5a/5b: the CTFE registry, and evaluating module consts | 1.5 |
| 5c: specialization | 0.04 |
| 5c': inline and simplify, over the whole graph | 3.75 |
| 5d: assemble, const inlining, dead code, promotion | 1.9 |

Lowering is twice what analysis costs, and nearly all of it is for functions
that never run. Inlining alone is a quarter of it: a pass over every module
of the graph, not memoized, that clones each function calling something
inline and mints a new handle for its module, which salsa then compares whole.

## The passes as they are

| Pass | Where | Runs |
|---|---|---|
| Const inlining, constant branches, dead blocks, dead code | `datafun-const/inline.rs`, `dce.rs` | modules: 5d, per module; scripts: phase 4 |
| Const-parameter specialization | `compiler/specialize.rs` | modules: 5c, graph-wide, only if a call has const arguments; scripts: phase 5 |
| Inlining, with `simplify` after | `ir/inline.rs`, `ir/simplify.rs` | modules: 5c', graph-wide; scripts: phase 5b |
| Dead code after inlining | `dce.rs` | modules: 5d (as above); scripts: phase 5b |
| Const promotion | `datafun-const/promote.rs` | modules: 5d; scripts: phase 6 |

The two pipelines do not agree on the order. A script const-inlines, then
specializes, then inlines; a module specializes, inlines, then const-inlines.
The difference is observable: specialization finds a const argument's value
only where a `Const` instruction defines it, so in a module a function's own
`const` cannot be a const argument, while in a script it can.

## The order

One order for both, each step per function:

1. **Const inlining**: declared consts replaced by their values, branches on
   them folded, dead blocks and dead code removed.
2. **Specialization**: each call with const arguments pointed at the copy
   for those values. Graph-wide, since it makes functions, and gated as now.
3. **Inlining**, with `simplify` as its last step: each call to an `inline`
   function replaced by the callee's body as steps 1 and 2 left it.
4. **Dead code**, for what inlining left unread.
5. **Const promotion**, last, since specialization reads const arguments out
   of the `Const` instructions it replaces.

Steps 1 and 3 to 5 touch one function at a time and read nothing but that
function and, for step 3, the bodies of inline functions. That is what lets
them run where the function's module is assembled.

## Per function, in the module's query

For modules, step 1 moves to just after const evaluation (5b), a memoized
query per module (`const_inlined_module`), and steps 3 to 5 move into phase
5d's `lower_module`, which is already a query per module and already runs in
parallel. The graph-wide phase 5c' goes. `lower_module` is given the handles
of the modules that define an inline function (`InlineModules`, interned, so
an unchanged set is the same key), and resolves an inline callee there. One
`Inliner` serves every function of a module, so a callee is expanded once per
module rather than once per call.

Scripts run the same steps through the same function (`optimize::finish`),
over the module registry's finished functions for a module callee.

The inline recursion check stays where it is, in the inliner, since a cycle
can only be met by expanding it.

## Later: per-function queries

The module is the finest key the pipeline has: phase 5a lowers a module's
functions together, and its result is one handle. A query per function would
need a handle per function out of 5a, so that editing one function's body
changes one key. With that, steps 3 to 5 become `optimized_function`, a query
per function reading its callees' expanded bodies through queries of their
own, and an edit re-optimizes the function and what inlined it.

## Later: lowering on demand

The interpreter and the JIT could ask for a function's IR the first time it
is called rather than finding every function of the import closure lowered;
for a script requiring every module, that is most of the 15 ms. What holds it
up is consts. A module's consts are evaluated when the module compiles, and a
failure is an error then, whether or not anything reads the const; lowered on
demand, a const's evaluation and its failure would move to its first use, and
CTFE's own lowering with them. Whether that is acceptable, and what a module
author would see, is not settled. Specialized copies also need an identity
that does not depend on which others exist, which today they get from being
numbered in a pass over the graph.

## Phases

1. **The order, per function** (done): const inlining moved before
   specialization for modules, as phase 5b'; steps 3 to 5 in `lower_module`
   and in the script pipeline through one function; phase 5c' removed.
   Specialization now removes the code that built the const arguments of the
   calls it rewrites, which module const inlining used to do after it.
   Against the table above, 5b' is 0.23ms and 5d 4.4ms, where 5c' and 5d were
   5.65ms together; the whole run went from 35.0ms to 33.6ms. With
   `DATALOVE_PARALLEL` set, 5d is 1.5ms, and the run went from 29.3ms to
   26.3ms, since inlining now runs in the parallel phase.
2. **Per-function handles out of 5a**, and `optimized_function`.
3. **Lowering on demand**, once the const question is settled.
