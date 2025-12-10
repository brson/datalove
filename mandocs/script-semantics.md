# Datalove (and Datafun) script semantics

The semantics of scripts are going to be designed to
such that the requirements of a modern repl map in an obvious way to
salsa's incremental computation.

Basic repl requirements:

- line-orientation
- incremental compilation
- incremental execution

I think the way to do this is to make
every statement submitted a unit of parsing and typechecking,
keep them all in either a linked list or vec,
salsa will make sure the previous computations are memoized and fresh.
The repl history is fully reactive, its compilation
and execution re-calculated (but memoized) every step.

For datafun's pure type system this should be straightforward.
Full datalove is not pure and will introduce compliations to solve later.

Probably we should even hold onto failed typechecks
and just not include them in subsequent calculations,
so we can still roll back to those steps.
Also we're going to want to hold onto commands that are not code,
but that switch on and off modes, etc.
We're going to be replaying history, everything must be deterministic.


## Accepting repl input

The repl frontend doesn't understand datalove.
It just reads lines and sends them to the engine.
The engine runs the statement parser
and if it has a complete statement looks at it:

if it is not a `fun` statement,
tells the caller to submit it for evalution,
else keep reading lines.

if it is a fun statement,
and we were already parsing a fun statement,
error.

if it is a fun statement and we were not already parsing
a fun statement,
push it to the function statement stack
and report to the user to keep parsing lines
because we are now in a fun statement.

ifc it is a `end fun` statement, and we're parsing a fun
then we have a full script unit, tell the user to submit the whole thing,
else error.


## Compilation

The script input to salsa consists of something like:

```
struct Script {
    units: Vec<ScriptUnit>,  
}

struct ScriptUnit {
    statements: Vec<InternedText>,
}
```

Not sure what else yet.

For parsing, all script units are independent.
For resolution and type checking we start with two types of statements: fun and let.

Their treatment is not the sam.


### Name resolution - resolve function statements

Function statement resolution is bidirectional,
supporting forward references. Functions can
name other functions but that's it (for now).

Algorithm:

Remember this is all memoized, not expensive.

- filter out everything but function units
- for units in oldest to newest
  - create a candidate list of all previously "green" units
  - append the current unit
  - name resolve all candidate unit together
  - if that succeeds this unit is green,
    append it to the list of green units.
  - if not it is dead -
    we will never consider this unit for compilation in future steps
    (we could consider extensions to revive it, but not now for simplicity)
- if the last unit passed resolution,
  then script resolution succeeded.

There is a question of what to do about function redeclarations.
For now we will consider that newer declarations win
and will also have to filter out duplicates, tbd.


## Name resolution: let statements

Let statements get to use the result of function resolution,
resolved bidirectionally into the future (from old let statement's persective).
Let statements _do not_ do forward resolution on other let statements.

So we're going to re-resolve and re-typecheck every let statement every step.
Still, should be pretty fast.
    
## Typecheck

tbd
