# Novelties

Datalove occassionally has unique designs:




## Matched-brace token-tree lexing

Matched-brace / token-tree lexing:
all braces are matched (`{ }`, `( )`, `[ ]`, `< >`).
Statement oriented, with statements that span multiple lines without statement terminators.
Pascal, Python, and Rust-influenced syntax.

Rust has token trees, but not for `< >`.

Datalove consequentially has to sacrifice comparison ops and arrows,
using `.<` and `.>` for comparison.




## Bimodal parsing — line-oriented to expression-oriented

Datalove leverages token trees to create a novel parser
that can line-break outside of braces while leaving the inside un-line-breaked.

This is what allows statements without separator tokens:

```datalove

let a = 0         // line-break

fun foo (
  a: int,         // no line-break - inside token branch
  b: int,
)                 // line-break
end fun
```




## Pervasive prefix type hints

Datalove's type hints have no direct precedent:

```datalove
let foo = (
  1,
  : int / 2,
  3,
)
```

Being a prefix is particularly unusualy.
We do it this we because it matches the "pull" feel of bidirectional typing,
where types flow into the goal.

Postfix type annotations are the opposite, less readable.




## Tensors and tables

These two types are pervasive in big niches of modern
computing but rarely recieve first-class language language support.

```datalove
todo
```




## Fully-memoized compilation

This is becoming common for production compilers,
required for quick iteration and IDE feedback.

Datalove's compilation pipeline is fully memoized
at the module and script-unit level:
parsing, typechecking, ownership analysis, lowering.




## Fully-reversible REPL

Datalove's REPL is reimagined for modern compiler pipelines,
with unified script and REPL compilation and evaluation.

REPL sessions are composed of a sequence of _script units_,
each of which is a function frame without arguments.

Script units are typechecked in sequence,
environment carried from previous to next.

Script units can be _undone_,
reversing the effects of both typechecking and evaluation.

For pure functional environment changes between units
undo is accomplished with memoization.
(For side-effecting functions this will be done through virtualized I/O).




## Enum variant types

Enum variants are themselves types.




## Worldfiles






