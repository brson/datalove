# Script reactivity

Editing a script unit should re-analyze and re-execute the units that depend on
it, and no others.

**This is the plan, kept for why things are shaped as they are and what was tried
and rejected.** What the code actually is, and how it is validated, is
[script-reactivity-architecture.md](script-reactivity-architecture.md).

Given units A B C D where C does not depend on B, editing B re-analyzes and
re-executes B and D. A is untouched because it is upstream. **C is untouched
because it does not use anything B provides**, even though it sits between them.

That last clause is the whole problem. Everything else follows from it.

This is the shape `mandocs/script-semantics.md` is reaching for and what
`repl-architecture.md` calls "rewind and replay". The UI is out of scope here;
this is the engine.

## Where things actually stand, measured

Four units, `let a = 1` / `let b = 2` / `let c = 30` / `let d = b + 5`, driven
through `ScriptCompiler::compile_fragment`:

| | queries | `typecheck_script_unit` |
|---|---|---|
| append unit 0 | 12 | 1 |
| append unit 1 | 11 | 1 |
| append unit 2 | 11 | 1 |
| append unit 3 | 11 | 1 |
| replay all four in a new compiler | 44 | 4 |

Three things to take from this, and the first was a surprise.

**Appending is already incremental.** Each new line typechecks exactly one unit.
`typecheck_script_unit` is keyed on `(unit_spec, module_specs, accumulated,
auto_adapt_mode)` where `accumulated` is every binding from every earlier unit --
a prefix aggregate -- and for an append that key is unchanged for the units
already there. So the existing design is right for the direction a REPL usually
grows.

**Replay in a fresh compiler reuses nothing.** `compile_fragment` mints a
`Source` per call, and `Source` is a `#[salsa::input]`, so identical text is a
different input and everything downstream of it is a different question. This
does not matter for appending -- a session keeps its `ScriptCompiler` -- but it
means a crash reset re-does the session from scratch, and it means every line
leaks an input, inputs never being collected.

**Editing is not expressible at all.** `accumulated_unit_specs` is
append-and-pop; there is no way to replace unit *i*. That is not an oversight to
patch over, because the thing in the way is real: see stage C.

## Why the prefix aggregate is the thing to remove

`accumulated: AccumulatedBindings` holds the vars, functions, function ASTs and
module aliases of every unit before this one. It is a whole-prefix value in a
per-unit key, which is the fourth time that shape has come up in this compiler
and the third fixed this month -- `AllModuleExports` in
`resolve_module_imports`, the rider stubs, the rider `IrModuleId` numbering.
Editing B changes B's outputs, which changes C's `accumulated`, which re-keys C
whether or not C ever mentions anything of B's. D follows for the same reason.

So C re-typechecks today. The fix is the same as the other three: **replace the
aggregate with the dependencies the unit actually has.**

## The four stages

### A. The unit dependency graph -- done

For each unit, what it **provides** and what it **uses**.

Provides already exists: `ScriptUnitTypecheckOutput` carries `new_vars`,
`new_fns`, `new_fn_asts` and `new_module_aliases`, which is exactly a unit's
outputs.

Uses does not exist. Nothing in the tree computes the free names of a unit --
the typechecker resolves names against a pre-seeded context as it goes, so the
information is consumed and never recorded.

Resolution is last-writer-wins by position: a name used in unit *i* resolves to
the nearest *j* < *i* that provides it. That gives the edges, and both later
stages are a walk over them.

**Under-reporting uses would be unsound** -- a unit would keep a stale type --
so it is recorded rather than computed. `TypeContext` notes every name asked of
it as the typechecker resolves one, and `ScriptUnitTypecheckOutput::asked_names`
carries the set out. A walk over the AST collecting free names was the
alternative and would have been wrong the first time some expression form was
forgotten; recording sits at the four lookups and cannot miss a form.

Two things about the set as recorded, both deliberate:

- **A name that was not found is in it.** A unit that asks for `x` and does not
  find one depends on there being no `x`: define one in an earlier unit and this
  unit's answer changes.
- **A name the unit binds itself is in it too.** Excluding those would mean
  knowing which binding each lookup found, and `statement.rs` writes pattern
  bindings straight into the map while scopes save and restore the whole of it --
  so the provenance would have to be threaded through every write site, and a
  missed one is the unsound direction. A name with no earlier provider resolves
  to no edge, so over-reporting costs a lookup and nothing else.

Done. `script_graph_tests` holds it: the A B C D case, a use of a variable, a use
of a function, a use from inside a function body, a unit drawing on two earlier
units, and a self-contained unit reaching neither.

Building it turned up a scoping hole, now fixed. **A function body is not a
closure**, but it could name an enclosing `let` and typecheck: a script unit is
seeded with every earlier unit's bindings, and entering a body only *saved* that
map rather than clearing it. The mistake then surfaced from lowering as "binding
not available yet", which reads as a phase-ordering problem rather than the
scoping error it is. A module has no top-level bindings, so nothing saw it there.
Entering a body now keeps only the consts.

Which exposed a second one: **const-ness was lost at the unit boundary.** A
script `const` arrived in the next unit's variables but not its const bindings,
so it read as a `let`. Nothing noticed while bodies could name either.
`AccumulatedBindings` carries the const names now.

**Both halves are fixed now.** `lower_script_functions` is handed the script's
consts and seeds them into the lowering context, the way
`lower_function_for_module` does with a module's, and the script compiler grew
the two strata the module pipeline already had -- lower, evaluate, lower again
only if a body deferred. `accumulated_script_consts` carries them across units.
A const naming a const across units needed the same seeding in
`evaluate_script_consts`, whose map started empty. `script_const_tests` has
eleven cases including the shadowing one, which is what says the seeding goes in
before this unit's own bindings rather than after.

What follows is what it looked like before, kept because the shape recurs:

**The gap was a lowering one.** A function body naming a
script const typechecks and then fails to lower -- "binding not available yet" --
where a *module* const in a module function body compiles and runs. So the
compiler agrees the program is legal and cannot build it.

It is not about crossing a unit boundary: a const and a function reading it in
the *same* unit fails the same way. A script-level const is simply not among
what a function body is lowered against. `lower_module_functions` is handed the
module's consts; the script path hands a function only its own, qualified
`func_name::const_name`.

**Nothing covered it.** `interp_constlet` has a const declared *inside* a
function, a script const read by a script `let`, and a fixture named
`006_script_const_cross_unit` whose const and use are in the same section. None
puts a script-level const in a function body, and the suite runs with
`skip_const_inlining`, so the ordinary path was thin too. `script_const_tests`
covers it now: three shapes that work, three that do not, and the module contrast
that makes it a defect rather than a missing feature. They assert today's
failures and say which to tighten when it is fixed.

### B. Precise keying for analysis -- done

The goal: editing B re-typechecks B and D, and C is a memo hit. **That holds
now**, and `script_reactivity_tests` pins it: editing B re-typechecks exactly the
units whose `asked_names` intersect what B provides, which for A B C D is B and
D, with C and A memo hits. A value-only edit reaches only the unit it is in; a
type edit reaches the units that read the binding; appending still typechecks
one unit.

What went in, and it is the design below with one thing changed:

- **`AccumulatedBindings` is gone.** `TypeContext` is no longer seeded with the
  earlier units' bindings; it holds an optional `InheritedBindings` and consults
  `binding_at` on a miss in `lookup_variable`, `lookup_variable_mutability`,
  `lookup_function`, `lookup_function_ast` and `is_const_binding`. The last of
  those was not on the list and had to be: const-ness across a unit boundary is
  what decides whether a function body may name a binding.
- **`unit_ast(db, unit)`** holds a unit's parse, name resolution and spans, keyed
  on the unit alone, since none of it depends on the units around it.
- **`unit_provides(db, script, env)`** is a firewall in front of
  `typecheck_script_unit`, and it earns its keep: a unit's whole output moves
  whenever anything in its body moves -- it carries the expression types -- so
  without the projection a body edit would re-run every later unit's `binding_at`
  walk. With it, a body edit stops there.
- **`ScriptEnv`** interns the modules and the auto-adapt mode. The plan had those
  passed by value; `binding_at` is asked once per name a unit uses, and hashing
  the module list at each of those is the module world in the key by another
  route. It held every module's `ModuleSpec` at first, which was the module world
  in the key by the *direct* route -- see section F.
- **A function body had to be told it is a body.** It is not a closure, and
  dropping the enclosing `let`s from `variables` was enough while they were
  seeded into it. Once a miss falls through to the earlier units, the body has
  to refuse what it may not name, so `TypeContext::in_function_body` gates the
  inherited-variable lookup to consts.

**The one change: the script is interned over each unit's `Source`, not an
input holding the units.** The property the plan wanted from an input --
identity independent of value -- is had one level down, because a `Source` *is*
an input and editing a unit is `set_text` on it. `ScriptUnit` is interned over a
source, and `Script` is a cons list of those: "this unit, and the units before
it". So editing any unit moves no key, which is what stage B needed, and
appending leaves the earlier handles alone without a setter at all.

The reason not to use the input: **appending would have needed `&mut db` too,
not just editing.** `set_units` is the only way to grow an input's list, so
every REPL line would want exclusive access to the database -- and
`ScriptCompiler` cannot be handed it. It holds `&'db dyn Database` and, behind
that lifetime, `ModuleSpec`s and a whole `SharedModuleContext` derived from
`CompiledModules<'db>`, and salsa's lifetimes exist precisely to stop that data
outliving a mutation. `ModuleCompilationPipeline` gets away with `&mut D`
because it holds nothing but `Source`s and rebuilds its `'db` data per compile;
making the script compiler do the same means re-deriving the module compilation
on every line and restructuring 58 call sites across 22 files. That is stage C
and D's shape, not a keying change.

`&mut db` is still what an edit costs, exactly as
`IncrementalModuleWorld::update_source` costs it -- `ScriptCompiler::unit_sources`
hands out the `Source` per unit for that, and the caller that owns the database
does the `set_text`. What is not needed is `&mut db` per appended line.

Two smaller things fell out. `function_types` on a unit's result no longer
carries the earlier units' functions, because every consumer looks one up by the
name of a function among *this* unit's statements; and it is sorted, which it
was not -- a `HashMap`'s iteration order was deciding part of a memoized value's
identity.

What follows is the design as written before the work, kept because the argument
for reading through a query rather than keying on resolved uses is the part that
matters and is unchanged.

**The script becomes a `#[salsa::input]`,** which is what
`mandocs/script-semantics.md` specified in the first place:

```rust
#[salsa::input]
struct Script { units: Vec<ScriptUnitSpec> }
```

An input's identity is independent of its value -- that is the whole reason to
reach for one here. So:

```rust
typecheck_script_unit(db, script, index, module_specs, auto_adapt_mode)
```

has a key that **does not move when any unit is edited**, because the key is a
position in a thing with a stable identity rather than a hash of everything
before it. That is the property the prefix aggregate cannot have.

The environment is then reached lazily rather than seeded:

```rust
unit_ast(db, script, index)               -> ScriptUnitSpec
binding_at(db, script, index, name)       -> Option<Binding>
```

`binding_at` walks back from `index - 1` for the nearest unit providing `name`.
`TypeContext` consults it on a lookup miss instead of being pre-seeded with
`AccumulatedBindings`, and **salsa records the dependency because the dependency
is the read** -- there is no key to get right and nothing to keep in step.

Editing B then goes: `unit_ast(script, B)` changes. `binding_at(script, i, name)`
re-runs for `i > B` and *backdates* unless B provides that name.
`typecheck_script_unit(script, C)` depended only on the `binding_at`s C asked
for, all backdated, so C is a memo hit. D asked for something B provides, so D
re-runs. Which is the goal, exactly.

**Why not key on the resolved uses.** It is circular: a unit's uses are only
known after typechecking it, so the key would have to come from a previous
revision's recording or from a separate analysis -- and the separate analysis is
the AST walk stage A avoided for being unsound when it misses a form. Reading
through a query has no such problem.

**The cost, and it is the awkward part.** An input is written with a setter, so
appending or editing a unit needs `&mut db`, where `ScriptCompiler` holds
`&'db dyn salsa::Database` today. That ripples to `Session` and `Engine`. It is
the same shape as `IncrementalModuleWorld::update_source` needing `&mut db`,
and `ModuleCompilationPipeline::compile` already takes `&mut D` for exactly this
reason, so there is precedent to follow rather than a new idea to invent.

Two things fall out for free. `Source`-per-`compile_fragment` stops being minted,
so the replay row in the table above and the input leak per line both go. And
stage D's "hold a stable handle per unit" is most of this, so D shrinks to
re-deriving the affected suffix.

**`asked_names` stays useful.** It is not the key -- the query reads are -- but it
is the graph as data, which stage C needs, and it is how to *test* that this is
precise: the units that re-typechecked should equal the units whose `asked_names`
intersect what the edit changed. Assert that rather than a bare count.

### C. Per-unit lowering and execution state -- done

The goal: editing B re-lowers and re-executes B and D, and C keeps the IR and
the frame it had. **That holds now**, and
`script_exec_reactivity_tests` pins it, with the engine's own tests saying the
REPL wires it up.

**The `(unit_index, ValueId)` argument checked out**, which was the thing to
verify before relying on it. `c_holds_no_reference_to_b` serializes a unit's IR
and walks it for every `ExternalValue`, `ExternalSlot` and `External` -- the
three forms that carry a unit index -- so it covers instruction forms nobody
enumerated. C refers to no earlier unit; D refers to unit 1 and nothing else.
Walking the serialization rather than matching on the instruction set is what
makes it hold for a form added later.

What went in:

- **`AccumulatedLowerBindings` is gone.** `UnitLowerRecord` holds what one unit
  produced -- its exports and their types, the script consts it declared, its
  dead and revived exports, and its provides and uses as owned text -- and
  `lower_context_over`, `script_consts_over` and `dead_externals_over` fold the
  records before unit *i* into what unit *i* is compiled against. Three folds
  went at once: the lowering bindings, the script consts, and `dead_externals`,
  which was the same shape and would have been missed.
- **`FrameStore::replace_frame`**, which destroys the old frame's unit-end
  bindings before putting the new frame in its place, and
  `UnitFunctionRegistry::set_unit_code_units` beside it, because a
  `CodeRef::Local` is a position in one unit's function list.
  `IrInterpreter::reexecute_script_unit_in_env` takes the unit index rather than
  reading it off the store, since re-executing must not move the unit.
- **`ScriptExecutor` derives its bindings** from a per-unit record of exports
  rather than folding into one map, for the same reason the compiler does:
  re-executing a unit replaces what it exports, and a fold would leave the names
  it used to export standing.
- **`ScriptCompiler::relower_reach(i)`** walks the graph and re-lowers the
  reach in index order. The reach is transitive -- D can depend on B only through
  C -- and taken over the **union of the pre-edit and post-edit graphs**, since
  either alone misses a case: a binding the edit removes is absent from the new
  graph though the unit that read it must be told, and a name the edit
  introduces is absent from the old one though a unit that asked for it in vain
  now finds it. Module aliases are in `provides` as well as bindings, because a
  `require` in one unit is what an `import` in a later one resolves through and
  the typechecker reaches that by a different query than `asked_names`.
- **Lowering no longer parses a unit itself.** The statements come from
  `unit_ast`, keyed on the unit, so re-lowering costs no second parse.

**The edit needs `&mut db`, so no compiler survives it.** `ScriptSession` is the
plain-data half -- the units' `Source`s and the per-unit records -- and
`CompiledModules::script_compiler_resumed` builds a compiler over it again
afterwards. The `Source`s being inputs is what makes that free: interning them
again gives back the very same `ScriptUnit` and `Script` handles, so every memo
keyed on one is still good. **The `Engine` owns its database now** rather than
borrowing it, and builds a compiler per operation. What that costs is
re-deriving the module compilation each line, which is salsa verifying what it
already has -- the same thing a crash reset has always relied on.

Two things worth knowing about what it does not do:

- **A unit that fails to re-lower keeps its frame.** Its record goes empty, so
  its bindings leave the environment and no later unit can name them, but the
  frame stays where it is: the numbering the frame store and every
  `(unit, value)` reference share cannot have a hole in it. What that frame owns
  is destroyed when the session ends, so nothing leaks.
- **An edit reaches expression units too**, and running one again needs
  somewhere for its value to land -- `UnitEnd` carries a result and the
  interpreter panics without a destination. `ScriptExecutor::reexecute_unit`
  handles both kinds.

What follows is what it looked like before.

**Half of this already exists, which the plan had wrong.** `FrameStore`
(`datalove-datafun-interp/src/frame.rs`) is already partitioned by unit:

```rust
pub struct FrameStore {
    frames: Vec<Frame>,                 // indexed by unit number
    unit_end_values: Vec<Vec<ValueId>>, // the persistent bindings to destroy
    unit_end_slots: Vec<Vec<SlotId>>,
}
```

So the runtime state is not a fold and never was. Re-executing B means replacing
`frames[B]`, and C's frame is untouched -- which is the structure execution
reactivity needs, sitting there already.

**Why not re-lowering C is safe.** A script value is identified by
`(unit_index, ValueId)`, so a unit's IR names the units it reads from. A unit
that uses nothing of B's has no `(B, *)` reference in its IR at all, so B's
values being rebuilt cannot reach it. Editing rather than removing a unit leaves
the indices alone, so the identification stays good. That is the property that
makes the whole stage work, and it is worth checking before relying on it.

What is actually missing is two things:

- **`accumulated_lower_bindings` is a mutable linear fold**
  (`AccumulatedLowerBindings`: name to `(unit_index, ValueId | SlotId | FuncId)`
  plus types). Phases 2 to 5 -- ownership, lowering, const evaluation, IR
  assembly -- run only for the unit just appended, against that fold. There is
  no per-unit record to re-derive a suffix from. Hold each unit's own produced
  bindings instead, and build unit *i*'s context from the units before it.
- **`FrameStore` is append-only.** It has `add_frame`,
  `destroy_unit_end_bindings` and `destroy_live_values`, but nothing that
  replaces unit *i*'s frame. Re-executing a unit has to destroy that unit's
  unit-end values and put a new frame in its place, or the old values leak and
  the indices go wrong.

Then re-deriving is a walk over the graph stages A and B built: for an edit to
unit *i*, re-lower and re-execute *i* together with the units whose
`asked_names` reach it, **in index order**, and leave the rest.

Two things make this tractable now and will stop being true:

- **A unit copies out of earlier bindings rather than moving from them**
  (`compiler-guide.md`, "Ownership across units"). So re-running B cannot
  invalidate a copy C took. Non-cloneable types remove this and the model needs
  rethinking -- `repl-architecture.md` says so too.
- **Only pure `fun`s exist.** The only effect is `debuglog`, which is per-unit
  output, so skipping a unit's execution skips nothing observable beyond it.
  `proc`s and real I/O need the virtualized I/O `script-semantics.md` describes.

### The alias edge, and why it is the soft spot

Every other edge in the graph comes from a read: stage A records the names asked
of `TypeContext`'s lookups, so it cannot miss one. **A module alias does not go
through those lookups.** `require module local/test/utils` binds the alias
`utils` -- the last component of the path, there being no `as` renaming -- and
`import utils.ident` resolves it through `module_alias_at`, which walks the
units' `module_aliases`.

So the alias edge is added to the graph by hand, from a `require`'s provides and
an `import`'s uses. It works: editing a `require` reaches a later `import` of
that alias, and `editing_a_require_reaches_a_later_import_and_nothing_else`
holds it with a third unit as the control. But **nothing about the way it is
built makes it right**, where the rest of the graph is right by construction.

If the reach ever under-reports, this is the shape to look for: another
resolution path that answers a name without going through the recorded lookups.
The sound version extends the recording to cover `module_alias_at` so the edge
comes from the read like every other one. Not urgent -- it is correct and
tested -- but it is the one asymmetry left in the design.

### D. The edit itself

`ScriptCompiler::edit_unit(i, text)` and an engine entry point: hold a stable
`Source` per unit and `set_text` on it, exactly as `IncrementalModuleWorld` does
for modules -- "a `Source` is the input, and it is the only handle worth
keeping". That also fixes the replay row in the table above and the input leak
per line.

Then re-derive from the graph: re-analyze and re-execute the units reachable
from the edited one, in order, and leave the rest.

## Order, and what each stage is worth

A is done. Then B, C, D. A was a prerequisite for everything. B is the visible
half of the goal and is testable on its own, by constructing batches directly
without needing edits to work. C is the biggest piece and buys nothing until D.
D is small once C is done.

**C took most of D with it.** The edit path is `ScriptSession::unit_sources` and
`set_text`, `ScriptCompiler::relower_reach` and `Engine::edit_unit`, all of which
C needed in order to be testable at all -- there is no way to measure the reach
of an edit without being able to make one. What D leaves is the replay row in
the table above and the input leak per line, neither of which C touched: a
session still mints a `Source` per appended line, and a fresh compiler still
re-does the session from nothing.

The measurement to hold the whole thing to is the one the
`edit_reach_tests`/`roots_tests` technique has caught three bugs with this month:
build A B C D, edit B, and count how many units re-ran each phase. Today the
answer for analysis is B, C and D; it should be B and D. Write that test first,
against today's behaviour, so the target is a number rather than a description.

## Confidence, and what is not covered

Written after auditing the suites rather than from memory.

**Solid, and falsified rather than merely asserted.** The unit-to-unit reach is
the thing the stages were for, and it is held three ways: the graph
(`script_graph_tests`), analysis (`script_reactivity_tests`), and lowering and
execution (`script_exec_reactivity_tests`) -- 38 tests between them and
`script_const_tests`. The expectations are *derived from the graph* rather than
written down, with a separate test pinning that the fixture is the shape the
claim is about. Every stage was checked against injected faults: stage B against
a spurious dependency, stage C against four -- a one-step reach, a reach widened
to every later unit, one narrowed to the edited unit, and `replace_frame` with
its destroy removed. Each reported the old answer. The `(unit_index, ValueId)`
property the whole design rests on is checked by walking a unit's serialized IR
for external references rather than by argument.

**The module gap is closed**, by section E, and `Engine::edit_module` makes it
reachable from shipped code rather than latent.

**What the scenario suite added.** `script_scenario_tests` crosses the five edit
kinds -- value-only, signature-changing, error-introducing, module body, module
signature -- against the three positions an edit can be made at, and asserts the
analysis reach *and* the execution reach for each, both derived from the
typechecker's per-unit record before and after the edit. The analysis reach is
derived by comparing what each name a unit asked about resolves to, which is
what `binding_at` answers; that is a smaller set than the execution reach, and
`a_value_only_edit_is_analyzed_narrowly_and_executed_widely` is what says the
two measurements are not one measurement taken twice. The same file holds a
twelve-unit session with a diamond, a unit drawing on two, a chain of four and
two independent roots; the removal case; and the ownership cases.

**Removing a unit turned out to be inexpressible rather than wrong.** Nothing in
`ScriptSession`, `ScriptCompiler`, `FrameStore` or `UnitFunctionRegistry` offers
a removal, and that is the numbering protecting itself: every script value is a
`(unit_index, ValueId)` pair, so taking a unit out would leave every later
unit's IR one frame off. What is expressible is *blanking* a unit, and it
behaves -- the index and the frame stay, what the unit exported stops being on
offer, the units that read it are told -- so that is what a "delete that line"
has to be built out of.
`blanking_a_unit_is_the_removal_the_numbering_allows` and
`a_blanked_units_index_is_not_reused` hold both halves, the second of them
stating the cost: a session that deletes lines spends an index per deletion.

**Ownership across a re-execution holds.** A unit copies out of an earlier
unit's binding, and `a_copy_out_of_an_earlier_unit_survives_its_re_execution`
puts a list -- heap memory, so a move would be visible to the leak checker --
through an edit to the unit it came from. A unit may only move out of a binding
it defined itself, and `a_move_across_units_survives_an_edit` says the record of
that survives a re-derivation: D013 is still reported for a line typed
afterwards, which is the only observable, since the frame still holds the value.

**Thinner than it looks:**

- **The lowering reach is the compiler's own answer.** There is no per-unit salsa
  query in phases 2 to 5 to record, so unlike the analysis reach it is not
  measured from outside -- it is held to a graph-derived expectation and to the
  falsification experiments. `analyze_script_fragment_tracked` is not a
  substitute: it is keyed on the typecheck result and statement handles, which a
  value-only edit does not move.
- **The alias edge is the one edge not derived from a read.** Correct and tested;
  see the section above.
- **A module edit re-typechecks the units that import from it**, and no longer
  every unit: section F. What is left is that a *body* edit still reaches the
  importers, because `ModuleSpec` carries the module's spans.
- **The cost per edit is still unmeasured.** The twelve-unit session says the
  *reach* is right at that size; it says nothing about time. The records fold
  from the start of the session for every unit re-derived, so the cost per edit
  grows with the session, and nobody has measured where that stops being free.
- **`Engine` re-derives the module compilation per line**, resting on
  `ScriptEnv` interning to the same handle each time -- which is now a much
  weaker thing to rest on: the env is a list of `Module` handles, and a `Module`
  is interned over a `ModuleId` and a `Source`, neither of which a recompile
  moves. It used to rest on every `ModuleSpec` field comparing equal. What an
  unchanged recompile does to the handle is still not tested directly.
- **No engine path adds or removes a module mid-session.** `Engine::edit_module`
  changes a module the session started with, and `WorkspaceDelta` knows how to
  add and remove with nothing reaching it. What an added or removed module does
  to the *analysis* is measured now --
  `adding_a_module_re_keys_every_unit` and its removal twin -- and it is that
  every unit re-keys, because the env interns over the module set. What is still
  unasked is the reach: an added module can only affect a unit typed after it,
  but a removed one leaves an import naming nothing.

## E. The module gap -- done

The goal: editing a module re-lowers and re-executes the script units that
import from it, and no others. **That holds now**, and
`script_exec_reactivity_tests` pins it -- the test that used to pin the
staleness is inverted -- with `engine_edit_tests` saying the REPL wires it up.

What went in, and it is the three steps below with one thing the diagnosis did
not mention:

- **`ScriptUnitTypecheckOutput::imported_modules`** holds the module paths a
  unit's imports resolve into, recorded where the import resolution already
  computes them. An import whose function was not found is in there too, since
  a unit that failed to import from a module still depends on it. It travels
  into `UnitLowerRecord::imports` as owned text, the way `provides` and `uses`
  do, because the reach is walked after the database has been mutated.
- **`ScriptCompiler::relower_module_reach(&[path])`** seeds the reach with the
  units whose imports resolve into the edited modules and then walks the same
  name edges `relower_reach` walks. `edit_reach` and the module reach are one
  function, `reach_from(seeds)`, so there is no second rule to keep in step.
- **`Engine::edit_module`**, and **`ScriptExecutor::set_module_registry`**,
  which is the step a unit edit does not need and the one that was not in the
  plan. **A script unit names a module function by `CodeRef::Module`**, so
  re-lowering the unit changes nothing about what that call lands on: the
  executor resolves it against the `Arc<ModuleFunctionRegistry>` it was handed
  when the session started. Without the swap the reach is right and every value
  is still stale, which is what the injected fault confirmed.

**An edit whose modules do not compile is put back.** Nothing in the engine can
work against a module set with errors -- `script_compiler_resumed` returns
`None` and every later line would fail to build a compiler at all -- so
`edit_module` returns the errors as `Err` and restores the previous text.
Reporting module errors while keeping the broken text means carrying them
through every path that compiles, which is a larger change than this.

**The over-propagation the plan warned about was real, and wider than
predicted.** Measured with `QueryRecorder`: after *any* module edit, body or
signature, every unit's `typecheck_script_unit` ran again -- and not because an
answer moved but because the *key* moved. `ScriptEnv` was interned over every
`ModuleSpec`, which holds the module's spans, so editing one module's body gave
a new env handle and every `(script, env)` pair was a different question. The
execution reach stayed narrow, which is the half that decides correctness, and
`script_scenario_tests` asserted the over-propagation outright rather than
leaving it to be discovered. Section F is the narrowing, and the assertion is
inverted now.

This is also where the diagnosis below is wrong: it says the env does not
re-key, and it does. What is right about it is the conclusion -- **the gap was
a missing driver** -- because with nothing asking, a moved key costs nothing.

What follows is the diagnosis as written before the work.

**Editing a module does not reach the script units that import from it.** They
keep values computed against the old module and nothing notices.

The diagnosis took three tries and the first two were wrong, so they are here to
save the next person the same trip:

- **"`ScriptEnv` does not over-propagate" -- measured correctly, concluded
  wrongly, and the mistake is the instructive part.** The measurement was that
  after a module edit no unit's `typecheck_script_unit` ran, for a body change
  or a signature one. The conclusion drawn was that the keys had not moved. They
  had: `ScriptEnv` is interned over every `ModuleSpec`, spans included, so a
  module edit re-keys every unit. Nothing had *asked*, because stage B made
  typechecking lazy.
  
  **A recording of what ran cannot tell you what would run if asked.** That is
  worth remembering whenever `QueryRecorder` is the instrument: it answers "what
  executed", and "is this memo still valid" is a different question. Asking the
  units directly is what settled it.
- **Not a stale env.** The env is rebuilt on resume -- `ScriptSession` holds
  units and records, not the env -- and a *new* unit can import a function the
  module has just gained. So the script's view of the modules is current.
- **It is a missing driver.** Stage B made typechecking lazy: an earlier unit is
  typechecked when a later one asks `binding_at` about a name it provides.
  Nothing asks after a module edit, so nothing re-runs. There is no entry point
  that says "a module changed, go and re-derive what depended on it" --
  `relower_reach` takes an edited *script unit* index and has no module-shaped
  sibling.

So the fix is a driver, not a keying change:

1. **Record which modules a unit imports from.** Derivable from
   `new_module_aliases` -- alias to full path -- plus the unit's import
   statements. This is the module-shaped half of the alias edge and it wants to
   be part of the graph rather than recomputed.
2. **A module-shaped entry point.** Take the edited module paths, find the units
   whose imports resolve into them, add their transitive script-unit dependents,
   and re-derive in index order -- exactly what `relower_reach` does from a unit.
3. **Engine support**, so it is reachable rather than latent. Nothing in
   `datalove-repl` can edit a module today, which is the only reason this gap is
   not a live bug.

**One thing to be careful of.** After a module *signature* change `ScriptEnv`
does re-intern, so any unit that is asked will re-typecheck. A driver that
re-derives the whole script on any module edit would therefore over-propagate
where the old behaviour under-propagated. Narrowing the env so a unit depends
only on the modules it imports from is the sound end state -- and it is the same
whole-world-aggregate-in-a-per-unit-key shape that has come up six times now --
but the correctness fix is the driver, and the narrowing can follow.

## F. The over-propagation on module edits -- done

**Done.** `ScriptEnv` holds `Vec<Module>` and `script_module_spec` derives a
module's spec per module, so a module edit re-keys nothing and reaches the units
that import from the edited module plus their name-edge dependents. Measured:

| edit | analysis reach, five units over three modules |
|---|---|
| module body (`m1`, imported by unit 2 alone) | `{2}` -- was `{0,1,2,3,4}` |
| module signature (`m1`) | `{2,3}` -- was `{0,1,2,3,4}` |
| a module added | every unit, and the env legitimately moved |
| a module removed | every unit, same reason |

The execution reach is unchanged at `{2,3}` either way, which is the point: it
was already narrow and now analysis agrees with it. `script_scenario_tests`
holds all four rows, the first two as dedicated cases and the matrix's module
rows through the same `analysis_reach` the unit rows use, seeded with
`module_importers` instead of the edited unit.

**What the falsification showed.** Six faults. Restoring the aggregate -- asking
`script_module_spec` for every module in the env rather than the one an import
names -- reported `{0,1,2,3,4}` for both kinds of edit, which is the old answer
exactly, and left the execution and lowering suites green: this really is wasted
analysis rather than a wrong answer. Reverting the three shipped files to the
previous commit with the new tests in place failed on the key attribution
itself, "a typecheck under a key no cold run used", which is the assertion the
old suite made in reverse. Dropping `module_spans` from `script_module_spec`
made a body edit backdate to `{}` while leaving the signature case at `{2,3}`
and every execution suite green -- see the note on splitting below, because that
is the experiment for it. Pinning the env's module list at three modules left
the add and remove cases reporting nothing. Seeding
`relower_module_reach` with every unit and with no unit moved the execution
reach to `{0,1,2,3,4}` and `{}`.

What follows is the plan as written before the work.

Any module edit re-keys every script unit, so every unit re-typechecks when
asked -- a body change and a signature change alike, and whether or not the unit
imports from that module. Wasted analysis rather than a wrong answer: the
execution reach stays narrow, so nothing computes the wrong thing.

**It is the sixth instance of one shape** -- a whole-world value in a per-unit
key -- after `AllModuleExports` in `resolve_module_imports`, the rider stubs, the
rider `IrModuleId` numbering, `accumulated` on script units, and
`accumulated_lower_bindings`. The fix is the one that worked the other five
times: replace the aggregate with a per-entity lookup, so a unit depends on the
modules it names rather than on all of them.

`ScriptEnv` is interned over `Vec<ModuleSpec>`, and a `ModuleSpec` holds the
module's path, source, spans, parse, id and name resolution. Editing any module
changes one of those, so the whole env interns to a new handle.

**`Module` is already the stable handle this needs.** It is interned over
`(ModuleId, Source)`; a `Source` is the input and `set_text` changes its text,
not its handle. So a `Module` survives a module content edit unchanged, where a
`ModuleSpec` does not. And every `ModuleSpec` field is derivable from a
`Module`: the path and id off the id, the source off the module, `module_spans`,
`parse_module_ast`, and `resolve_script_names`.

So:

```rust
ScriptEnv { modules: Vec<Module>, auto_adapt_mode }   // was Vec<ModuleSpec>

script_module_spec(db, module: Module) -> ModuleSpec  // new, tracked per module
```

The env is then stable across a module's *content* changing and moves only when
the module *set* does -- which is right, since that is a different world. A unit
that resolves an import asks `script_module_spec` for the one module it named, so
it depends on that module and no other.

Editing a module then reaches the units that import from it and stops. Which is
what the execution reach already does, so the two would finally agree.

**A second-order limit worth knowing before measuring the result.** `spans` is
part of a `ModuleSpec`, and a body edit moves spans, so
`script_module_spec` will not backdate on one -- the importing units still
re-typecheck even though no signature moved. That is a large improvement on
*every* unit re-typechecking, and it is not the end state. Splitting the spec so
a unit depends on a module's signatures and not its spans would make a body edit
backdate entirely; diagnostics are what want the spans, and they are needed only
when something is reported.

**The measurement, taken as fault 3 of the falsification run above.** Building
`script_module_spec` with an empty span table took a module body edit's analysis
reach from `{2}` to `{}` and left the signature case at `{2,3}`, with
`script_exec_reactivity_tests`, `script_reactivity_tests` and every other module
row unchanged. So the split does the thing it promises and the signature edge
does not go through the spans.

It is also cheaper than it looks, because **the only field of a `ModuleSpec`
anything now reads is `name_resolution`.** `path` and `module_id` came off the
`Module`'s id once the env held handles, and `source`, `spans` and `parsed` were
already unread before that. So the split is not a split: it is
`script_module_names(db, module) -> CollectedNames`, and a body edit would
backdate through it for free, because `parse_module_ast` compares equal across
one -- a `StmtFun`'s body rides a tracked field -- so `resolve_script_names`
does not re-run at all. The firewall is already there; the spec is what reaches
around it.

**Zero is the right answer for a body edit, not merely a smaller one.** It could
be read as the edit being ignored. It is not: zero means no script unit
re-typechecks, while the importers still re-execute, because the value changed
and the type did not -- the same split as a value-only edit to a script unit.

Safe because a unit's typecheck result cannot depend on a module function's
body: `synthesize` reads `type_params` and `type_bounds` off the AST it looks
up, which are signature, and nothing reads `body`. Comptime and const evaluation
of a module function happen in lowering, driven by the execution reach, which
stays non-zero. What *would* make zero wrong is a unit's typecheck result
embedding something body-derived -- a diagnostic pointing into a module body,
say -- so that is the thing to re-check if diagnostics ever start doing it.

**Done.** `script_module_names(db, module) -> CollectedNames` replaced it and
`ModuleSpec` is deleted. A module body edit now re-typechecks nobody, where it
reached the importer before; a signature edit still reaches the importer and its
readers; the execution reach is unchanged at both. The five unread fields were
vestigial, as suspected -- nothing outside the one builder and the one consumer
touched them.

What to settle before doing it was whether anything is *meant* to read the other
five. They predate `module_spans` being a query of its own, and a diagnostic
against a module asks `module_spans(db, module)` directly now.

## G. Insert, remove and truncate

The verbs today are append -- a fragment or a bare expression -- and edit, of a
unit or of a module. A session can grow and anything in it can change, but it
cannot be **reordered**, and insert, remove and undo-of-insert are all the same
problem: a unit's index is part of every value's identity, so splicing the list
renumbers everything after the splice and every later unit's IR has to be
rebuilt. That cost is inherent. The ids really did change.

**All three are one mechanism.** Splice the unit list, rebuild the chain,
discard the suffix's runtime state, re-derive the suffix:

```
truncate(n)    drop units >= n.                 No renumbering: nothing after.
remove(i)      splice out i, then the above from i.
insert(i, ..)  splice in at i, then the above from i.
```

Truncate is the cheap one and the only one that renumbers nothing, which is why
it is also what undo-of-append would use.

The pieces exist. `Script::from_units(db, &[ScriptUnit])` already folds a chain
out of any unit list. `ScriptSession`'s `units` and `records` are index-aligned
`Vec`s, so splicing is a splice. `rederive(Vec<usize>)` already re-derives an
arbitrary list, and `relower_reach` already walks a reach. What is missing:

- **`FrameStore` cannot truncate.** It has `add_frame`, `replace_frame` and
  `destroy_unit_end_bindings`, and nothing that drops the frames from *i* on.
  Re-deriving a suffix means destroying those units' unit-end values first --
  this repo has had leak bugs, and the leak checker only runs under `just test`.
- The three verbs themselves, on `ScriptCompiler` and then `Engine`.

**Two things to get right rather than discover.** A removal whose bindings a
later unit reads must leave that unit failing to compile, which is what blanking
already does -- and a unit that fails to re-lower keeps its frame, because the
numbering cannot have a hole. And the suffix re-deriving is *not* a reach bug to
be narrowed away: those units' values genuinely changed identity, so a test
should assert the suffix is re-derived rather than treat it as over-propagation.

Undo and redo are deliberately not part of this. They need a history, which is
engine-level and needs nothing from the machinery -- undoing an edit *is* an
edit. Only undo-of-insert and undo-of-remove wait on this item.
