# Script reactivity

Editing a script unit should re-analyze and re-execute the units that depend on
it, and no others.

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

It also pins something found while building it, which bounds what the graph can
be tested against rather than being about the graph: **a function that reads a
script variable is not callable from a later unit.** The defining unit
typechecks and its use of the variable is recorded, but the function does not
export -- `UnresolvedName` at the call.

### B. Precise keying for analysis

Replace `accumulated` in `typecheck_script_unit`'s key with the resolved uses --
the bindings this unit actually references, each paired with the unit that
provided it.

Then editing B re-keys only the units that use a name B provides. C's key is
unchanged and C is a memo hit. That is the stated goal, for analysis.

There is a second way, and stage A removed the objection to it. Have
`TypeContext` resolve an unknown name through a per-name query --
`binding_at(script, i, name)` -- rather than being seeded with the whole
environment up front. Salsa then records the reads itself and the dependency set
is right by construction, with no key to get right. It is the firewall pattern
this codebase already uses, and this plan first said it was not enough because
stage C needs the graph as data. **That was wrong**: the recording added in stage
A *is* the graph as data, and it is independent of how the environment is
reached.

So the two are alternatives after all:

- **Key on the resolved uses.** The key is computed before the call, so it must
  be right before the call -- and what a unit uses is only known after
  typechecking it. That circularity is the catch: the key would have to come from
  a previous revision's recording, or from a separate analysis, which is the walk
  stage A avoided.
- **Read through a per-name query.** No circularity, because the dependency is
  the read. The cost is threading a fallback through `TypeContext`, which modules
  share, and `binding_at` re-running per (unit, name) on an edit -- cheap, and it
  backdates, the same trade `resolve_module_imports` already makes.

The second looks right for exactly the reason the first is awkward. Decide with a
prototype rather than on paper; the circularity is the thing to check first.

### C. Per-unit lowering and execution state

This is the stage the other work is waiting on, and the reason editing is not
merely unimplemented.

`accumulated_lower_bindings` is a **mutable linear fold**. Phases 2 to 5 --
ownership, lowering, const evaluation, IR assembly -- run only for the unit just
appended, against that fold. There is no per-unit record of what a unit consumed
or produced at the value level, so there is nothing to re-derive a suffix from.

So: make a unit's lowering and execution a function from its inputs to its
outputs, with the inputs and outputs held per unit rather than folded. Then
re-running B and D means re-running two functions, and C's outputs from the
previous run stay valid precisely because C does not depend on B.

Two things make this tractable *now* and are worth writing down because they
will stop being true:

- **A unit copies out of earlier bindings rather than moving from them**
  (`compiler-guide.md`, "Ownership across units"). So a unit's inputs are values
  it may hold independently, and re-running B does not invalidate C's copies.
  When non-cloneable types land this stops being available and the model needs
  rethinking -- which `repl-architecture.md` already says.
- **Only `fun`s exist, and they are pure.** Effects are `debuglog`, which is
  per-unit output. `proc`s and real I/O will need the virtualized I/O
  `script-semantics.md` describes before their execution can be skipped or
  replayed.

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

The measurement to hold the whole thing to is the one the
`edit_reach_tests`/`roots_tests` technique has caught three bugs with this month:
build A B C D, edit B, and count how many units re-ran each phase. Today the
answer for analysis is B, C and D; it should be B and D. Write that test first,
against today's behaviour, so the target is a number rather than a description.
