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

### A. The unit dependency graph

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

**Under-reporting uses would be unsound** -- a unit would keep a stale type. Two
ways to be sure, and they are complementary rather than alternatives:

- Compute uses explicitly, by a walk over the unit's statements collecting
  referenced names minus the ones it binds itself. This is what stage C needs
  too, since execution cannot lean on salsa's internal dependency records.
- Have the typechecker record the names it actually resolved against the
  incoming environment, and assert in tests that the explicit set is a superset.
  Under-reporting then fails a test rather than producing a stale result
  silently.

Do both. The second is cheap and it is the only thing that makes the first
trustworthy.

### B. Precise keying for analysis

Replace `accumulated` in `typecheck_script_unit`'s key with the resolved uses --
the bindings this unit actually references, each paired with the unit that
provided it.

Then editing B re-keys only the units that use a name B provides. C's key is
unchanged and C is a memo hit. That is the stated goal, for analysis.

An alternative worth recording because it is tempting: have `TypeContext`
consult a per-name query on a lookup miss, so salsa records the reads and the
dependency set is correct by construction. Sound without any analysis, and it is
the firewall pattern this codebase already uses. It is *not* enough on its own,
because stage C needs the graph as data, and it would mean threading a fallback
through `TypeContext`, which modules share. Prefer A+B, and keep this in mind if
the explicit analysis turns out hard to get right.

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

A then B then C then D. A is a prerequisite for everything. B is the visible
half of the goal and is testable on its own, by constructing batches directly
without needing edits to work. C is the biggest piece and buys nothing until D.
D is small once C is done.

The measurement to hold the whole thing to is the one the
`edit_reach_tests`/`roots_tests` technique has caught three bugs with this month:
build A B C D, edit B, and count how many units re-ran each phase. Today the
answer for analysis is B, C and D; it should be B and D. Write that test first,
against today's behaviour, so the target is a number rather than a description.
