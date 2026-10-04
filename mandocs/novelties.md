# Novelties

Datalove occasionally has unique designs:




## Matched-brace token-tree lexing

Matched-brace / token-tree lexing:
all braces are matched (`{ }`, `( )`, `[ ]`, `< >`),
as are the earmuff braces (`{| |}`, `[| |]`)
and the sigil braces (`%{ }`, `#{ }`).
Statement oriented, with statements that span multiple lines without statement terminators.
Pascal, Python, and Rust-influenced syntax.

Rust has token trees, but not for `< >`.

Datalove consequentially has to sacrifice comparison ops,
using `.<` and `.>` for less-than and greater-than.




## Bimodal parsing — line-oriented to expression-oriented

Datalove leverages token trees to create a novel parser
that treats line breaks as significant outside of braces
and ignores them inside.
This is what allows statements without separator tokens.

```datalove

let a = 0         // line-break

fun foo (
  a: int,         // no line-break - inside token branch
  b: int,
)                 // line-break
end fun
```

In the case of tables though line breaks become significant
again within braces to create a CSV-like syntax without line terminators.

```datalove
let csvish = {|
  name: string, qty: int
  "coal", 10
  "iron", 20
|}
```

In cases where line breaks are significant,
`;` can also act as line terminators / statement seperators.

```datalove
let csvish = {|
  name: string, qty: int; "coal", 10; "iron", 20
|}
```




## Pervasive prefix type hints

Datalove's type hints work in every expression position
and have no direct precedent.

```datalove
let foo = (
  1,
  : int / 2,
  3,
)
```

Being a prefix is particularly unusual.
We do it this way because it matches the "pull" feel of bidirectional typing,
where types flow into the goal.
Postfix type annotations are deemed less intuitive
and are also more challenging to parse without disambiguating compromises.




## Tensors and tables

These two types are pervasive in big niches of modern
computing but rarely receive first-class language support.

```datalove
let identity: [|f64, 2|] = [| 1.0 0.0, 0.0 1.0 |]

let people: {| name: string, age: u32 |} = {|
  name, age
  "Ann", 31
  "Bob", 27
|}
```

Tensor rank is part of the type;
the shape is inferred from the literal's separators.




## Fully-memoized compilation

This is becoming common for production compilers,
required for quick iteration and IDE feedback.

Datalove's compilation pipeline is fully memoized
at the module and script-unit level:
parsing, name resolution, typechecking, ownership analysis, lowering.
It is built on Salsa.
The exception is const evaluation itself,
which runs the interpreter and is not memoized;
the phases either side of it are.




## Fully-reversible REPL

Datalove's REPL is reimagined for modern compiler pipelines,
with unified script and REPL compilation and evaluation.

REPL sessions are composed of a sequence of _script units_,
each of which is a function frame without arguments.

Script units are typechecked in sequence,
environment carried from previous to next.

Script units can be edited, inserted, or removed,
and a session truncated,
reversing the effects of both typechecking and evaluation.
An edit re-checks and re-runs only the later units
that depend on what changed.

For pure functional environment changes between units
this is accomplished with memoization.
(For side-effecting functions this will be done through virtualized I/O.)




## Enum variant types

Enum variants are themselves types.
An atom or term stands alone as a type,
and an enum is a closed union of them:

```datalove
type Shape: enum { atom Empty, term Circle f64, term Square f64 }

fun unit_circle(): term Circle f64
    ret term Circle 1.0
end fun

let c = unit_circle()     // c: term Circle f64
let s: Shape = c@         // widened to the enum
```




## Worldfiles

A whole world --
modules in any number of packages and libraries,
rider interfaces,
and a script --
can be written in a single text file,
which makes compiler tests self-contained.
Worldfiles can also describe a sequence of edits to modules,
for testing incremental recompilation,
and can be generated randomly (`datalove worldgen`)
for fuzzing the compiler.
See [Worlds](worlds.md).
