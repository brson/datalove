# The responsive scripting environment

How editing a script unit re-analyzes and re-runs the units that depend on it,
and no others. This is the map of the machinery and how it is held to its claim.

[plan-script-reactivity.md](plan-script-reactivity.md) is the plan that built it,
organised by stage and keeping the wrong turns; read it for *why* things are
shaped this way and what was tried and rejected. This document is what the code
is. [repl-architecture.md](repl-architecture.md) has the crates, the UI and the
request/response shape around it. The semantics it aims at are
`mandocs/script-semantics.md`.

## The claim

Units A B C D, where C uses nothing B provides. Editing B re-analyzes and
re-executes **B and D**. A is upstream and untouched. **C is untouched even
though it sits between them.**

Editing a module re-runs the units that import from it, and the units those
reach.

## The shape of it

```
Script (interned cons list)          the identity everything is keyed on
  prev: Option<Script>               "this unit and the ones before it"
  unit: ScriptUnit (interned)
          source: Source             <- the input; set_text edits a unit

ScriptEnv (interned)                 what the script is checked against
  modules: Vec<Module>               <- handles; a Module survives set_text
  auto_adapt_mode
          script_module_spec(module) -> ModuleSpec, per module, tracked

per unit, from typechecking:         ScriptUnitTypecheckOutput
  new_vars / new_fns / new_fn_asts   what it provides
  new_module_aliases / new_consts
  asked_names                        what it uses
  imported_modules                   which modules it reaches

per unit, from lowering:             UnitLowerRecord
  exports, value_types, slot_types   what later units can name
  consts, dead_exports

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

A module edit needs one more thing: a unit names a module function by
`CodeRef::Module`, resolved against the `ModuleFunctionRegistry` the executor
holds, so re-lowering alone changes nothing that *runs*. `set_module_registry`
swaps it, down through `FunctionRegistry`, `ScriptEnvironment` and
`ScriptExecutor`.

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
| `script_scenario_tests` | 14 | the edit-kind matrix, the module cases, a twelve-unit session, blanking, ownership |
| `script_const_tests` | 11 | consts across units and into function bodies |
| `script_splice_tests` | 14 | truncate, remove and insert: the suffix, the values, the rejections |
| `datalove-repl/engine_edit_tests` | 12 | the shipped entry points end to end |

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
  because nothing sits after what goes, so it re-derives nothing at all.

  Two things each verb needs that an edit does not. **The suffix is run as
  appends**, not in place: `rerun` goes through `replace_frame`, which puts a
  frame at the index its unit already had, so after a splice
  `ScriptExecutor::truncate_units` destroys the frames from the splice point on
  and the re-derived units take the next free index each, chosen between
  `execute_fragment` and `execute_expr` by the unit's `is_expr` flag. And **the
  same `Source` handles go back in** for the units a splice does not touch:
  `Source::new` would mint a new one and move every memo key from there on.

  **A splice whose suffix fails to compile is rejected and put back**, the
  policy `edit_module` already follows, because a unit that fails to compile has
  no IR and so no frame, and the numbering the frame store shares with every
  `(unit_index, ValueId)` reference cannot have a hole in it. So the suffix is
  compiled before the frame store is touched, and a rejected splice leaves the
  session exactly as it was and still usable. A consequence: **a unit a later
  unit depends on cannot be removed without removing its dependents first.**

  Blanking a unit is still legal and still behaves correctly, at the cost of an
  index and a frame until the session ends; removing it is now the cheaper
  answer everywhere the suffix compiles without it.

  **This is the third time positional identity has been the constraint** --
  `IrModuleId` numbered by graph position, script `unit_index`, and now this. The
  standing answer is the same each time: derive the id from something content- or
  name-shaped rather than from a position, and the renumbering goes away.

- **A unit an earlier edit left failing to compile blocks every splice before
  it.** `relower_reach` leaves such a unit in place -- it provides nothing and
  keeps its frame -- so a later `remove_unit` or `insert_unit` finds it in the
  suffix, and the rule is that the suffix must compile. The splice is rejected
  by a unit that has nothing to do with it, and the only ways on are to fix that
  unit or to remove it first. Deliberate rather than overlooked, since the
  alternative is a placeholder frame for a unit with no IR, but nothing asserts
  it today.
- **There is no undo or redo.** Nothing keeps a unit's previous text;
  `ModuleCompilationPipeline::module_text` exists only so `edit_module` can put
  back an edit that did not compile. The machinery needs nothing new for it --
  undoing an edit *is* an edit, undoing an append is `truncate_units`, and
  undoing an insert or a remove is the opposite splice verb -- so what is
  missing is a history, which is engine-level rather than reactivity-level and
  was deliberately left out of the splice work.
- **A `CallDispatcher` is not told about a module edit, or about a splice.**
  Cached inlining decisions and JIT code survive one, and a dispatcher keys a
  remembered function on `(unit, id)`, which a splice renumbers. Unreachable
  today, the REPL passing `None`.
- **No engine path adds or removes a module** mid-session. `WorkspaceDelta`
  knows how. What a set change does to the analysis is pinned -- the env interns
  over the modules, so every unit re-keys, which is right -- but the *reach* for
  an added or removed module has never been asked about.
- **A rejected module edit is put back**, deliberately, because nothing can build
  a compiler against a module set with errors.
- **Only pure `fun`s exist**, so the only effect is `debuglog` and skipping a
  unit's execution skips nothing observable beyond it. `proc`s and real I/O need
  the virtualized I/O `mandocs/script-semantics.md` describes.
- **Units copy out of earlier bindings rather than moving from them**, which is
  what makes re-running B safe for C. Non-cloneable types remove that and the
  model will need rethinking -- `repl-architecture.md` says so too.
