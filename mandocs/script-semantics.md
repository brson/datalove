# Datalove script semantics

Datalove is REPL-first,
and with its strong linear types and functional purity,
plus virtualized I/O,
we intend to make it do some sophisticated things like rewind and replay.

The semantics of the REPL will be the semantics of scripts,
with the exception that scripts are not interactive,
so won't exercise as many features.

Most of this document deals with representing scripts
in a way that is repl-compatible.


The semantics of scripts are going to be designed to
such that the requirements of a modern repl map in an obvious way to
salsa's incremental computation.

Basic repl requirements:

- line/statement-orientation
- ability to interpret standalone expressions
- incremental compilation
- incremental execution

For our purposes I don't think there's any technical reason
why we need to limit input to single statements, so our underlying
script input will be:

- The package world - normal modules
- A growable list of _script units_ wich are either:
  - A _script fragment_, one or more statements,
    `let`, `fun` declarations, etc.
  - A single bare expression

These units typechecked and interpreted in sequence
form a dynamic repl session. The script interpreter when run in
non-interactive mode can accept this same mixture of interleaved
script fragments and expressions, though in practice they will
usually recieve exactly one script fragment.

They must be retained and re-typechecked together because
our linear type system will move and invalidate old slots etc,
so previously defined variables will be invalidated etc.
Retypechecking will be cheap with memoization.

We will make every script unit submitted a unit of parsing and typechecking,
keep them all in either a vec,
salsa will make sure the previous computations are memoized and fresh.
The repl history is fully reactive, its compilation
re-calculated (but memoized) every step.
Intent is that execution will be memoized too for rewinding;
this is relatively straightforword for pure functions (datafun `fun`s),
requires virtualized I/O for non-pure functions (datalove `proc`s).

Script units that fail typecheck or that have been rewound,
will be maintained within the repl, but removed from the salsa input;
this way they can be replayed later if needed. The interpreter
doesn't need to worry about them though.

For the purposes of this document,
we are only considering datalit/datafun types,
all of which can be cloned.
We'll redesign for non-clonable types later.




## Accepting repl input

This is just background about repl interaction with script.

The repl frontend doesn't understand datalove.
It just reads script units and sends them to the repl engine,
which wraps the compiler and interpreter.
The repl engine runs the script parser,
and if it parses:

- if only whitespace tokens, no-op; UI input field reset
- if no statements parsed (only comments etc), error

If the input didn't parse:

- it tokenizes the input and looks at the first non-whitespace/comment token
- if it _is not_ a statement keyword the input is an expression
- if it _is_ a statement keyword, report the parse error




## Compilation

The script input to salsa consists of something like:

```
#[salsa::input]
struct Script {
    units: Vec<ScriptUnit>,  
}

#[salsa::input]
enum ScriptUnit {
    ScriptFragment(ScriptFragment),
    ScriptExpression(ScriptExpression),
}

#[salsa::input]
struct ScriptFragment {
    statements: InternedText,
}

#[salsa::input]
struct ScriptExpression {
    statements: InternedText,
}
```



## Script processing


NB: BELOW THIS IS OLD IDEAS

---



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
