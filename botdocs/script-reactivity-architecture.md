# The responsive scripting environment

How editing a script unit re-analyzes and re-runs the units that depend on it,
and no others. This is the map of the machinery and how it is held to its claim.

[plan-script-reactivity.md](plan-script-reactivity.md) is the plan that built it,
organised by stage and keeping the wrong turns; read it for *why* things are
shaped this way and what was tried and rejected. This document is what the code
is. [repl-architecture.md](repl-architecture.md) has the crates, the UI and the
request/response shape around it.

## The claim

Units A B C D, where C uses nothing B provides. Editing B re-analyzes and
re-executes **B and D**. A is upstream and untouched. **C is untouched even
though it sits between them.**

Editing a module re-runs the units that import from it, and the units those
reach.

**Unless a unit took or wrote what it reads.** A unit moves out of an earlier
unit's binding as it would out of its own, and `set` writes into an earlier
unit's `var`. Then the binding is no longer as the units that read it found
it, and re-running one of them means building the binding again first. So if
D moved `a` out of A, editing B re-runs A, B and D. See
[5. The reach goes back](#user-content-5-the-reach-goes-back).

## The shape of it

```
Script (interned cons list)          the identity everything is keyed on
  prev: Option<Script>               "this unit and the ones before it"
  unit: ScriptUnit (interned)
          source: Source             <- the input; set_text edits a unit

ScriptEnv (interned)                 what the script is checked against
  modules: Vec<Module>               <- handles; a Module survives set_text
          script_module_spec(module) -> ModuleSpec, per module, tracked

per unit, from typechecking:         ScriptUnitTypecheckOutput
  new_vars / new_fns / new_fn_asts   what it provides
  new_module_aliases / new_consts
  asked_names                        what it uses
  imported_modules                   which modules it reaches

per unit, from lowering:             UnitLowerRecord
  exports, value_types, slot_types   what later units can name
  consts, dead_exports
  taken, written, revived            what it did to earlier units' bindings

runtime, per unit:                   FrameStore
  frames[i], unit_end_values[i], unit_end_slots[i]
```

Four things carry the whole design, and each is worth understanding on its own.

### 1. A unit's key is its position, not its history

`typecheck_script_unit(db, script, env)` is keyed on two interned handles. A
`ScriptUnit` is interned over the unit's `Source`, and a `Source` is a
`#[salsa::input]`: editing a unit is `set_text`, which changes a source's *text*
and not its *handle*. So every `ScriptUnit`, every `Script` node and every key
stays exactly where it was across an edit.

That is the property the design turns on. It used to be keyed on
`AccumulatedBindings` -- every binding from every earlier unit -- so B's outputs
moving re-keyed C whether or not C mentioned them. A whole-prefix value in a
per-unit key; the same shape has been fixed six times in this compiler.

**`ScriptEnv` is the other half of the same key, and it holds handles for the
same reason.** A `Module` is interned over a `ModuleId` and a `Source`, so a
module edit leaves it where it was; a unit reaches a module's parse and name
resolution through `script_module_spec(db, module)`, for the module its import
named. The env held every module's `ModuleSpec` at first -- spans included -- so
any module edit re-keyed every unit. That was the sixth instance of the shape.
The env still moves when the module *set* changes, which is a different world
and so a different question.

### 2. The environment is read, not seeded

`TypeContext` is not given the earlier units' bindings. On a lookup miss it asks:

```
binding_at(db, script, env, name)     -> the nearest earlier unit providing it
module_alias_at(db, script, env, name)
unit_provides(db, script, env)        -> the projection those walk
unit_ast(db, unit)                    -> parse, spans, name resolution
```

**Salsa records the dependency because the dependency is the read.** There is no
key to compute in advance and nothing to keep in step. Editing B re-runs
`binding_at` for the units after it and *backdates* unless B provides that name,
so C is a memo hit and D is not.

The alternative -- keying on the resolved uses -- is circular: a unit's uses are
only known after typechecking it.

`is_const_binding` is among the lazy lookups, because const-ness across a unit
boundary decides whether a function body may name a binding. And entering a
function body sets `TypeContext::in_function_body`, which gates the inherited
lookup to consts only: **a function body is not a closure**, and with nothing
seeded there is no longer a map to filter, so the rule lives in the fallback.

### 3. Lowering folds records, rather than mutating one

`UnitLowerRecord` holds what one unit produced. Unit *i*'s context is built from
the records before it:

```
lower_context_over(&records[..i])
script_consts_over(&records[..i])
dead_externals_over(&records[..i])
```

Phases 2 to 5 -- ownership, lowering, const evaluation, IR assembly -- take an
index and build their context from a *position*, where they used to run only for
the unit just appended against a mutable fold. `compile_unit_at(index)` is the
one driver, used by appending and by re-deriving alike.

### 4. Re-derivation is a walk, and the runtime is already per-unit

```
relower_reach(edited)            -> seeds = {edited}
relower_module_reach(paths)      -> seeds = module_importers(paths)
   both -> reach_from(seeds) -> rederive(units)
```

`reach_from` takes the transitive closure over the name edges -- a unit is
reached if it asks for a name a reached unit provides -- and returns it in index
order. **The reach is taken over the union of the pre- and post-edit graphs**,
because either alone is unsound: a binding the edit *removes* is absent from the
new graph though its reader must be told, and one it *introduces* is absent from
the old.

`FrameStore` was already partitioned by unit, so re-executing is
`replace_frame`: destroy that unit's `unit_end_values` and `unit_end_slots` after
the new run succeeds, then put the new frame at its index. Safe in that order
because a unit's IR holds no reference to itself.

**Why leaving C alone is sound.** A script value is identified by
`(unit_index, ValueId)`, so a unit's IR names the units it reads from. A unit
that uses nothing of B's holds no `(B, *)` reference, so B's values being rebuilt
cannot reach it. Editing rather than removing leaves the indices alone, which is
what keeps the identification good. `c_holds_no_reference_to_b` checks this by
walking a unit's serialized IR rather than by argument.

That holds for C's *references*. It does not by itself hold for the state C
reads: a value C took is in C's own frame, but a binding C reads may have been
taken or written since by a later unit, and so may one that a re-run unit
reads. That is what section 5 is for.

A module edit needs one more thing: a unit names a module function by
`CodeRef::Module`, resolved against the `ModuleFunctionRegistry` the executor
holds, so re-lowering alone changes nothing that *runs*. `set_module_registry`
swaps it, down through `FunctionRegistry`, `ScriptEnvironment` and
`ScriptExecutor`.

### 5. The reach goes back

A unit's record says which earlier units' bindings it **took** (moved out of)
and which it **wrote** (by `set`, or through a `mut` or `out` argument). Both
are what the unit did to state that is not its own, and both break the
assumption that re-running a unit sees what it saw the first time: a taken
binding holds nothing, and a written one holds what the write left.

So `reach_from` is two rules, repeated until nothing is added:

- **Forward**, as before, with one more kind of edge: a unit is reached when a
  name it uses resolves to a reached unit, and a unit that took or wrote a
  name counts as providing it, since the units after it see the binding as it
  left it.
- **Back.** A reached unit that uses a binding which it, or a unit after it,
  took or wrote would read it changed, so the provider is reached too. A unit
  between the provider and the reader that changed the binding is left alone:
  what it left is what the reader read. Running the provider again builds the
  binding again.

A provider reached by the back rule is **replayed**, not re-derived: its code
and its inputs are the same, so it builds the values it built before. A unit
that only reads them would compute what it already holds, so the forward rule
does not reach it from a replayed unit. What does have to run again is every
unit that took or wrote what the replayed unit builds, in index order, and those
are replayed in turn. A unit reached both ways is re-derived. Leaving a reader's
frame alone while its provider's is replaced is sound for the reason above, and
because a borrow cannot outlive the expression it is in: a frame holds no
pointer into another unit's frame. Tensor views alias their parent, but one
cannot be bound with `let`, only by a `for` over the rows, inside the loop.

The back rule is taken over what the units did *before* the edit, the records
as `held_graph` finds them, because that is what the running session went
through. The forward rule needs what they do *after* it as well -- an edit that
makes B take `a` reaches the later units that use `a` -- and that is only known
once B is compiled. So `rederive_reach` takes the reach, compiles it, and takes
it again with the new records, until it stops growing.

The splices need the same: a unit that goes, or runs again as part of the
suffix, may have taken or written a binding before the splice point.
`rewind_point` moves the start of the splice back to that binding's provider,
and on back while the units in between changed something earlier still, and
the splice re-derives from there. That is why `truncate_units` can re-derive
units and returns a `Splice` like the others. A splice does not skip readers:
it re-executes from the rewind point as appends, so every unit from the
provider on runs again.

**Overwrites need less.** `UnitLowerRecord::overwritten` is the part of
`written` the unit writes on every way through it before reading what the
binding held, early returns included. Ownership analysis decides it with the
machinery that checks an `out` parameter is written, an earlier unit's `var`
starting out "not yet replaced". The back rule uses it twice:

- A reached unit that overwrites a binding needs nothing of what it held, so
  it does not reach back for it at all.
- What builds a binding as a unit finds it is the nearest unit before it that
  sets it without reading it -- its provider, or an overwrite after that. If
  that unit is reached anyway, nothing more is needed; if not, it is the one
  rewound, not the provider. So a rewind runs from the last overwrite rather
  than from where the `var` was made.

The records the reach reads are the units' current ones, so an edit that makes
a unit overwrite what it used to update shrinks the reach once the unit is
compiled. `rederive_reach` takes the reach again until it settles, either way.
The splices do not use any of this yet: `rewind_point` still starts at the
provider. A revival, a write to a binding that was given away, is not an
overwrite here, so undoing one reruns the provider where emptying the binding
would do; see `mandocs/drafts/reactive-script-semantics.md`.

This is rewind and replay, and it is what makes the reach sound without
copying anything. The cost is running the provider again, which may be the
expensive unit of a session: a chain of lines that each take the previous
one's value re-runs from its head when its tail is edited.

What it rests on is that a unit's record says what it took and wrote. A unit
that compiled and did not run to its end -- returned early, or was blocked --
keeps its record, so it is assumed to have taken what it would take. Re-running
its provider for nothing is the cost of that; see `issues.md`.

## Crossing a mutation

Editing is `set_text` on a unit's `Source`, which needs `&mut db`, so nothing
holding `'db` data can be alive across it. `ScriptSession` is the plain-data half
-- the units' sources, the per-unit records, the flags -- and
`CompiledModules::script_compiler_resumed` rebuilds a compiler over it
afterwards. Interning the same sources gives back the same `Script` handles, so
the memos survive the round trip.

`Engine` owns its `Database` rather than borrowing one, which is what makes the
setter reachable, and builds a compiler per operation. The entry points are
`Engine::edit_unit(unit, source)`,
`Engine::edit_module(library, package, module, source)`,
`Engine::truncate_units(n)`, `Engine::remove_unit(i)` and
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

**Only a unit that ran offers its bindings.** The rule is enforced at the
layer that can see the failure:

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
its index with no frame: `FrameStore::frames` is `Vec<Option<ScriptFrame>>`,
`ScriptExecutor::skip_unit` appends an empty one and `clear_unit` empties one an
edit broke, destroying what it owned. An early-returned unit keeps its frame,
which holds whatever it bound before the return, until it runs again or the
session ends. The executor folds every unit's exports, so a name a failed unit
binds still shadows an earlier one, as it does in the compiler, and
`get_binding` and `get_environment` leave out names whose nearest binding did
not run.

**The compiler and the executor have to number units alike**, and every caller
that drives the two side by side has to hand the executor every unit the
compiler keeps -- a failed one through `skip_unit`. The script compiler stamps
each unit's index in `ScriptContext::unit`, and the executor asserts it on every
append and re-execution, because a caller that drops a failure would otherwise
read the wrong frames rather than panic.

A runtime error is not one of these: the runtime discards all of its memory
(`IrInterpreter::discard_runtime_state`), so the session starts again.

## How it is validated

**The technique.** Ask how far downstream a change travelled and compare against
the dependency graph, using `QueryRecorder` (`datalove-ct/src/query_events.rs`)
to record what salsa executed. Expectations are **derived from the graph** --
`asked_names` intersected with what the edited unit provides, transitively --
rather than written down, with a separate test pinning that the fixture is the
shape the claim is about. This technique has found six bugs that four older
suites missed.

| suite | tests | what it holds |
|---|---|---|
| `script_graph_tests` | 7 | provides and uses per unit: the graph itself |
| `script_reactivity_tests` | 6 | analysis reach, at the tycheck layer |
| `script_exec_reactivity_tests` | 17 | lowering and execution reach, values through an executor |
| `script_scenario_tests` | 23 | the edit-kind matrix, the module cases, a twelve-unit session, blanking, ownership and rewinding |
| `script_const_tests` | 11 | consts across units and into function bodies |
| `script_splice_tests` | 14 | truncate, remove and insert: the suffix, the values, a suffix that stops compiling |
| `datalove-repl/engine_edit_tests` | 21 | the shipped entry points end to end, failed units going green, rewinding |
| `datalove-repl/reactive_tests` | 2 | step tables in `tests/fixtures/reactive/`: what each unit should do per step, with the engine's known extra work annotated. A unit recompiled if salsa ran its typecheck, or ran an ownership analysis while compiling it (`query_events::compiling_unit`); and that `mandocs/drafts/reactive-script-semantics.md` shows them as rendered |

**Falsification, which is the part that makes it evidence.** Every stage was
checked by injecting a fault into the shipped code and confirming the tests
reported the *old* answer. Stage B against one; stage C against four -- a
one-step reach, a reach widened to every later unit, one narrowed to the edited
unit, and `replace_frame` with its destroy removed, which failed eight tests with
leak reports. The module work against seven. The module-edit narrowing against
six, one of them a straight revert of the three shipped files with the new tests
in place, which failed on the key attribution itself. Several of these faults
failed exactly one or two tests apiece.

The splice verbs against four. `FrameStore::truncate_units` shortening the
three parallel vectors without destroying failed seven of the thirteen splice
tests and four engine tests with leak reports -- and the six that stayed green
are the rejection cases and the no-op truncation, which correctly destroy
nothing. Re-deriving `reach_from(splice_point)` instead of the whole suffix
reported `{1}` where `{1,2}` is right and `{1,2}` where `{1,2,3,4}` is, and
**also broke the rejection policy**: a removal whose dependent sat outside the
reach was accepted, because nothing compiled the unit that would have failed.
Leaving `scripts` as it was rather than rebuilding the chain left the removed
unit's binding still in the environment, accepted that same removal, and
panicked on the insert cases where the chain was a node short of the records.

**The fourth found a hole in the tests, which is the second fault here to do
that.** A unit's frame and its functions sit at the same index in two places --
`FrameStore::frames` and `UnitFunctionRegistry::unit_functions` -- and making
the second truncate a no-op failed *nothing*: not the thirteen splice tests, not
the twelve engine tests, not the four other script suites. It is load-bearing
anyway, because **appending pushes a unit's functions on the end rather than
writing them at an index**, so a registry left long while the frame store is
short puts the next appended unit's functions at the dropped unit's index and a
cross-unit call lands on the function that was dropped. Nothing reached it
because a `CodeRef::Local` resolves through the running frame's own list, so a
call that stays inside one unit is blind to it; only a cross-unit call after a
truncation asks the registry. `a_truncated_unit_s_functions_go_with_it` is that
call, and under the fault it returns the dropped function's value.

The rewind against three: dropping the back rule failed the reading and moving
cases, the leak checker aborting on one of them; dropping the forward edge from
a unit that took or wrote a name failed nothing until
`a_unit_that_now_writes_a_var_reaches_its_later_readers` was written, the back
rule having reached the same units in every other case; and starting a splice
at its own point rather than `rewind_point` failed the truncate and remove
cases.

**Two faults found holes in the tests rather than in the code**, and both are
worth keeping. The registry truncate is above. The other: making
`set_module_registry` a no-op left a binding reading its old value, and **the
scenario matrix did not notice, because it measures reach and not values.** A
matrix over reach is not a substitute for asserting what a program computed.
Both holes had the same shape -- a correct piece of machinery that no test could
distinguish from a missing one -- which is the argument for injecting a fault
per piece rather than per claim.

**A trap in the instrument.** `QueryRecorder` answers "what executed". It cannot
answer "would this re-run if asked", and confusing the two produced a wrong
diagnosis of the module gap: no unit re-typechecked after a module edit, which
looked like the keys had not moved. They had -- nothing had asked, because
typechecking is lazy. Ask the units directly when the question is about validity.

## What is not covered, and what is known wrong

- **A module *body* edit still reaches the units that import from that module.**
  The whole-unit over-propagation is gone -- a module edit reached every unit
  until `ScriptEnv` stopped holding `ModuleSpec`s -- but a `ModuleSpec` carries
  the module's spans, and a body edit moves those, so `script_module_spec` does
  not backdate and the importers re-typecheck for a change no signature of
  theirs saw. `name_resolution` is the only field of a spec anything reads, and
  it backdates across a body edit by itself, so the remedy is to stop carrying
  the rest; `botdocs/plan-script-reactivity.md` section F has that measured.
- **The lowering reach is the compiler's own answer.** Phases 2 to 5 are plain
  functions with no per-unit query to record, so unlike the analysis reach it is
  not measured from outside. It is held to a graph-derived expectation and to the
  falsification runs.
- **The alias edge is the one edge not derived from a read.** A `require` binds
  an alias and an `import` uses it, but aliases resolve through
  `module_alias_at`, which the `asked_names` recording does not see, so the edge
  is added by hand. Correct and tested; if the reach ever under-reports, another
  resolution path answering a name outside the recorded lookups is the shape to
  look for.
- **Insert and remove cost the suffix, and that is the price of the index being
  part of the identity.** A script value is identified by
  `(unit_index, ValueId)`, so splicing at *i* renumbers everything after it and
  every later unit's IR has to be rebuilt. That is inherent, not a shortcoming
  of the walk: the ids really did change. **So the three splice verbs re-derive
  the whole suffix rather than `reach_from`**, and
  `the_whole_suffix_is_rederived_not_just_the_reach` asserts that against a
  graph-derived reach that is strictly narrower -- a test pinning the reach here
  would be pinning a bug. The reach is for an edit, which moves no index.

  All three are one mechanism: splice the unit list, rebuild the `Script` chain,
  destroy and discard the suffix's runtime state, re-derive the suffix.
  `ScriptCompiler::{truncate_units, remove_unit, insert_unit}` are the
  compiler's, `Engine::{truncate_units, remove_unit, insert_unit}` the shipped
  ones. Truncating is the cheap one and the only one that renumbers nothing,
  because nothing sits after what goes, so it re-derives nothing at all --
  unless a unit that goes took or wrote an earlier binding, which rewinds the
  splice to where the binding was made (section 5).

  Two things each verb needs that an edit does not. **The suffix is run as
  appends**, not in place: `rerun` goes through `replace_frame`, which puts a
  frame at the index its unit already had, so after a splice
  `ScriptExecutor::truncate_units` destroys the frames from the splice point on
  and the re-derived units take the next free index each, chosen between
  `execute_fragment` and `execute_expr` by the unit's `is_expr` flag. And **the
  same `Source` handles go back in** for the units a splice does not touch:
  `Source::new` would mint a new one and move every memo key from there on.

  **A unit of the suffix that stops compiling stays**, as an appended unit that
  fails does (see [Units that did not run to their end](#user-content-units-that-did-not-run-to-their-end)).
  Removing a unit a later unit reads leaves that unit failing, and putting the
  binding back makes it compile and run again. Splices used to be rejected
  whole instead, because a unit with no IR had no frame and the numbering could
  not have a hole in it; an index with no frame is what made accepting them
  possible.

  **This is the third time positional identity has been the constraint** --
  `IrModuleId` numbered by graph position, script `unit_index`, and now this. The
  standing answer is the same each time: derive the id from something content- or
  name-shaped rather than from a position, and the renumbering goes away. For
  `IrModuleId` it has: a pipeline numbers modules by path, once, for its life.

- **A unit that returns early keeps what it did to earlier units.** A `set`
  or an `out` argument writes into an earlier unit's frame, and a move takes
  from one, and an early return after either leaves it done: `var p = (1, "a")`
  and then a unit that does `set p.1 = "mutated"` before a failing `!` leaves
  `p` mutated. So a failed unit does influence the session until something
  re-derives it, when the rewind of section 5 builds `p` again first.
- **There is no undo or redo.** Nothing keeps a unit's previous text;
  `ModuleCompilationPipeline::module_text` exists only so `edit_module` can put
  back an edit that did not compile. The machinery needs nothing new for it --
  undoing an edit *is* an edit, undoing an append is `truncate_units`, and
  undoing an insert or a remove is the opposite splice verb -- so what is
  missing is a history, which is engine-level rather than reactivity-level and
  was deliberately left out of the splice work.
- **A `CallDispatcher` is not told about a module edit, or about a splice.**
  JIT code survives one, and a dispatcher keys a
  remembered function on `(unit, id)`, which a splice renumbers. Unreachable
  today, the REPL passing `None`.
- **No engine path adds or removes a module** mid-session. `WorkspaceDelta`
  knows how. What a set change does to the analysis is pinned -- the env interns
  over the modules, so every unit re-keys, which is right -- but the *reach* for
  an added or removed module has never been asked about.
- **Only pure `fun`s exist**, so the only effect is `debuglog` and skipping a
  unit's execution skips nothing observable beyond it. `proc`s and real I/O need
  virtualized I/O, which does not exist yet.
- **A unit that did not run is assumed to have done what it would do.** Its
  record lists what it would take and write, so a blocked unit makes the
  names it would take unusable after it, and its provider is rewound for it,
  though nothing was taken. See `issues.md`.
