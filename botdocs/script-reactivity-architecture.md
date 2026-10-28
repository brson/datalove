# The responsive scripting environment

How editing a script unit compiles again and reruns the units that depend on
it, and no others. Explanation; describes the engine as of October 2026.

A REPL session is a list of script units, and an edit to one of them changes
what the units after it would compute. Rerunning the whole session is
correct, but it costs every unit for a change most of them cannot see. Rerunning
only the edited unit is cheap, but its readers then hold stale values. **So
the engine re-derives the edit's reach**: the units that use, transitively,
what the edited unit provides. Its unit keys and lowering are built so that
nothing else moves. Linear ownership complicates this: a unit can take or
write an earlier unit's binding. So the reach also goes backward, to rebuild
such a binding before anything reads it again.

[script-reactivity-cases.md](script-reactivity-cases.md) is what the engine
should do, as rules and worked cases it is tested against.
[repl-architecture.md](repl-architecture.md) has the crates, the UI and the
request/response shape around it.
[plan-script-reactivity.md](plan-script-reactivity.md) is the plan that built
it, with the designs that were tried and rejected.
[script-glossary.md](script-glossary.md) defines the terms.

## Contents

- [The claim](#user-content-the-claim)
- [The shape of it](#user-content-the-shape-of-it)
- [1. A unit's key is its position, not its history](#user-content-1-a-units-key-is-its-position-not-its-history)
- [2. The environment is read, not seeded](#user-content-2-the-environment-is-read-not-seeded)
- [3. Lowering folds records, rather than mutating one](#user-content-3-lowering-folds-records-rather-than-mutating-one)
- [4. Re-derivation is a walk, and the runtime is already per-unit](#user-content-4-re-derivation-is-a-walk-and-the-runtime-is-already-per-unit)
- [5. The reach goes backward](#user-content-5-the-reach-goes-backward)
- [Splices](#user-content-splices)
- [Crossing a database change](#user-content-crossing-a-database-change)
- [Units that did not run to their end](#user-content-units-that-did-not-run-to-their-end)
- [What a step reports](#user-content-what-a-step-reports)
- [How it is validated](#user-content-how-it-is-validated)
- [What is not covered, and what is known wrong](#user-content-what-is-not-covered-and-what-is-known-wrong)

## The claim

Units A B C D, where C uses nothing B provides. Editing B compiles and reruns
**B and D**. A is upstream and untouched. **C is untouched even
though it sits between them.**

Editing a module reruns the units that import from it, and the units those
reach. A module edit that changes only a function body re-typechecks no unit.

**Unless a unit took or wrote what it reads.** A unit moves out of an earlier
unit's binding as it would out of its own, and `set` writes into an earlier
unit's `var`. Then the binding is no longer as the units that read it found
it, and rerunning one of them means building the binding again first. So if
D moved `a` out of A, editing B reruns A, B and D. See
[5. The reach goes backward](#user-content-5-the-reach-goes-backward).

## The shape of it

```
Script (interned cons list)          the identity everything is keyed on
  prev: Option<Script>               "this unit and the ones before it"
  unit: ScriptUnit (interned)
          source: Source             <- the unit's text; set_text edits it
          is_expr

ScriptEnv (interned)                 what the script is checked against
  modules: Vec<Module>               <- handles; a Module survives set_text
          script_module_names(module) -> what it declares, per module, tracked
  data: BTreeMap<String, Source>

per unit, from typechecking:         ScriptUnitTypecheckOutput
  new_vars / new_fns / new_fn_asts   what it provides
  new_module_aliases / new_consts
  uses                               every name it asks for
  imported_modules                   which modules it reaches

per unit, from lowering:             UnitLowerRecord
  exports, value_types, slot_types   what later units can name
  consts, dead_exports, functions
  provides, uses, imports            its edges, as owned text
  takes, writes, overwrites,         what it did to earlier units' bindings
  revives

runtime, per unit:                   FrameStore, UnitFunctionRegistry
  frames[i], unit_end_values[i], unit_end_slots[i], unit_functions(i)
```

`Script` and `ScriptUnit` are in `crates/datalove-datafun-ast/src/script.rs`,
the typecheck queries in `crates/datalove-datafun-tycheck/src/api.rs`,
`UnitLowerRecord` in
`crates/datalove-datafun-compiler/src/tracked_script_lower.rs`, and the
compiler that drives them in
`crates/datalove-datafun/src/pipeline/script_compiler.rs`.

Five things carry the design, and each is worth understanding on its own.

### 1. A unit's key is its position, not its history

`typecheck_script_unit(db, script, env)` is keyed on two interned handles. A
`ScriptUnit` is interned over the unit's `Source`, and a `Source` is a
`#[salsa::input]`. Editing a unit is `set_text`, which changes a source's
*text* and not its *handle*. So every `ScriptUnit`, every `Script` node and
every key stays where it was across an edit.

The design turns on that property. A key holding anything about the earlier
units' outputs would move whenever B's outputs did, re-keying C whether or not
C mentioned them. That is a whole-prefix value in a per-unit key; see
[salsa-patterns.md](salsa-patterns.md).

**`ScriptEnv` is the other half of the same key, and it holds handles for the
same reason.** A `Module` is interned over a `ModuleId` and a `Source`, so a
module edit leaves it where it was. A unit reaches a module through
`script_module_names(db, module)`, for the module its import named, and that
query returns only the module's declarations. A body edit leaves them equal,
so the query backdates and no importing unit re-typechecks.

The env does move when the module *set* changes, which is a different world
and so a different question: an import may resolve differently in it.

### 2. The environment is read, not seeded

`TypeContext` is not given the earlier units' bindings. On a lookup miss it asks:

```
binding_at(db, script, env, name)     -> the nearest earlier unit providing it
module_alias_at(db, script, env, name)
type_alias_at(db, script, env, name)  -> the nearest earlier `type`
unit_provides(db, script, env)        -> the projection those walk
unit_ast(db, unit)                    -> parse, spans
```

A fragment's names -- its aliases and function signatures -- are resolved in
`typecheck_script_unit` and not in `unit_ast`, because a signature or an
alias may name a type an earlier unit declared.

**Salsa records the dependency because the dependency is the read.** There is
no key to compute in advance and nothing to keep in step. Editing B reruns
`binding_at` for the units after it, and it backdates unless B provides that
name, so C is a memo hit and D is not.

The alternative, keying on the resolved uses, is circular: a unit's uses are
only known after typechecking it.

`is_const_binding` is among the lazy lookups, because const-ness across a unit
boundary decides whether a function body may name a binding. Entering a
function body sets `TypeContext::in_function_body`, which gates the inherited
lookup to consts only. **A function body is not a closure**, and since nothing
is seeded, the rule lives in the fallback.

### 3. Lowering folds records, rather than mutating one

`UnitLowerRecord` holds what one unit produced. Unit *i*'s context is built from
the records before it:

```
lower_context_over(&records[..i])
script_consts_over(&records[..i])
dead_externals_over(&records[..i])
```

The phases after typechecking -- ownership, lowering, const evaluation,
instantiation, IR assembly -- take an index and build their context from a
*position*. `compile_unit_at(index)` is the one driver, used by appending and
by re-deriving alike. Re-lowering a unit replaces its record and leaves the
others alone.

### 4. Re-derivation is a walk, and the runtime is already per-unit

```
relower_reach(edited)            -> seeds = {edited}
relower_module_reach(paths)      -> seeds = module_importers(paths)
   both -> rederive_reach(seeds) -> reach_from, compile, repeat -> rerun
```

`reach_from` takes the transitive closure over the name edges -- a unit is
reached if it asks for a name a reached unit provides -- and returns it in index
order. **The reach is taken over the union of the pre- and post-edit graphs**,
because either alone is unsound. A binding the edit *removes* is absent from
the new graph, though its reader must be told. One it *introduces* is absent
from the old. The alias edge, from a `require` to an `import` that uses its
alias, is added by hand: aliases resolve through `module_alias_at`, which
`uses` does not record.

`FrameStore` is partitioned by unit, so rerunning one is
`ScriptExecutor::rerun_unit`. That runs the unit, then puts the new frame at
its index and destroys the old frame's `unit_end_values` and `unit_end_slots`.
The unit's functions in the `UnitFunctionRegistry` are swapped at the same
time (`ScriptEnvironment::replace_unit`). The order is safe because a unit's IR
holds no reference to itself.

**Why leaving C alone is sound.** A script value is identified by
`(unit_index, ValueId)`, so a unit's IR names the units it reads from. A unit
that uses nothing of B's holds no `(B, *)` reference, so rebuilding B's values
cannot reach it. An edit leaves the indices alone, which keeps the
identification good. `c_holds_no_reference_to_b` checks this by walking a
unit's serialized IR.

That holds for C's *references*. It does not by itself hold for the state C
reads. A value C took is in C's own frame. But a later unit may have since
taken or written a binding C reads, and the same goes for a binding a rerun
unit reads. That is what section 5 is for.

A module edit needs one more thing. A unit names a module function by
`CodeRef::Module`, resolved against the `ModuleFunctionRegistry` the executor
holds, so re-lowering alone changes nothing that *runs*. `set_module_registry`
swaps it, down through `FunctionRegistry`, `ScriptEnvironment` and
`ScriptExecutor`. It also calls `forget_compiled_bodies`, which tells a
`CallDispatcher` that the bodies it compiled may be stale; a rerun or
truncated unit does the same. The REPL runs without a dispatcher.

### 5. The reach goes backward

A unit's record says which earlier units' bindings it **took** (moved out of)
and which it **wrote** (by `set`, or through a `mut` or `out` argument). Both
are things the unit did to state that is not its own. Both break the
assumption that rerunning a unit sees what it saw the first time: a taken
binding holds nothing, and a written one holds what the write left.

So `reach_from` is two rules, repeated until nothing is added:

- **Forward**, with one more kind of edge than section 4: a unit that took or
  wrote a name counts as providing it, since the units after it see the binding
  as it left it.
- **Backward.** Take a reached unit that uses a binding which it, or a unit
  after it, took or wrote. It would read the binding changed, so the unit that
  rebuilds the binding, where the rerun starts, is reached too. A unit between
  that start and the reader that changed the binding is left alone, since what
  it left is what the reader read.

A unit reached by the backward rule is **rerun**, not re-derived. Its code
and its inputs are the same, so it builds the values it built before. A unit
that only reads them would compute what it already holds, so the forward rule
does not reach it from a rerun unit. What does have to run again is every
unit that took or wrote what the rerun unit builds, in index order, and
those rerun in turn. A unit reached both ways is re-derived.

Leaving a reader's frame alone while its provider's is replaced is sound for
the reason in section 4. It also needs a borrow to never outlive the expression
it is in, which holds, so a frame has no pointer into another unit's frame.
Tensor views alias their parent, but one cannot be bound with `let`, only by a
`for` over the rows, inside the loop.

The backward rule is taken over what the units did *before* the edit, the records
as `held_graph` finds them, because that is what the running session went
through. The forward rule needs what they do *after* it as well. An edit that
makes B take `a` reaches the later units that use `a`, but that is only known
once B is compiled. So `rederive_reach` takes the reach, compiles it, and takes
it again with the new records, until it stops growing.

**Overwrites and revivals narrow it.** `UnitLowerRecord::overwrites` is the
part of `writes` the unit writes on every way through it, early returns
included, before reading what the binding held. Ownership analysis decides it
with the machinery that checks an `out` parameter is written, treating an
earlier unit's `var` as starting out "not yet replaced". A revival, a write to
a binding that was given away, cannot read what it held either, so it counts
the same. The reach uses them three ways:

- A reached unit that writes a binding without reading it needs nothing of
  what the binding held, so it does not reach back for it.
- Nor is it re-derived because an earlier unit changed the binding, since only
  what the binding is matters to it, and its provider says that. If its write
  was retracted, it reruns.
- The unit a rerun starts from, to rebuild a binding as a reader finds it, is
  the nearest unit before the reader that sets it without reading it: its
  provider, or an overwrite or revival after that. So a rerun starts from the
  last overwrite rather than from where the `var` was made.

Nor is a unit re-derived because a unit before it that takes a binding was. If
the take is there before the edit and after it, the binding holds nothing after
the edit either way.

The records the reach reads are the units' current ones, so an edit that makes
a unit overwrite what it used to update shrinks the reach once the unit is
compiled. `rederive_reach` iterates until it settles, either way.

These are reruns, and they are what make the reach sound without copying
anything. The cost is running the rerun's start again, which may be the
expensive unit of a session: a chain of units that each take the previous
unit's value reruns from its head when its tail is edited.

It rests on a unit's record saying what it took and wrote. A unit that compiled
and did not run to its end -- returned early, or was blocked -- keeps its
record, so it is assumed to have taken what it would take. The cost of that is
rerunning its provider for nothing; see
[issues.md](issues.md#user-content-a-unit-that-did-not-run-still-gives-away-what-it-would-have-taken).
Retracting a revival also does more than it needs to: it reruns the provider
and the take after it, where a fixup, emptying the binding, would do. See
[script-reactivity-cases.md](script-reactivity-cases.md#user-content-where-the-engine-does-more-than-the-rules).

## Splices

`truncate_units`, `delete_unit` and `insert_unit` change the list rather than a
unit, and that moves indices. Since a script value is identified by
`(unit_index, ValueId)`, splicing at *i* renumbers everything after it, and
every later unit's IR has to be rebuilt. That is inherent, not a shortcoming of
the walk: the ids really did change. **So the splices re-derive the whole
suffix rather than the reach.** `the_whole_suffix_is_rederived_not_just_the_reach`
asserts that against a graph-derived reach that is strictly narrower.

All three are one mechanism: splice the unit list, rebuild the `Script` chain,
destroy and discard the suffix's runtime state, then re-derive the suffix.
Truncating is the cheap one, and the only one that renumbers nothing, because
nothing sits after what goes. It re-derives nothing at all, unless a unit that
goes took or wrote an earlier binding.

That is the rerun of section 5 again. A unit that goes, or runs again as part
of the suffix, may have taken or written a binding before the splice point.
`rerun_start` moves the start of the splice back to that binding's provider,
and further back while the units in between changed something earlier still.
The splice re-derives from there, so `truncate_units` returns a `Splice` like
the others. A splice does not skip readers: it reruns every unit from the
rerun start on. `rerun_start` does not yet use the overwrite rules, so it
starts at the provider even when a nearer unit overwrites the binding.

Two things each splice needs that an edit does not:

- **The suffix runs as appends**, not in place. `ScriptExecutor::truncate_units`
  destroys the frames and the unit functions from the rerun start on, and the
  re-derived units take the next free index each. Each is run by
  `execute_fragment` or `execute_expr`, chosen by the unit's `is_expr` flag.
  Frames and functions have to go together because appending pushes a unit's
  functions on the end rather than writing them at an index. A registry left
  long would put the next unit's functions at a dropped unit's index.
- **The same `Source` handles go back in** for the units a splice does not
  touch. `Source::new` would mint a new one and move every memo key from there
  on.

**A unit of the suffix that stops compiling stays**, as an appended unit that
fails does (see [Units that did not run to their end](#user-content-units-that-did-not-run-to-their-end)).
Removing a unit that a later unit reads leaves the reader failing, and putting
the binding back makes it compile and run again.

`ScriptCompiler::{truncate_units, delete_unit, insert_unit}` are the compiler's
verbs, and `Engine::{truncate_units, delete_unit, insert_unit}` the shipped
ones.

## Crossing a database change

Editing is `set_text` on a unit's `Source`, which needs `&mut db`, so nothing
holding `'db` data can be alive across it. `ScriptSession` is the plain-data
half: the units' sources, the per-unit records and the flags.
`CompiledModules::script_compiler_resumed` rebuilds a compiler over it
afterwards. Interning the same sources gives back the same `Script` handles, so
the memos survive the round trip. That is also why `UnitLowerRecord` holds its
edges as owned strings: the reach is walked after the change.

`Engine` owns its `Database` rather than borrowing one, which makes the setter
reachable, and builds a compiler per operation. The entry points are
`Engine::edit_unit(unit, source)`,
`Engine::edit_module(library, package, module, source)`,
`Engine::truncate_units(n)`, `Engine::delete_unit(i)` and
`Engine::insert_unit(i, source, is_expr)`.

## Units that did not run to their end

An edit can break a unit, and the edit that fixes it can be anywhere upstream.
So **a unit that fails stays in the session**, holding its index, and is
re-derived like any other when an edit reaches it. A unit fails in one of three
ways, and `ScriptExecutor::unit_status` says which:

- `NotCompiled` -- it did not parse, or did not typecheck, or failed in
  ownership analysis or lowering. A unit that did not parse is never
  typechecked: `unit_provides` answers for it from `unit_parses` alone, and the
  script compiler does not ask for its typecheck, so it provides nothing and
  asks for nothing. `typecheck_script_unit` itself still checks a broken parse,
  which the typechecker's own recovery tests rely on.
- `ReturnedEarly` -- it ran and left its top level through `!` or `?`.
- `Blocked { on }` -- it compiled, and was not run because it reads from units
  that did not run to their end.

**Only a unit that ran offers its bindings.** Each layer enforces the rule for
the failures it can see:

- A unit that failed to typecheck provides nothing to `binding_at`, so later
  units resolve past it to an older binding of the name, or to nothing.
- A unit that typechecked and failed later leaves an empty `UnitLowerRecord`,
  so the typechecker sees its names and lowering does not. The later unit fails
  to lower, and `lowering_failure` reports it as "`x` is bound by unit N, which
  did not compile".
- A unit that failed at run time keeps its full record, because typecheck and
  lowering cannot see run-time outcomes. The later units compile against it
  and the executor refuses to run them. `IrCodeUnit::script_units_read` lists
  every unit a unit's code names -- external operands, external slot stores,
  cross-unit calls, its functions' bodies included -- and a unit that reads
  one whose status is not `Ran` is `Blocked`. A bare-name expression unit reads
  its binding outside its code, so its provider counts too.

Blocking rather than hiding the names keeps the static view honest: the later
unit keeps its types, and it is in the reach of the failed unit because it uses
names that unit provides. When the failed unit goes green, its dependents are
re-derived with it and run.

**The numbering must not have a hole in it**, so a unit that did not run takes
its index with no frame. `FrameStore::frames` is `Vec<Option<ScriptFrame>>`.
`ScriptExecutor::skip_unit` appends an empty one, and `clear_unit` empties one
an edit broke, destroying what it owned. An early-returned unit keeps its frame,
which holds whatever it bound before the return, until it runs again or the
session ends. The executor folds every unit's exports, so a name a failed unit
binds still shadows an earlier one, as it does in the compiler. `get_binding`
and `get_environment` leave out names whose nearest binding did not run.

**The compiler and the executor have to number units alike.** Every caller
that drives the two side by side has to hand the executor every unit the
compiler keeps, a failed one through `skip_unit`. The script compiler stamps
each unit's index in `ScriptContext::unit`, and the executor asserts it on every
append and rerun. A caller that dropped a failure would otherwise read
the wrong frames rather than panic.

A runtime error is not one of these. The runtime discards all of its memory
(`IrInterpreter::discard_runtime_state`), so the session starts again.

## What a step reports

Every operation on the units -- an append, an edit, a splice, a module edit --
says what it did to each unit it touched, in a `UnitStep` per unit:
`compiled`, which compile passes salsa ran for the unit, and `eval`, what
re-deriving it came to, or `None` if it was not re-derived. The edits return
these, and `Engine::last_step` holds them for any operation, the append
included. The two halves are independent. A unit can be compiled again and not
run again: its typecheck runs again because what it is checked against moved,
and comes out the same. And one can run again and not be compiled again: a
rerun to restore a binding for another unit, or one whose input values changed
and whose types did not. So a step leaves each unit unchanged, compiled again
and not run, compiled again and ran again, or ran again.

**Compiled means the parse, the typecheck or the ownership analysis ran.**
Lowering is re-derivation and not counted. An ownership analysis can run with
no typecheck before it, since it is keyed on what earlier units left behind as
well as on the unit's types: edit a unit so that it no longer gives a binding
away, and a later unit that writes the binding is compiled again while its
typecheck is memoized.

**The engine hears it from the queries themselves.** The entry query of each
pass records itself at the top of its body, through
`datalove-ct/src/unit_analysis.rs`, which salsa only runs when it executes the
query: the parser's `parse` and `parse_expr`, `typecheck_script_unit`, and the
two ownership queries. Each names its unit by `Source`: the parse by its key,
the typecheck by its `Script` argument, and the ownership analysis by the
`Source` it is also keyed on, which it needs for the spans its diagnostics point
at. The engine collects them around each
operation and maps the sources to the units after it. The log is a
thread-local and no salsa event callback is installed, so a query nothing
records costs nothing, and a recorded one a thread-local read. It sees only
the engine's thread, which is where script units are compiled.

The parse is recorded in the parser and not in the typechecker's
`unit_parses`, because salsa runs `unit_parses` again for every unit on every
edit -- it reads the parse's accumulated diagnostics -- while the parse itself
runs only for the unit whose text changed.

## How it is validated

**The technique.** Ask how far downstream a change travelled and compare it
against the dependency graph. `QueryRecorder`
(`crates/datalove-ct/src/query_events.rs`) records what salsa executed.
Expectations are **derived from the graph** -- `uses` intersected with
what the edited unit provides, transitively -- rather than written down. A
separate test pins that the fixture has the shape the claim is about.

| suite | what it holds |
|---|---|
| `datalove-datafun/tests/script_graph_tests` | provides and uses per unit: the graph itself |
| `datalove-datafun/tests/script_reactivity_tests` | compile reach, at the typecheck layer |
| `datalove-datafun/tests/script_exec_reactivity_tests` | lowering and execution reach, values through an executor |
| `datalove-datafun/tests/script_scenario_tests` | the edit-kind matrix, the module cases, a twelve-unit session, blanking, ownership and reruns |
| `datalove-datafun/tests/script_const_tests` | consts across units and into function bodies |
| `datalove-datafun/tests/script_splice_tests` | truncate, remove and insert: the suffix, the values, a suffix that stops compiling |
| `datalove-script-engine/tests/engine_edit_tests` | the shipped entry points end to end, failed units going green, modules joining and breaking, reruns |
| `datalove-script-engine/tests/reactive_tests` | the step tables in `tests/fixtures/reactive/`, and that [script-reactivity-cases.md](script-reactivity-cases.md) shows them as rendered |

A step table says what each unit should do per step, with the engine's known
extra work annotated. There, a unit counts as recompiled if salsa ran its
typecheck, or ran an ownership analysis while compiling it
(`query_events::compiling_unit`). Every step also checks the engine's own
account of what it compiled against that, so a pass whose entry query does not
record itself fails the fixtures it changes; removing the ownership hook fails
every step where a unit's ownership analysis runs and its typecheck does not.
And it checks that no script unit's typecheck or ownership analysis ran on a
worker thread, where the engine's log cannot see it.

**Falsification is what makes it evidence.** Each piece was checked by
injecting a fault into the shipped code and confirming the tests reported the
wrong answer: a reach of one step, a reach widened to every later unit, one
narrowed to the edited unit, a `replace_frame` that does not destroy, a splice
that re-derives only the reach, a dropped backward rule, and so on. Several faults
failed only one or two tests apiece.

**Two faults found holes in the tests rather than in the code.** Making the
unit function registry's truncate a no-op failed nothing. Only a cross-unit
call after a truncation asks the registry, since a `CodeRef::Local` resolves
through the running frame's own list. `a_truncated_unit_s_functions_go_with_it`
is that call. And making `set_module_registry` a no-op left a binding reading
its old value, which the scenario matrix did not notice, **because it measures
reach and not values.** Both holes had the same shape: a correct piece of
machinery that no test could tell from a missing one. That is the argument for
injecting a fault per piece rather than per claim, and for asserting what a
program computed alongside how far an edit travelled.

**A trap in the instrument.** `QueryRecorder` answers "what executed". It
cannot answer "would this re-run if asked". Typechecking is lazy, so a unit
whose key moved but which nothing asked about shows as not re-run. Ask the
units directly when the question is about validity.

## What is not covered, and what is known wrong

- **The lowering reach is the compiler's own answer.** The phases after
  typechecking are plain functions with no per-unit query to record, so unlike
  the compile reach the lowering reach is not measured from outside. It is held
  to a graph-derived expectation and to the falsification runs.
- **A change to the module set re-keys every unit.** The engine narrows its
  modules to what the units require on every step (see
  [repl-architecture.md](repl-architecture.md#user-content-the-engine)), and a
  step that moves them interns a new `ScriptEnv`. Every unit's typecheck then
  has a new key. The units already lowered keep their IR, which is sound
  because module ids are given by path. But the reach for a set change has
  never been asked about, and no engine path adds a module source mid-session;
  see [issues.md](issues.md#user-content-nothing-consumes-a-workspacedelta).
- **A unit that returns early keeps what it did to earlier units.** A `set` or
  an `out` argument writes into an earlier unit's frame, a move takes from one,
  and an early return after either leaves it done. `var p = (1, "a")` followed
  by a unit that does `set p.1 = "mutated"` before a failing `!` leaves `p`
  mutated. So a failed unit does influence the session until something
  re-derives it, when the rerun of section 5 builds `p` again first. See
  [issues.md](issues.md#user-content-a-repl-unit-that-fails-keeps-what-it-did-to-earlier-units).
- **A unit that did not run is assumed to have done what it would do.** Its
  record lists what it would take and write. So a blocked unit makes the names
  it would take unusable after it, and its provider reruns for it, though
  nothing was taken.
- **Splices rerun from the provider, not the nearest overwrite**, and rerun
  every unit from there; see [Splices](#user-content-splices).
- **The engine keeps no history.** The REPL's `/undo` and `/redo` keep it,
  and need nothing new of the engine: undoing an edit *is* an edit, undoing an
  append is `truncate_units`, and undoing an insert or a delete is the opposite
  splice. See
  [repl-architecture.md](repl-architecture.md#user-content-undo-and-redo).
- **Only pure `fun`s exist**, so the only output is `debuglog`, and skipping a
  unit's execution skips nothing observable beyond it. `proc`s and real I/O
  need virtualized I/O, which does not exist yet.
