# The compiler's salsa architecture

What is tracked, at what granularity, and what that means a recompile does.

This is the map. [salsa-patterns.md](salsa-patterns.md) is the rulebook -- why
things are shaped this way and the mistakes that are easy to make. The phase
narrative is in [compiler-guide.md](compiler-guide.md).

**Every number here is generated, not remembered.** A hand-written inventory of
127 tracked functions rots within a session; the one that used to live in the
compiler guide listed fifteen and was six queries and one signature out of date
when it was found. Regenerate with:

```
cargo run --release -p datalove-bench --example query_census
```

which compiles the system library plus a local module and prints what ran for a
cold compile, two unchanged recompiles, an edit to your own module and an edit
to the bottom of the system library, then every query that kept a memo and how
many it kept. The figures below are that output for 26 modules.

The counts in the next table are a source scan rather than a database one, so
they come from a different command -- and they had drifted to 60 here and 59 in
salsa-patterns.md before this was written down:

```
python3 -c 'import pathlib,re
k={"interned":0,"input":0,"struct":0,"fn":0}
for p in pathlib.Path("crates").rglob("*.rs"):
    L=p.read_text().splitlines()
    for i,l in enumerate(L):
        m=re.match(r"\s*#\[salsa::(interned|input|tracked)",l)
        if not m: continue
        j=i+1
        while j<len(L) and re.match(r"\s*#\[",L[j]): j+=1
        st=re.match(r"\s*(pub\S*\s+)?struct\b",L[j])
        k[m.group(1) if m.group(1)!="tracked" else ("struct" if st else "fn")]+=1
print(k)'
```

## The databases

The compiler's is `datalove-datafun-compiler/src/lib.rs`:

```rust
#[salsa::db]
#[derive(Default, Clone)]
pub struct Database { storage: salsa::Storage<Self> }
```

It implements `DbClone`, so a clone shares `Arc<Zalsa>` -- the memo state -- and
gets its own thread-local. That is what the rayon paths hand to workers.
`Database::recording(recorder)` returns one that reports every query it runs,
and clones report to the same recorder, so work farmed out to rayon is visible.

`bcts`, `datalove-datalit` and the `datalove` facade each declare a plain salsa
database of their own. Compiler work uses the datafun one, re-exported as
`datalove_datafun::Database`.

## What there is

| kind | count | what it is |
|---|---|---|
| `#[salsa::input]` | 1 | `Source`. A path's text, changed with `set_text`. The only thing that comes from outside. |
| `#[salsa::interned]` | 12 | Deduplicated by content: `InternedText`, `InternedSubText`, `ModuleId`, `Module`, `ModuleGraph`, `Package`, `PackageModule`, `PackageWorld`, `Script`, `ScriptUnit`, `ReachableFuncIds`, `RiderSources`. |
| `#[salsa::tracked]` struct | 57 | Computed values with an identity. Three have a `#[tracked]` field; see below. |
| `#[salsa::tracked]` fn | 126 | Of which 43 keep a memo for a module compile; the rest are script, datalit and lexing paths. |

### The three structs with a tracked field, and why

A tracked struct's identity is a hash of its *untracked* fields. A `#[tracked]`
field is read through an edge of its own, so it can change while the identity
stays put -- and **every read of one is a dependency edge salsa must walk when
it validates a memo**. That is the whole trade, and it is measured both ways in
salsa-patterns.md. Only these three earn it:

| struct | tracked fields | why |
|---|---|---|
| `ParsedModuleGraph` | 4 of 5 | Identity is the `ModuleGraph`, so a query that reads only the graph is not disturbed by a parse. This is what lets `graph_declares_consts` and friends walk the modules without depending on their statements. |
| `StmtFun` | 2 of 5 | `FunSignature` and `body`, apart, because name resolution reads the signature and not the body. **That split is the parse firewall.** Folding them is 19% faster and makes a body edit re-typecheck every dependent module. |
| `ModuleLowered` | 1 of 2 | Identity is the `ModuleId`; the IR rides behind the edge. This is the handle phase 5 passes around so a query keyed on one costs a word rather than a hash of every instruction. |

`ExprFun`, `TypeFunction` and `ExprFunctionCall` used to be on this list and
are not: expressions, signatures and call sites are read whole by consumers
that are keyed per module, so the fine grain cost edges and bought nothing.
Untracking them was worth 2.6x, 12% and 12% of a 512-module edit.

The call site one needed a fixture with calls in it to see. The synthetic world
in `recompile_profile` is arithmetic and has none, so it reported the change as
free; the effect only appears once the program actually calls functions. That
is worth remembering before concluding that a change to the AST does nothing --
**check that the fixture contains the thing being changed.**

## The queries, by granularity

Granularity is the number of memos a query keeps, and it is the thing to know
about a query, because it says what the query is allowed to depend on. The
`compile_scaling_tests` suite holds exactly that: **a query must not depend on
anything finer-grained than its key.**

### One memo: keyed on the graph or the world

These re-run whenever anything they read moves, which is most edits. They may
look at each module; they must not look at each function.

| query | crate | what it holds |
|---|---|---|
| `parse_module_graph` | compiler | `ParsedModuleGraph`: statements and resolved requires per module |
| `resolve_all_names` | resolve | the per-module name resolutions, gathered |
| `ir_module_ids` | compiler | every module's `IrModuleId`, riders numbered first. The one place that decides one |
| `rider_interfaces` | compiler | each rider's signatures and the statement behind each native, keyed on an interned `RiderSources` |
| `compute_func_id_map` | compiler | `FuncIdMap`: every function's `(IrModuleId, FuncId)` |
| `func_id_lookup` | compiler | the same as a lookup map, memoized so it is built once per revision rather than once per const |
| `graph_declares_consts` | compiler | whether phase 5b has anything to do |
| `dependencies_of`, `import_demands`, `package_world_map`, `module_world_map`, `resolve_package_world`, `basic_config` | datafun/pkg | package resolution |

### A handful of memos: keyed on a phase's arguments

| query | memos | why more than one |
|---|---|---|
| `typecheck_module_graph`, `analyze_module_graph` | 2 | the module path and the script path key differently |
| `close_shapes_over_calls` | 4 | twice per compile, once per lowering stratum |
| `ctfe_module_registry` | 4 | likewise, one per stratum |
| `create_module_graph_lowering_result`, `module_function_registry` | 4 | one per distinct lowering result; `module_function_registry` is capped at `lru = 4` because its key moves on every edit |
| `resolve_module_exports`, `module_function_asts` | 4 | keyed per module, but only asked of a module something imports from -- four on the system library, where most modules are required by nobody |

### One memo per module: the per-module passes

These are where the work is, and they are what an edit is supposed to re-run
exactly one of. They may look at their own module's functions; they must not
look at the rest of the world -- which is what `reachable_func_ids` is for.

`parse`, `lex_chunk`, `bracer`, `source_map`, `basic_source_map`,
`parse_module_full`, `parse_module_ast`, `resolve_module_names`,
`resolve_module_imports`, `module_import_demands`,
`typecheck_module`, `analyze_module`, `module_const_kinds`,
`lower_module_functions`, `module_shape_inputs`, `shape_closed_module`,
`merge_module_strata`, `module_has_comptime_calls`, `ctfe_module_units`,
`lower_module`, `module_code_units`, `reachable_func_ids`.

Some keep more than one memo per module -- `lower_module_functions` keeps two,
one per lowering stratum, and `reachable_func_ids` keeps one per module per
graph shape it has seen.

`resolve_module_imports` belongs here and only recently did. It was keyed on
gathered maps of every module's exports and function ASTs, whose identity is a
hash of the lot, so a signature edit anywhere gave it a new key for every module
in the world. It asks `resolve_module_exports` of the modules its own module
requires instead, so what reaches it is the dependency graph rather than the
world. The section below on what the suites do not hold says why nothing caught
that.

## What a recompile does

Measured on the system library plus one local module, 26 modules:

| | queries run |
|---|---|
| cold compile | 615 |
| unchanged recompile | **0** |
| a second unchanged recompile | **0** |
| edit one function in your own module | 23 |
| edit one function at the bottom of the system library | 23 |

The two edits costing the same is the point and not a coincidence: what an edit
pays for is the per-module work for the module that changed plus the
graph-keyed passes, and which module changed does not enter into it.

The 23 are: the edited module's `parse`/`lex`/`source_map` chain, its
`typecheck_module`, `analyze_module`, `lower_module_functions`,
`module_shape_inputs`, `lower_module` and so on -- one each -- plus the
graph-keyed passes that must re-run because one of their inputs moved. Nothing
runs per unchanged module.

It was 28 while the rider interfaces were built inside `parse_module_graph`,
which re-ran on every edit. That meant the rider's own source was re-lexed and
re-parsed each time -- the second `parse`, `lex_chunk`, `bracer` and
`source_map` in the count -- and it built the rider's `Source` with
`Source::new` on each pass, so a compile leaked an input per rider, inputs
never being collected. `rider_interfaces` is keyed on an interned
`RiderSources` now, so it happens once per distinct rider source.

**An edit costs the same whatever the world size.** Seventeen queries at 8
modules and seventeen at 64 in the synthetic fixture; the 28 above is larger
only because the system library has riders and consts. What still grows with
the world is the *validation walk* -- salsa checking each memo is still good --
which is inherent while the graph-keyed passes ask about every module.

## The invariants, and where they are held

| property | held by |
|---|---|
| an unchanged recompile runs no queries, with and without consts | `incremental_lowering_tests` |
| an edit lowers and assembles exactly one module | `incremental_lowering_tests` |
| adding a module leaves the others alone | `incremental_lowering_tests` |
| an edit runs the same queries whatever the world size | `compile_scaling_tests` |
| a graph-keyed query does not depend on every function | `compile_scaling_tests` |
| a per-module query does not depend on the rest of the world | `compile_scaling_tests` |
| the parse firewall's four rows | `parse_firewall_tests` |
| an edit reaches the importers of what changed, and no further | `import_memo_tests` |
| a rider's stubs are minted once, from the rider sources alone | `import_memo_tests` |
| every shape of edit reaches only what the graph says it can | `edit_reach_tests` |
| adding or removing a module at the end leaves the others entirely alone | `edit_reach_tests` |
| `Roots::All` compiles the world and `Roots::From` only what it reaches | `roots_tests` |
| per-module, per-phase behaviour across add/remove/change | 16 `module_memo` fixtures |
| `extract_dependencies` costs what changed | `incremental_memo_tests` |

### What none of them hold

- **Aggregates are invisible to the memo-size tests, interned or tracked.**
  Widening `ReachableFuncIds` to the whole world was tried against them and
  passes, because an interned value is one dependency however much it
  aggregates. The same held for `AllModuleExports`, a *tracked* struct whose
  identity hashed every module's exports: it sat in `resolve_module_imports`'
  key and cost eleven of twenty-five modules a full re-typecheck on any
  signature edit, and every memo-size test passed throughout. What sees this is
  asking how far downstream an edit travelled, which is what `import_memo_tests`
  does and what nothing did before.
- **Dependency-edge counts are not measured directly**, so the `#[tracked]`
  decisions rest on the memo-size proxy. Re-adding `#[tracked]` to
  `ExprFun::expr` costs 2.6x at 512 modules and passes everything.
- **Durability** is not observable through salsa events; see
  `durability_tests`' own header.
- **Wall-clock anything.** The suite says what runs and what a run depends on.
  It does not say what a run costs. Profile before believing a change is free.
