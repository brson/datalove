# One Rule for Literals That Share a Type

How datalit and datafun type a literal that has no expected type but sits
beside others that must share its type: the elements of a collection, the
operands of a binary operator, the arguments of a generic call. Written
October 2026 after a review of both checkers, and implemented in both the same
month. The rules as built are stated in
[Datalit Typing Rules](datalit-typing-rules.md) and
[Datafun Typing Rules](datafun-typing-rules.md), and for users in
[Datalove Literals](../mandocs/datalit.md#user-content-typing-together); this
document keeps the argument for them and what is left.

## Contents

- [The problem](#user-content-the-problem)
- [What a literal is](#user-content-what-a-literal-is)
- [Empty collections stop synthesizing](#user-content-empty-collections-stop-synthesizing)
- [Classes of variable](#user-content-classes-of-variable)
- [One rule for sharing a type](#user-content-one-rule-for-sharing-a-type)
- [In linear time](#user-content-in-linear-time)
- [What changes for the user](#user-content-what-changes-for-the-user)
- [What is left](#user-content-what-is-left)
- [Open questions](#user-content-open-questions)

## The problem

Both checkers are bidirectional. A literal checks against every type that
holds its value, and synthesizes one type when nothing is expected: `5`
checks against `u8` and synthesizes `int`. That works as long as an expected
type reaches every literal. But three places have several expressions that
must share one type and nothing outside them to say what it is, and each
place settles it differently:

| Site | Rule before | Failed on |
|---|---|---|
| Collection elements | every element synthesizes; all must equal the first | `[: u8 / 1, 2]`, `[some 1, none]`, `[[], [1]]` |
| Binary operands | four ranked cases, `Pair-Adapt`, `Pair-Cons`, `Pair-Lit`, `Pair-Syn` | hard to state; only two operands |
| Generic arguments | the first argument that synthesizes fixes `T`; the rest check | `pick_first(99, : u32 / 1)`; exponential when nested: 14 s at depth 15 |

On top of these, the empty collections synthesized with `()` for whatever
type they left open: `[]` was a `[()]`. That choice is arbitrary, and it
leaked. With `ys: [u32]`, `list.concat(ref ys, ref [])` checked and
`list.concat(ref [], ref ys)` was F016, "expected `[()]`, found `[u32]`",
because `[]` fixed `T = [()]` before `ys` was reached. `[[], [1]]` reported
"expected `[()]`, found `[int]`", which names a type nobody wrote.

Therefore: empty collections stop synthesizing, and one rule, stated once,
types every group of expressions that must share a type. The rule is the
standard one for untyped literals (Go's untyped constants, Haskell's numeric
defaulting, Rust's `{integer}`), and it is implemented with literal type
variables so that it costs linear time.

## What a literal is

The principle the rest follows from:

> An expression synthesizes a type only when its form determines the whole
> type. A literal checks against every type that holds the value it writes.

A literal is not a value of a subtype. `5` is in `⟦u8⟧` and in `⟦int⟧`, and a
`u8` *value* never becomes an `int` without `@`. `atom A` is in `⟦E⟧` for
every enum `E` listing it, and an `atom A` *value* never becomes an `E`
without `@`. Integer and atom literals follow the same principle; they differ
only in direction. `5` synthesizes the largest type it fits, `atom A` the
smallest, because there is a type of all integers and no enum of all atoms.

## Empty collections stop synthesizing

`[]`, `#{}`, `%{}`, an empty tensor (`[| |]`, `[| 0 3 | |]`) and a table with
columns but no rows (`{| x, y |}`) each leave an element or column type open,
so none of them synthesizes. Each joins `none`, `er e` and `enum {v}` as a
form that needs an expected type: T016 in datalit, F011 in datafun.

What the syntax fixes stays fixed. `[| |]` is rank 1 and `{| x, y |}` has the
columns `x` and `y` by syntax, so the rank and column checks (T048, T054,
T055) are unchanged. The zero-column table `{| |}` leaves nothing open and
synthesizes `{| |}`, as `()` and `{}` do.

The requirement passes through every position that synthesizes, as it does
for `none`: `data []`, `some []` and an unhinted `{ xs = [] }` each need a
hint somewhere above the `[]`.

## Classes of variable

The four `Pair-*` rules ranked operands by how far their synthesized type
could be trusted. The ranking was real, but it was computed from syntax. It
can be read off the types instead, once a literal's type is a variable with a
class saying what the variable may become and what it becomes by default:

| Expression | Inferred type |
|---|---|
| integer, hex, float literal | a variable of class `int`, `hex`, `float`, defaulting to `int`, `int`, `f64` |
| `atom a`, `term a e` | a variable of class `atom a`, `term a τ`, defaulting to the singleton type |
| `e@` | a variable of class `adapt T`, for `e` of type `T`: any type `T` adapts to, defaulting to `T` |
| `none`, `er e`, `enum {v}`, empty collection | a variable of class `open`, with no default |
| `some e`, `ok e`, tuple, struct, non-empty collection | built from its parts' types |
| variable, call, projection, `data e`, `error e`, anything hinted | its type, with variables only for a generic call's parameters |

A variable takes a type only if its class admits it: an integer literal
takes `u8` but not `string`. Two variables merge only if some type could be
of both classes: an integer and a hex literal merge, an integer and a float
literal do not, and neither do two `@`s of different types.

`e@` was planned to be definite. It is not, because `let s: int = a@ + b@`
has to pass `int` to both operands, and a definite `a@` would have refused
it. Its class keeps it from being anything but its own type unless something
asks, so `a@ .< 0` compares at `a`'s type, as planned.

## One rule for sharing a type

> Expressions that must share a type are typed together. Their types are
> unified; each literal takes the type unification gives its variable, and a
> variable nothing decides takes its class's default. If a variable has no
> default (`none`, `[]`), the expressions need an expected type.

The rule applies at every site in the table above, and it replaced the
`Pair-*` rules, the first-element rule for collections, and the first-
argument rule for generic calls. A generic call is the same thing with a
variable for each type parameter, unified with the arguments and, in checking
mode, with the expected result. An intrinsic call's arguments are now checked
against its parameters, as any call's are.

Because unification decides each position wherever the deciding part is, the
rule does not depend on order. `[(1, : u8 / 1), (: u8 / 2, 2)]` is
`[(u8, u8)]`: neither element is definite as a whole, but each decides one
position. A rule of "first definite element wins" would refuse it.

Where two parts disagree, unification leaves them apart rather than failing,
and the first in source order keeps the shared type. Checking then reports
the later one against it, as before: `[1, 2, "three"]` is reported at
`"three"`. Two operands that come to different types are F016, "operands must
have the same type", as before.

## In linear time

The direct implementation of the rule was: synthesize each element, choose
the type, then check again each element that did not already have it. The
second check is a walk of the element. A large defaulted element beside a
small definite one, at every level of nesting, makes each level walk
everything below it again. With a definite element whose hint is as deep as
the nesting, a datalit input of size `n` costs about `n^1.5`. Data files are
written by programs, and anything a program can write, one eventually will.
So the re-walk was not acceptable even as a rare case.

What was built instead synthesizes in three steps:

1. **Infer.** A walk of the expression gives each literal and each open
   expression a variable, and unifies the types of what must agree, with
   union-find on the variables. It reports nothing: a disagreement is left
   as it is.
2. **Default.** Each variable nothing decided takes its class's default. One
   with no default is the "needs a type" error, at the expression that left it
   open.
3. **Check.** The expression is checked against the type that results. This
   is the ordinary checking walk, which reads each literal as its type,
   checks ranges and enum membership, and reports any disagreement inference
   left, with the diagnostics checking already had.

Datalit synthesizes this way at the root and nowhere below it, so a document
is walked twice in all. Datafun infers an expression once, the first time
anything asks for its type, and keeps what it inferred for every part;
checking reads those types instead of inferring again. That is what removed
the exponential case: a generic call used to synthesize each argument to bind
its type parameters and then check it, so each level of nesting did twice the
work of the one below, and `list.of(...)` nested 15 deep took 14 seconds. It
now takes under a tenth of a second at depth 80.

The variables are local to the expression being synthesized. They never
reach a binding: `let xs = []` is an error, and nothing infers `xs` from a
later `push`.

[A Runtime Datalit Loader](plan-datalit-loader.md) would be a second reader
of datalit text with the same typing. Its typing has to be this one too, and
union-find over a flat array is a natural fit for a reader that does not use
salsa.

## What changes for the user

| Expression | Before | After |
|---|---|---|
| `[]`, `let xs = []` | `[()]` | needs a type |
| `[: u8 / 1, 2]`, `[2, : u8 / 1]` | T018 | `[u8]` |
| `[some 1, none]`, `[none, some 1]` | T016 | `[?int]` |
| `[[], [1]]` | T018, "expected `[()]`" | `[[int]]` |
| `[(1, : u8 / 1), (: u8 / 2, 2)]` | T018 | `[(u8, u8)]` |
| `list.concat(ref [], ref ys)` | F016 | `[u32]` |
| `pick_first(99, : u32 / 1)` | F016 | `u32` |
| `let y: u32 = id(5)` | F016 | `u32` |
| `list.len(ref [])` | `0`, of a `[()]` | F011 at the `[]` |
| `icall add_wrapping_u32(a, 2)` | F016 | `2` is a `u32` |
| `x == 0`, `0 == x`, `x == none`, `e == atom A` | check | unchanged |
| `a@ .< 0` with `a: u32` | compares at `int` | compares at `u32` |
| `a@ .< b@` with `u32` and `u64` | F016 | unchanged |
| `const X: i32 = 10 + 5` | F016, expected `i32`, found `int` | F026, bare `+` on `i32` |
| `data []` | holds a `[()]` | needs a hint |

The `a@ .< 0` row is the one change that is not a relaxation. `a .< 0`
already compared at `u32`, so `a@ .< 0` was only ever useful to compare at
`int`, and that is written `a@ .< : int / 0`.

Datalit's diagnostic codes for an element that differs from the first (T018
to T021, T051, T052, T060) are retired: the check reports the element with
the check's own code.

The value printer gets a rule for where hints are needed, which
[Canonical forms](../mandocs/datalit.md#user-content-canonical-forms) now
states: the root needs a hint when its type is not the one its literals
default to, and an empty collection needs one when nothing around it fixes
its type. The printer does not follow it yet.

## What is left

- **Recorded types are copies.** Datafun records a cloned `Type` per
  expression, and inference keeps a type per expression the same way. Shared
  or interned types would make the record linear; see
  [issues](issues.md#user-content-datafun-records-a-copy-of-every-expressions-type).
- **No depth limit.** Inference recurses on the expression, as checking does,
  and both overflow the stack on deep enough nesting; see
  [issues](issues.md#user-content-deeply-nested-literals-overflow-the-stack).
- **Bounds do not take part.** A bound such as `T is float` is checked after
  `T` is found, so `lerp(0, 1, 0.5)` makes `T` `int` and is refused, where a
  class for the bound would have made it `f64`.
- **A fallback keeps the old collection rule.** Where datafun inference
  cannot read part of an expression, such as a name that is not bound,
  synthesis falls back to the per-form rules, which still type a collection
  by its first element. They exist to report the unreadable part, which they
  do, and the first-element rule decides nothing that reaches a program.
- **The value printer** writes no hints where the rule above says they are
  needed.

## Open questions

- Should a disagreement blame the earlier part or the later one? The first
  in source order keeps the shared type, so the later is reported. A hinted
  part might be the better one to trust, since a hint is what the user wrote
  on purpose.
- Should bounds be classes, so `lerp(0, 1, 0.5)` is an `f64` call? A class for
  `float` would admit `f32` and `f64` and default to `f64`; joining it with an
  integer literal's class would have to be refused, as joining a float and an
  integer literal is.
- Is `enum {v}` worth keeping? A bare atom or term already checks against an
  enum. The wrapper's only effect is to refuse a non-enum expected type.
- What depth limit, if a limit rather than a work stack? Data written by
  programs can nest arbitrarily, so a limit is a format restriction, and the
  spec would have to state it.
