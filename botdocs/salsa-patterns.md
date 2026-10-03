# Salsa: idiomatic and effective use

How this codebase uses salsa, why, and the mistakes that are easy to make and
hard to notice. Written against salsa 0.28.

Most of what goes wrong with salsa fails silently. The type system does not
catch it, the test suite usually does not either, and the symptom is a compiler
that is slower than it should be or that produces different bytes on every run.
So most of the sections below end with how to measure the thing, not just how to
write it.

## The four kinds, and how to choose

**`#[salsa::input]`** is data that comes from outside and is set, not computed.
Only `Source` (`bcts/src/input.rs`) is an input here: a path's text, changed
between revisions with `set_text`. Inputs have no lifetime, so their fields must
be `'static`, and they are never collected.

**`#[salsa::interned]`** is a value deduplicated by content: equal content gives
the same handle. `InternedText`, `ModuleId`, `Module` and `ModuleGraph`
(`bcts/src/module_graph.rs`, `bcts/src/text.rs`) are all interned. Reach for
this when you want a cheap handle to something you look up by value.

**`#[salsa::tracked]`** structs are computed values with an identity. They can
only be created inside a tracked function. See the next section, because the
distinction between their two kinds of field is the part that gets missed.

**`#[salsa::accumulator]`** is a side channel, used for diagnostics in
`datalove-diagnostic`. Values are pushed with `.accumulate(db)` and read back
with `query::accumulated::<T>(db, args)`, which walks the memoized dependency
graph. The compiler also returns diagnostics directly in result structs
(`PendingDiagnostic`); both mechanisms are in use.

A previous version of this file warned that accumulators do not fire on cache
hits. That was not re-verified against 0.28 and `accumulated()` is designed to
walk memoized edges, so treat it as unproven rather than as a rule.

## Tracked structs have two kinds of field, and we mostly use one

A tracked struct's identity is `hash(untracked fields) + a disambiguator
assigned in creation order within the active query`. Fields marked `#[tracked]`
are *not* part of that identity: they are read through their own dependency
edge, so they can change while the struct's identity stays put.

That is the whole point of a tracked struct. Used with no `#[tracked]` fields at
all it is an interned struct with extra steps: creating one hashes everything it
holds, and any change to any field yields a different struct, so a consumer that
reads one field is invalidated by a change to another.

**In this codebase, 54 of 57 tracked structs have no `#[tracked]` field.**
`SingleModuleTypecheckResult` has nine fields and none of them are tracked, so
its identity is a hash of the whole type table.

This used to be written up here as a smell to be worked through. It is not:
tracking a field is a trade, not an improvement, and the next section has both
sides of it measured. Marking `SingleModuleTypecheckResult` and
`SingleModuleAnalysis` tracked was tried twice -- before and after the shape
closure was changed -- and was neutral both times. Do not sweep the rest on
theory; the section below says what question to ask instead.

### Where the line is: coarse structs track, fine ones do not

The two questions -- should this be a tracked struct, and should its fields be
`#[tracked]` -- are the same question asked about granularity, and both were
measured on the same pair of AST types.

**A tracked field is a dependency edge per read.** Validating a memo means
walking its edges, so the cost of an edit is proportional to how many edges the
whole program's memos hold, not to what changed. `ExprFun` was a tracked struct
per *expression* with a tracked field, which in a 512-module world is on the
order of a hundred thousand edges. Profiling an edit there, 44% of it was
`salsa::Table::get_raw::<Value<ExprFun>>` and another 15% was `verify_memo`
above it: almost none of an edit was the compiler.

It bought nothing, because **every consumer was coarser than the grain**.
`typecheck_module`, `analyze_module` and `lower_module_functions` are keyed on
the module, so any expression changing invalidates the same consumer whichever
expression it was. A hundred edges where one would do, all belonging to one
reader. Untracking the field took a 512-module edit from 40ms to 15ms, and the
win grows with the world because what it removed scaled with the program.

**`StmtFun` is the opposite, and was measured too.** Its fields stay tracked, so
its identity is `(module_id, name, local_index)` and a literal edit leaves it
alone -- which leaves `ParsedStatements` comparing equal, which is what spares
name resolution. That is the firewall two sections down. Untracking it was tried:
a 512-module edit went from 16.5ms to 21.8ms and the database grew by 62MB,
because the statements then differ on every edit and a new set of structs is
minted each time.

So the test is not "how many fields are tracked" but:

> Is anything that reads this coarser than it is? If every consumer re-runs
> whenever *any* instance changes, the grain is costing edges and saving
> nothing.

Expressions fail that test and statements pass it. There are two orders of
magnitude more expressions than statements, which is the same thing said
another way.

**The same question applies field by field.** `StmtFun` had five tracked
fields; four of them were the signature -- type parameters, their bounds, the
parameters, the return type -- and nothing reads one of those without the
others, so they were four edges doing one edge's work. Bundled into a
`FunSignature`, a 512-module edit went from about 14.6ms to about 12.9ms and a
1024-module one from 33.6ms to 27.6ms.

The body is the field that stays separate, and the reason is the one thing
worth carrying away: **a field earns its own edge when some consumer reads it
and not the rest.** Name resolution reads the signature and not the body.
Folding the body in too is faster again -- 19% rather than 13% -- and it makes
an edit to a body re-resolve that module's names and re-typecheck every module
that depends on it. A constant factor for a cascade is a bad trade at any
scale, and a worse one the larger the program. `parse_firewall_tests` and the
`module_memo` `change_ast` fixtures both catch it.

### The one that was worth it: a handle

`ModuleLowered` (`datafun-compiler/src/tracked_lower.rs`) is the case the
distinction was made for. It has one untracked field, a `ModuleId`, and one
`#[tracked]` field holding a module's whole lowered IR. So its *identity* is a
module's, and the IR rides along behind a dependency edge of its own.

That is what a handle is for: phase 5's passes are functions of the whole
program's IR, and before this they took it by value, so keying a query on one
meant hashing every instruction in the program. `lower_module` was paying that
on every compile, and it was around a tenth of an unchanged recompile. Under
handles the key is a list of ids, and the unchanged recompile of a 32-module
world went from 2.0ms to 0.24ms.

Two things to know before reaching for the same trick:

- **A tracked struct's identity map belongs to the query instance that created
  it.** Two calls of the same query with different arguments that each mint a
  handle for module `m` mint two different handles. So a graph-keyed query that
  re-mints everything invalidates every consumer as soon as a module is added
  anywhere. `close_shapes_over_calls` hands a module back under the handle it
  came in under when it did not change it, for exactly this reason.

  **This bit a second time, and the second one is the more instructive.**
  `resolve_module_imports` minted a synthetic `StmtFun` per rider import, and
  its key included a gathered map of every module's exports -- so a signature
  edit anywhere gave it a new key, hence a new instance, hence fresh stub ids,
  hence a full re-typecheck of every module importing from a rider. Eleven of
  twenty-five on the system library, for an edit none of them could see, and
  every memo-size test passed the whole time.

  The rule worth carrying: **do not mint a tracked struct in a query whose key
  is wider than what the struct belongs to.** Re-execution on its own is
  harmless -- the same instance re-running mints the same ids. It is the *key*
  moving that costs, which is why this is invisible to anything counting
  executions.

  A rider stub belongs to the rider, so keying its minting on the graph was only
  half a fix: it survived a signature edit and then re-minted the moment a module
  appeared or disappeared, because `ParsedModuleGraph`'s identity *is* the module
  graph. The stubs are built with the `RiderInterface` now, under
  `rider_interfaces`, which is keyed on an interned `RiderSources` -- the rider
  sources and nothing else. **A rider says nothing about the module set, so
  nothing about the module set should be in the key.** `edit_reach_tests` holds
  both halves.
- **Backdating on the tracked field wants pointer equality to be meaningful.**
  `Arc<T>: PartialEq` short-circuits on `ptr_eq` when `T: Eq`, so a pass that
  only replaces what it rewrites gets the comparison for free; one that rebuilds
  everything to write one field pays a deep compare and gets no backdating.

## Never store a salsa `Id`. Use a key of your own

A `salsa::Id` is `{ index: NonZeroU32, generation: u32 }`. Freed slots are
recycled with the generation bumped, for tracked structs and interned values
alike, and salsa leaks a slot rather than let the generation wrap. So the
generation is an ABA guard, and comparing two whole `Id`s is safe.

What is not safe:

- **Dropping the generation.** `id.index()` throws the guard away, so two
  expressions that occupied one slot in different revisions become
  indistinguishable. `Id::from_index` rebuilds with generation 0, so a
  round-trip through an index is lossy.
- **Dereferencing a stale id.** Comparing is fine; asking what it points at is
  not. Tracked structs panic outright, even in release. Interned values only
  `debug_assert`, which means silent corruption in a release build.
- **Assuming stability across revisions.** An id is stable only while the
  identity behind it is. The disambiguator is assigned in creation order within
  a query, so changing the set of structs a query creates moves the ids of
  everything after the change.
- **Letting ids into output.** They put salsa's numbering into error messages
  and expected test output, which then churns for unrelated reasons.
- **Indexing a `Vec` by `id.index()`.** Ids are not dense over the subset you
  care about. One measured case held 3764 slots to record 8 entries.

The rule that is easy to remember: **a whole `Id`, never taken apart, compared
within a single revision, where the table and the ids come from queries in the
same dependency chain, is fine.** Everything else is a bug waiting.

Even when it is fine, prefer a key you define. The id version's correctness
rests on an invariant nobody can see at the call site. The keys this codebase
uses:

| key | where | identifies |
|---|---|---|
| `ExprKey { module_id, fn_name, local_index }` | `datafun-ast/src/ast.rs` | a datafun expression |
| `local_index: Option<u32>` on `ExprFull` | `datalit/src/ast.rs` | a datalit expression's position in its parse |
| `ConstStmtId(u32)` | `datafun-ir/src/lib.rs` | a const statement's position in its unit |

`ExprKey` carries a hand-written `Debug` that prints `ExprKey(fn #4)` rather
than the interned id behind `fn_name`, so keys can appear in expected output.

### A key of your own still wants one place that hands them out

`IrModuleId` is a key of our own, and it was derived from a module's position in
the graph in five different places -- `compute_func_id_map`, the specializer, the
two assembly paths and the script compiler's own `build_func_id_map`. All five
had to agree for a call to land on the function it named, and nothing checked
that they did.

Worse, one of them numbered the riders *after* the regular modules, so adding a
module anywhere shifted every rider's id, every module's `ReachableFuncIds` moved
with it, and the whole world re-lowered. `ir_module_ids` is the one place that
decides now, the riders are numbered first so their ids depend on the rider
sources alone, and `build_func_id_map` is deleted -- it was a subset of the map
the pipeline already had.

**A key you define needs a single definition as much as it needs to be stable.**
Five derivations of one numbering is five chances to disagree, and the numbering
being positional is what let a change at one end of the world move the other.

One deliberate exception survives: `InternedText`'s `Ord`
(`bcts/src/text.rs`) compares by id. `Ord::cmp` receives no database, so it
cannot compare content. See the determinism section for why it is load-bearing
rather than a wart.

## Key a query on one entity where you can

A tracked function's memoization key is all of its non-`db` arguments. With more
than one, salsa interns a tuple to key on, which costs an interned entry per
distinct argument combination and leaves salsa unable to name the entity in the
`WillExecute` event it reports.

`resolve_module_names` used to take `(module, parsed)`. Keying it on `module`
alone and fetching the parse inside removed 24 interned tuples (2112 bytes for a
24-module world) and made its events attributable to modules.

The limit is real, though: `typecheck_module` and `lower_module` take six or
more arguments including graph-level data that is not reachable from a module.
Restructuring compiler phase signatures to serve test instrumentation is the
wrong trade, and they were deliberately left alone.

### Backdating compares the value, not the dependencies

A query that reads something which changed re-executes. Whether its *consumers*
re-run is decided by whether what it returned compares equal -- so **adding a
dependency does not stop a query backdating, and removing one does not make it
backdate.** What matters is the shape of the return value.

This is easy to get backwards. `script_module_names` returns a module's
declarations, and a module body edit changes its spans, so it re-executes and
then backdates: the declarations are unmoved. Its predecessor returned a whole
`ModuleSpec` with the spans *in it*, and that could not backdate. The fix was to
narrow the value, not to read less.

An attempt to falsify that by reading the spans and discarding them did nothing,
correctly: the query re-ran and still backdated. The experiment that works is to
put the spans back in the returned value, or to revert the change and keep the
tests.

### A whole-world value in a per-unit key: the one that keeps coming back

Six times now, in six shapes, always the same mistake: a query about one entity
keyed on something describing the whole program, so a change anywhere moves the
key and the entity re-runs for an edit it cannot see.

| where | the aggregate in the key | replaced by |
|---|---|---|
| `resolve_module_imports` | `AllModuleExports` | per-module export lookup |
| the rider stubs | `ParsedModuleGraph` | `rider_interfaces`, keyed on `RiderSources` |
| rider `IrModuleId`s | the module's graph position | `ir_module_ids`, riders numbered first |
| script units | `accumulated` diagnostics | per-unit reads |
| script lowering | `accumulated_lower_bindings` | `UnitLowerRecord` folded per position |
| `typecheck_script_unit` | `ScriptEnv` over `Vec<ModuleSpec>` | `Vec<Module>` plus `script_module_spec` |

**The last one is the clearest statement of the rule, because the fix was a type
change and nothing else.** A `ModuleSpec` holds a module's path, source, spans,
parse, id and name resolution, and `ScriptEnv` -- which every per-unit script
query is keyed on -- interned over a `Vec` of them. So editing any module's body
gave every script unit a new `(script, env)` pair to answer, whether or not it
imported from that module. A `Module` is interned over a `ModuleId` and a
`Source`, and `set_text` changes a source's *text* and not its *handle*, so an
env of `Module`s survives a module edit where an env of specs could not. The
content is reached through `script_module_spec(db, module)`, per module, which a
unit asks for the one module its import named.

The test that catches this shape: **is anything in this key describing something
the query is not about?** A script unit is not about the module set, so nothing
about the module set beyond the handles belongs in its key. And note what does
*not* catch it -- counting executions. The same query instance re-running is
harmless; it is the key moving that costs, and a fixture recording "was this
module typechecked" sees nothing. `QueryRecorder`'s salsa ids do, if the test
attributes an event to an entity by the key a cold run used and treats an
unrecognised key as a failure rather than something to skip.
`script_scenario_tests` does that.

## The firewall: split a query so edits stop early

The most effective memoization tool here is a cheap projection in front of an
expensive one.

`parse_module_full` returns statements and the span table together.
`parse_module_ast` projects out just the statements, **deriving from
`parse_module_full` rather than parsing again**. Name resolution reads the
projection. So:

| edit | full parse | projection | name resolution |
|---|---|---|---|
| blank line at end (moves no span) | runs | backdates, not asked for | not run |
| blank line at start (moves every span) | runs | runs, returns equal value | not run |
| `ret 1` to `ret 99` | runs | runs, returns equal value | not run |
| rename a function | runs | runs | **runs** |

The literal case works because statements compare by tracked-struct identity and
the literal lives in a tracked field, which typechecking reads and name
resolution does not.

Write the projection the other way round and it parses the source a second time:
twice the parsing work, invisible to any fixture that records parsing as a
yes-or-no per module. `parse_firewall_tests.rs` pins all four rows, and the
first row is what catches the duplicate parse.

## Not everything wants to be a tracked struct

A tracked struct costs an id, a page slot and revision metadata per instance.
That is worth paying for identity. It is not worth paying for a value nothing
looks up by identity.

`Token` was a tracked struct, one per token: 1032 of them, 66 KB, the largest
single thing in the database for 24 four-line modules. As a plain value in the
`Vec` a `ChunkLex` already held, the same tokens cost 50 KB, and a tracked
function that memoized `text.contains("\n")` *per token* became a direct call.

Backdating was unaffected, which was worth checking rather than assuming: a
`Vec` of values compares by value where a `Vec` of ids compared by id, so this
could have spread span-shift invalidation. It did not, because the firewall that
spares name resolution sits at `ParsedStatements`.

The test for "should this be tracked" is: **does anything look it up by
identity, or does it only ever get read out of a collection?**

## Determinism: `BTreeMap` where it is iterated, `HashMap` where it is not

`HashMap` iteration order comes from a seed drawn afresh in every process. For a
compiler that means the same input can produce different output.

This was not hypothetical. Compiling one file four times produced four different
object files. The causes, all found by walking hash collections:

- `ModuleFunctionRegistry` was a `HashMap`, and the Cranelift backend walks it
  to declare functions. `declare_function` hands out `FuncId`s in call order, so
  the identifiers themselves varied.
- Type descriptors were collected into a `HashSet` and emitted in set order.
- The C backend sorted them by `type_depth` with `sort_by_key`, which is
  **stable**, so equal-depth types kept set order, and every primitive is depth
  zero.
- Consts were flattened with `or_insert_with`, so when two shared a short name
  the winner was whichever hash order reached it first.

The rule: **`HashMap` for lookup, `BTreeMap` or a sorted `Vec` for anything
iterated.** About 116 lookup-only hash maps are left alone and should be.

This is also why `InternedText: Ord` exists, via a chain worth knowing:

1. `ExprTypes` and friends are untracked fields of tracked structs, so they are
   part of the identity hash and must implement `Hash`.
2. `std::collections::HashMap` does not implement `Hash` and cannot, since its
   iteration order is unspecified.
3. `BTreeMap` does, but requires `K: Ord`.
4. `ExprKey` derives `Ord` and contains `Option<InternedText>`.
5. `Ord::cmp` gets no database, so the only thing left to compare is the id.

Swapping those to `HashMap` would free `InternedText` from needing `Ord`, and it
would be a mistake: `BTreeMap` is what keeps a memoized value's contents
order-stable.

Reproducibility cannot be tested in one process, because the seed is fixed
within a run and a hash map iterates identically every time. `datalove-cli/tests/reproducible_build_tests.rs`
compiles a fixture three times in **separate processes** and compares the bytes.

### Hashers

Salsa already hashes with `rustc_hash::FxHasher` internally. Replacing std's
SipHash in our own maps was measured: SipHash is 1.4 to 2.4 percent of runtime
by phase, and switching the lot to `ahash` bought about 1 percent overall for a
44-file diff. It was tried and backed out. Hashing is not where the time goes.

## Durability

Inputs that will not change during a session should say so. `datafun-pkg/src/package.rs`
marks system library sources `Durability::HIGH` and local sources
`Durability::LOW`. High-durability inputs let salsa skip whole subtrees when
validating.

This also interacts with interned garbage collection: salsa only reuses interned
slots at `Durability::LOW`, after `DEFAULT_REVISIONS` (3) have passed.

## Measuring

**What ran.** `datalove-ct/src/query_events.rs` provides `QueryRecorder`, which
listens for `WillExecute` and records every query salsa executes, with the salsa
id of its argument. `ModuleKeys` maps those ids back to module paths. This needs
no annotation and sees every query on every thread.

```rust
let recorder = QueryRecorder::new();
let db = Database::recording(&recorder);
// ... compile ...
recorder.clear();
// ... compile again ...
assert!(recorder.take().is_empty());
```

Attribution only works for queries keyed on a single entity; multi-argument
queries key on an interned tuple that cannot be mapped back.

**The older mechanism.** The `module_memo` fixtures answer a narrower question
(which modules were parsed, resolved, typechecked, lowered) using 12 hand-placed
`log_query` calls, spread over 6 of the 94 tracked functions. It is thread-local, so anything
rayon runs is invisible, and a query nobody annotated does not exist to it.

Its failure mode is worth knowing, because it has bitten twice. Moving the parse
queries to another crate left their `log_query` calls behind, and all 16 fixtures
reported a regression; nothing about memoization had changed. Then package
resolution was made to share phase 1's parse, which moved the parse *earlier* --
before the point at which the harness called `enable_query_logging` -- so by the
time the window opened every parse was a memo hit and all 16 fixtures reported
every module as unparsed. Again nothing had regressed; the work had moved out of
the window.

**If the fixtures move as a group after a refactor, check the instrumentation
before believing them**, and check where the window starts as well as where the
`log_query` calls are. A hand-placed log measures a *place*, so it breaks when
work moves between places. `QueryRecorder` measures execution and does not.

**Memory.** `<dyn salsa::Database>::memory_usage(&db)` reports per-ingredient
counts and bytes. It sizes fields by their stack size, so a `Vec` looks like
three words whatever it holds; declare `#[salsa::tracked(heap_size = f)]` to
count the rest. `ChunkLex` does, and without it the tokens would appear free.
`database_memory_tests.rs` includes a test whose only job is to check the bytes
are still counted, so that "no `Token` structs" cannot pass by hiding them.

**Time.** Run the variants interleaved in one session -- A, B, A, B -- and
compare medians. A block of A followed by a block of B measures the machine as
much as the code: the parser profile saw one binary range from 8.9ms to 10.9ms
in a session, and a change that read as 8% in blocks was nothing interleaved.
`datalove-bench` has the front-end benches; build a fresh database per
iteration outside the timer, or you measure a memo hit.

## Parallel execution

`DbClone` lets a database be cloned across threads. Clones share `Arc<Zalsa>`,
the global memo state, and get their own `ZalsaLocal`. The pattern is to warm
the cache in parallel and then let a sequential tracked function aggregate from
cache hits:

```rust
work.into_par_iter().for_each(|(db_clone, module)| {
    let _ = typecheck_module(db_clone.as_salsa_db(), module, ...);
});
typecheck_module_graph(db.as_salsa_db(), parsed_graph)
```

Enabled with `DATALOVE_PARALLEL=1`.

## Pitfalls in short

- **Tracked structs can only be created inside tracked functions.** Return plain
  data from helpers and build the struct at the tracked boundary. `TypeFunction`
  is one of these; passing raw strings down the pipeline and interning inside
  the tracked function is the workaround used here.
- **`ModuleId::new()` twice gives two different ids** when `ModuleId` is an
  input. It is interned now, so equal paths give equal ids, but the general
  point stands for inputs.
- **A tracked function taking the whole graph invalidates on any module.** Key
  per-module functions on the module.
- **`no_eq`** is used three times, in `bcts/src/chunks.rs` and
  `bcts/src/source_map.rs`. It suppresses the equality check that backdating
  relies on, so add it only when equality is genuinely meaningless or too
  expensive, and say why.
- **A tracked field read is a table lookup**, though it looks like a field
  access. Read `#[returns(ref)]` fields once and carry the `&'db` slice into
  hot loops; the lexer's `peek` and the bracer's iterator each re-read theirs
  per character or token, and fixing that was the largest win in both
  front-end profiles.
- **Interning hashes every occurrence.** Salsa deduplicates, but only after
  hashing into its sharded map, so interning a stream where a few dozen
  strings recur tens of thousands of times (the lexer's whitespace and
  sigils) is costly. Intern from a `&str` rather than building a `String`
  first (the key need only be `HashEqLike` with the field), index fixed sets
  by enum, and put a local `FxHashMap<&str, _>` in front for the rest; see
  `bcts/src/lexer.rs`. And do not re-intern what a token already holds.
- **Do not put a crate under `#![allow(unused)]`.** `bcts` was, and it hid 41
  warnings including two `db` parameters that a refactor had made redundant.

## Checklist for a new query or struct

1. Does it need identity, or is it a value read out of a collection?
2. If tracked: which fields identify it, and which should be `#[tracked]`?
3. Can it be keyed on one entity rather than several arguments?
4. Is there a cheap projection that would stop edits before the expensive part?
5. Does anything iterate a collection inside it? If so, is that collection
   ordered?
6. Are you storing a `salsa::Id`? Use a key of your own instead.
7. How will you tell whether it memoizes? Write the measurement, then break the
   code and check the measurement notices.
