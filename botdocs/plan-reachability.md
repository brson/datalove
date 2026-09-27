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
| package resolve + graph | 8.75ms | 8.66ms |
| phases 1-4 (check) | 7.10ms | 0.58ms |
| phase 5 (lower) | 8.73ms | 0.05ms |
| **whole** | **24.58ms** | **9.29ms** |

Those are the figures after the duplicate parse was removed; they were 33.44ms
and 10.16ms before, and the phases 1-4 column held the parse that resolution now
holds. See the resolution section below.

Three things follow, and the second was a surprise.

**Pruning phase 5 alone is not enough.** *Check the whole world, lower from
roots* is the split that keeps errors eager, and it caps the win at phase 5's
share -- 30% when first measured, 36% now. Pruning the graph itself reaches the
frontend too, which is why `Roots` prunes the graph.

**Package resolution becomes the cold compile.** It is 36% before pruning and
93% of what is left after, and it cannot be pruned by reachability as written,
because resolving `require`s is how you find out what is reachable. It has been
profiled; see below.

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

What is left is one necessary parse of the world, and it is necessary only because
resolution reads every module rather than following requires from the roots. That
is what item 1 below is about.

## Status

Done:

- `Roots` as a parameter to `build_graph`, threaded through the pipeline with
  `set_roots`, defaulting to `All`.
- `roots_tests`: transitive requires, direction, unions, empty roots, roots that
  name nothing, the error-visibility change, and changing roots between compiles.
- `cold_phases` and `resolve_profile`, which are where the numbers above come
  from.
- The duplicate parse of the world, removed.

Next, roughly in order of what it buys:

1. **Resolve from the roots, not over the world.** Resolution is a parse of every
   module, so a compile pruned to two modules still parses twenty-five: 8.66ms of
   its 9.29ms. Reachability is a walk, and a walk only needs to parse what it
   reaches -- parse the roots, read their requires, parse those. That makes
   resolution demand-driven and is what makes pruning actually pay. As it stands
   `dependencies_of` computes the whole world's dependency map up front.
2. **Decide the roots from the program.** Nothing yet computes a root set: the
   CLI and the REPL both still pass `All`. For `datalove run prog.dfs` the roots
   are the modules the script requires, which is known only once the script is
   parsed -- so the module pipeline has to be told, and in a REPL the set grows
   as lines arrive. Each `require` on a new line changes the module set, which is
   a new `ModuleGraph`, which re-runs everything graph-keyed.
3. **Stop numbering modules by position.** `ir_module_ids` numbers regular
   modules by their place in the graph, so a module appearing anywhere but the
   end renumbers what follows and those modules re-lower. Today that happens when
   you add a file. Under reachability it happens whenever a program's imports
   change, which in a REPL is constantly. Deriving the id from the module's path
   rather than its position removes it. The riders already work this way.
4. **Prune codegen.** `datalove-datafun-cranelift-aot` says "whole-world
   compilation" and walks the whole registry; DCE
   (`datalove-datafun-const/src/dce.rs`) only removes unreachable blocks within a
   function. Pruning the graph shrinks the registry, so most of this falls out --
   but an unused function in a *reachable* module is still emitted.
5. **Rider interfaces are not pruned.** `rider_interfaces` parses every rider
   source whether or not anything requires it. Memoized and keyed on the sources,
   so it is paid once, but it is not nothing on a short-lived invocation.
