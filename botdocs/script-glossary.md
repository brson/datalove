# Script Glossary

The words the script engine, the REPL and their docs use for script units and
what happens to them, each with the code that carries it. Reference; written
October 2026, when the terms were settled. Where a doc or a comment disagrees
with this one, this one is right and the other is a bug.

The rules these words state are in
[script-reactivity-cases.md](script-reactivity-cases.md); how the engine
follows them is [script-reactivity-architecture.md](script-reactivity-architecture.md),
and the REPL around it [repl-architecture.md](repl-architecture.md).

## Contents

- [Units and sessions](#user-content-units-and-sessions)
- [Bindings and uses](#user-content-bindings-and-uses)
- [Steps](#user-content-steps)
- [The algebra](#user-content-the-algebra)
- [Unit states](#user-content-unit-states)
- [REPL words](#user-content-repl-words)

## Units and sessions

**unit**, script unit. What the user submits: one piece of code, compiled and
run as a whole. Not a "line" (a unit can span many) and not an "input" (an
input can be a REPL command). `datalove_script_engine::Unit`;
`ScriptUnit` in `crates/datalove-datafun-ast/src/script.rs`.

**fragment**. A unit of statements, whose bindings the session keeps: "a
fragment of statements". `Unit::Fragment`; `scriptunit-fragment` in a
worldfile; `ScriptCompiler::compile_fragment`.

**expression unit**. A unit that is one expression, whose value is its result.
`Unit::Expression`; `scriptunit-expr`; `is_expr`.

**session**. The live, editable sequence of units the engine holds, and the
REPL shows as its code. `ScriptSession`, the engine's `session` field;
`repl::Session` adds the units' numbers.

**script**. A sequence of units. A `.dfs` file is one written down. The
compiler's interned `Script` is the prefix of the session ending at a unit:
"this unit and the ones before it".

## Bindings and uses

**binding**. A name a unit defines with `let`, `var`, `const`, `fun` and the
like, for later units to name.

**provides**, **provider**. A unit provides the bindings it defines. The
provider of a binding a later unit sees is the nearest earlier unit that
provides it. `UnitAccesses::provides`, `UnitLowerRecord::provides`,
`ScriptExecutor::provider_of`. Older docs said "maker" or "builder" for the
same unit; they meant the provider.

**uses**. Every earlier binding a unit asks for, whatever it does with it.
Each use is exactly one of the five accesses below. `UnitAccesses::uses`,
`UnitLowerRecord::uses`, `ScriptUnitTypecheckOutput::uses`.

The five accesses, each per binding, in `UnitAccesses` and `repl::UnitDetail`
under these names:

| access | what the unit does to the earlier binding | example |
|--------|-------------------------------------------|---------|
| **reads** | borrows it, and nothing else | `a + 1`, `f(ref a)` |
| **takes** | moves it out | `let a2 = a`, `f(a)` |
| **overwrites** | writes it on every way through the unit before anything reads it | `set b = 10`, `f(out b)` |
| **updates** | any other write | `set b = b * 2`, `f(mut b)`, `set p.1 = x`, a write under `if` |
| **revives** | writes a `var` an earlier unit gave away | `set b = 600` after a take |

`UnitLowerRecord` has `takes`, `writes` (overwrites and updates together),
`overwrites` and `revives`. Not "effects": that word is for consequences.

**given away**. What a taken binding is, as the user sees it: D013, "`a` was
given away by an earlier unit". "Moved" is fine in ownership-internal prose.

## Steps

**step**. One operation on the session, an append, an edit, a delete, an
insert, a truncation or a module edit, and what it did to each unit.
`UnitStep`, `Engine::last_step`.

**compiled again**. The step ran the unit's parse, typecheck or ownership
analysis: `UnitStep::compiled` (`Compiled { parse, typecheck, ownership }`). A
salsa memo hit is not compiling again. Lowering is not counted. Not
"analyzed". The History tab says "compiled again".

**ran again**, **reran**. The step ran the unit: `UnitStep::eval` is `Some`.
A unit compiled again need not run again -- its lowering may be unchanged --
and the History tab says "no other unit reran" when none did.

**re-derive**. Engine-internal: lower a unit again and run it, after compiling
it if salsa asks to.

## The algebra

**retract**, **apply**. The two halves of a modification. Retract reverses
what the unit's previous code did to earlier bindings; apply runs its current
code. "Undo" and "redo" are not used for these.

**Edit**, **Break**, **Delete**, **Fix**, **Add**. The modifications.
Capitalised, Edit is an edit that compiles (retract, then apply); Break one
that does not (retract); Fix one that makes a unit compile that did not
(apply). Delete is retract and Add apply. A lower-case edit is any change to a
unit's code.

**forward**. What a step does to the units after the modified one: rerun those
that read a changed value, break or fix those whose names it took or gave
back.

**backward**. What a step does to units before the modified one, to restore
bindings it reads or changed. Not "back" or "reverse".

**rerun**. Restoring a binding by running units again: the nearest unit before
that sets the binding without reading it (its provider, or a later overwrite
or revival), then the units between that took or wrote it. In a step table's
because column, `rerun:`. Also the plain verb for running a unit again.
`ScriptExecutor::rerun_unit`.

**rerun start**. The unit a rerun begins at. For an edit it is the nearest
setter; a splice uses the provider (`ScriptCompiler::rerun_start`). Distinct
from the provider only when an overwrite or revival is nearer.

**fixup**. Restoring a binding without running anything, possible when it
held nothing, as before a revival: `` `b` dropped `` in a step table, `fixup:`
in its because column.

"Rewind" and "replay" are retired from the algebra; both were reruns.

## Unit states

How a unit fared the last time it was compiled or run. The REPL colours its bar
by state.

| state | engine `Eval` | app `UnitStatus` | bar |
|-------|---------------|------------------|-----|
| **ran** | `Ran(bindings)`, `Value(..)` for an expression unit | `Ran` | green |
| **did not compile** | `NotCompiled` | `NotCompiled` | red |
| **returned early** | `ReturnedEarly` | `ReturnedEarly` | yellow |
| **blocked** | `Blocked { on }` | `Blocked` | magenta |
| **waiting** | -- | `Waiting` | cyan |

A runtime error (`Eval::RuntimeError`) and an engine panic (`Eval::Crashed`)
are not unit states: they empty the session. `ScriptExecutor::unit_status`
holds the engine's view.

## REPL words

**outcome**. What a command came to: `repl::Outcome`, which is the engine's
`Eval` for a unit or a REPL command's answer.

**undo**, **redo**. Only the REPL's `/undo` and `/redo` commands, which take
back the last change to the session and make it again (`ReplCommand::Undo`,
`ReplCommand::Redo`, `Change`). Not the algebra's retract and apply.

**replay**. Only re-submitting saved REPL inputs, from `/load` or
`repl --load` (`ReplApp::replay`). Not recompiling a session from scratch, and
not a rerun.

**inputs**. What the REPL's history and a saved session hold: units and
commands both. **entry**: one item of the REPL's history.
