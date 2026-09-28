# Compiling what is reachable

The world is every module a worldfile mentions. A program that uses the system
library gets two dozen modules it may touch none of, and compiles all of them.
This is the plan for compiling what the program actually reaches, with the
measurements that decide its shape.

## What it is, and what it is not

**Module-level reachability**: pick a set of root modules, walk the `require`
edges transitively, and put only what you reach into the `ModuleGraph`.

It is *not* function-level dead code elimination, which is a different
granularity with a different cost profile -- and `salsa-patterns.md` has the
measurement arguing against fine grain when the consumers are coarse. Treat that
as a separate question.

It is also not `reachable_func_ids`, despite the name. That narrows a *memo key*
so per-module lowering does not depend on the whole world's func-id map. It
prunes nothing; every module it is asked about has already been compiled.

## What it is worth, measured

`cargo run --release -p datalove-bench --example cold_phases -- 15`, on the
system library plus one local module, 25 modules, one rider:

| | whole world | reachable from a module that requires nothing |
|---|---|---|
| package resolve + graph | 9.45ms | 0.07ms |
| phases 1-4 (check) | 8.31ms | 0.62ms |
| phase 5 (lower) | 10.37ms | 0.04ms |
| **whole** | **~25ms** | **0.67ms** |

The whole-world column has not moved through any of this work and is not meant
to: `Roots::All` is the same compile it always was. The pruned column went
10.16ms, then 9.29ms once the world stopped being parsed twice, then 0.67ms once
resolution stopped reading modules nothing reaches.

End to end, `datalove script` on a program that requires nothing went **43.0ms to
6.6ms**, and on one that uses `sys/std/option` **42.6ms to 7.9ms**.

Three things follow, and the second was a surprise.

**Pruning phase 5 alone is not enough.** *Check the whole world, lower from
roots* is the split that keeps errors eager, and it caps the win at phase 5's
share -- 30% when first measured, 36% now. Pruning the graph itself reaches the
frontend too, which is why `Roots` prunes the graph.

**Package resolution was the cold compile, until it followed the roots.** It was
36% before pruning and 93% of what was left after, because resolving `require`s
means parsing, and it read every module to do it. It walks from the roots now and
is 0.07ms of a pruned compile. See below.

**The edit loop is not what this is for.** An edit already costs 23 queries
whatever the world size; `compile_scaling_tests` holds that and
`edit_reach_tests` holds that its blast radius is the dependency graph.
Reachability is for cold compiles, full rebuilds, the size of the emitted
artifact, and the validation walk -- salsa revalidating fewer memos.

## The shape: a parameter, not a mode

`Roots` (`datalove-datafun/src/incremental.rs`) is an argument to
`IncrementalModuleWorld::build_graph`:

```rust
pub enum Roots {
    All,
    From(BTreeSet<String>),
}
```

`Roots::All` is bit-for-bit what the compiler did before there was a choice, so
the two paths are one path with a different argument rather than two that drift,
and every existing suite keeps working unchanged. `ModuleGraph` is interned, so
the roots become part of the graph's identity for free: a different root set is a
different graph, not a memo to invalidate. `roots_tests` holds that changing them
on a live pipeline gives both answers.

`build_fresh` and `prepare_for_compile` were identical wrappers and are gone;
`build_graph` is the one entry point.

## It changes what is an error

A type error in a module nothing requires stops being an error. That is pinned by
`an_error_in_an_unreachable_module_is_not_reported`, deliberately, because it is
the cost of the feature rather than a bug in it.

So `Roots::All` stays the default, and the line to draw is that **compiling from
roots is for running a program, not for vouching for a world**. CI and
`just test` want `All`. A library wants checking whether or not this program
calls into it.

## Package resolution, profiled

`cargo run --release -p datalove-bench --example resolve_profile -- 15`:

| step | |
|---|---|
| intern packages | 0.01ms |
| `import_demands` | 8.45ms |
| resolve demands to modules | 0.03ms |
| `to_module_graph` + path walk | 0.03ms |
| `build_graph` | 0.06ms |

**Resolution is `import_demands` and nothing else** -- 98.5% of it -- and
`import_demands` is a parse of every module in the world. Resolving the demands
once they are found, sorting the graph and interning it come to 0.13ms between
them. So there is no resolution algorithm to speed up: there is a parse.

It used to be a *second* parse. `module_import_demands` took a `Source` and
called `parse`, which is `parse_with_module_id` with no module id; phase 1's
`parse_module_full` passes one. Two tracked functions over one body of work, so
every module was parsed twice and resolution's copy was discarded but for its
`require` lines. Keying `module_import_demands` on the `Module` phase 1 uses
makes them share: a cold compile went from 33.44ms to 24.58ms and from 640
queries to 615, `parse` from 26 memos to one -- the rider source, which still
goes through it.

`resolve_profile`'s last row is the guard. It parses every module the way phase 1
does, after resolution, and wants to be a memo hit: 5.33ms before, 0.02ms after.

The second parse was also minting a second copy of every module's AST, which no
timing showed. `ExprFun` went from 288 structs to 144 and `StmtFun` from 48 to
24 -- exactly halved -- and the tracked structs a 24-module world holds went from
135224 bytes to 107000. `database_memory_tests` found that, by failing: it asked
for tokens to be under 40% of all tracked-struct bytes, and a 21% smaller
denominator took them to 47% without a byte of them moving. The assertion is an
absolute one now, for the reason written on it.

Resolution follows the roots now, so that one parse of the world is no longer of
the world: it is of what the roots reach. On the fixture above, resolution went
from 8.66ms of a pruned compile to 0.07ms, and the pruned compile from 9.29ms to
0.67ms.

The walk asks `module_import_demands` for a module, resolves each demand to a
path, and recurses -- and because that query is keyed on the `Module` phase 1
uses, reaching a module during the walk is what makes phase 1's parse of it a memo
hit. The two share one parse and now only of what is needed.

## Status

Done:

- `Roots` as a parameter to `build_graph`, threaded through the pipeline with
  `set_roots`, defaulting to `All`.
- `roots_tests`: transitive requires, direction, unions, empty roots, roots that
  name nothing, the error-visibility change, and changing roots between compiles.
- `cold_phases` and `resolve_profile`, which are where the numbers above come
  from.
- The duplicate parse of the world, removed.
- **The roots decided from the program**, for `script`, `script-ir`,
  `aot-compile` and `script-world`. `narrow_roots_to_script` reads a script's
  `require`s and narrows to them; it refuses when one does not resolve, because
  pruning on a program whose requires are wrong would drop the module the
  diagnostic is about. `typecheck-std` and the REPL still pass `All`.

  A real `datalove script` invocation goes from **43.0ms to 20.3ms**, and a script
  that does use the standard library from 42.6ms to 20.7ms -- it pulls what
  `sys/std/option` reaches rather than all two dozen modules.

  A worldfile's own modules are roots whether the script reaches them or not.
  That distinction is the thing to remember: **a module the author wrote in the
  artifact is part of what they asked to be compiled; a library module they do
  not use is not.** 19 of `world_error_tests`' fixtures are a broken module and a
  script that ignores it, and they are right to expect an error.

Next, roughly in order of what it buys:

1. **Stop numbering modules by position.** `ir_module_ids` numbers regular
   modules by their place in the graph, so a module appearing anywhere but the
   end renumbers what follows and those modules re-lower. Today that happens when
   you add a file. Under reachability it happens whenever a program's imports
   change, which in a REPL is constantly. Deriving the id from the module's path
   rather than its position removes it. The riders already work this way.
2. **Prune codegen.** `datalove-datafun-cranelift-aot` says "whole-world
   compilation" and walks the whole registry; DCE
   (`datalove-datafun-const/src/dce.rs`) only removes unreachable blocks within a
   function. Pruning the graph shrinks the registry, so most of this falls out --
   but an unused function in a *reachable* module is still emitted.
3. **Rider interfaces are not pruned.** `rider_interfaces` parses every rider
   source whether or not anything requires it. Memoized and keyed on the sources,
   so it is paid once, but it is not nothing on a short-lived invocation.
