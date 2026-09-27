# Reserved words: what the implementation did, and what it does now

The recommendation at the end was taken, with statement-start words reserved
for function names and collection names freed for aliases; botspec section 2.1
is the rule as implemented. What follows is the review that led there.

A review of which words datalove treats specially, where, and what happens
when a program uses one as a name. Nothing here takes the botspec's keyword
list, `is_primitive_name`, or the parsers as canonical; each was checked
against the others and against running programs (2026-09-27).

## Datalove has no keywords, only positions

No word is special everywhere. Every special word is special in a position:
the start of a statement, the start of an expression, after an operand, in a
type, inside a construct. A word is a problem only where it can stand in the
same position as a name and mean something else there. So the useful question
is not "is this a keyword" but "for which kind of name, if any, does this word
collide".

The kinds of name a program chooses:

| Namespace | Declared by | Read in |
|---|---|---|
| value | `let`, `var`, `const`, parameters, `case term T x` bindings | expressions |
| function | `fun`, `native fun` | calls, in expressions and as statements |
| type | `type` aliases, type parameters `<T>` | types |
| module | `require`, `import`, module and package aliases | import paths, qualified calls |
| field / column | struct and table literals and types | projections `.x` |
| variant | `atom A`, `term A`, enum variants | after `atom`/`term`, `case` |

## The special words, by position

From every comparison against a fixed word in `datalove-datafun-parser`,
`datalove-datalit/src/parser` and `bcts`:

| Position | Words |
|---|---|
| start of an expression | `true` `false` `none` `some` `ok` `er` `data` `error` `atom` `term` `enum` (before `{`) `not` `icall` |
| after an operand | `and` `or` `xor` |
| start of a statement | `let` `var` `const` `set` `fun` `native` `ret` `if` `match` `loop` `break` `continue` `require` `import` `type` `debuglog` |
| inside a construct | `else` `end` `case` `default` `while` (after `loop`) `with` `is` (bounds) `module` `rider` `data` (after `require`) `mut` `out` `ref` `const` (parameter and argument modes) |
| in a type | `bool` `u8`..`u64` `i8`..`i64` `index` `offset` `f32` `f64` `int` `string` `data` `error` `atom` `term` `enum`; and, as refusals with a suggestion, `tuple` `list` `map` `set` `table` `tensor` and any miscasing of a primitive (`Int`, `String`, `Error`) |
| import paths | `pkg` |

Nothing reads `for`, `in` or `table` as a keyword, though the botspec lists
them. The botspec omits `const`, `set`, `debuglog`, `native`, `module`,
`rider`, `with` and `is`.

## What happens when each is used as a name

Each word was declared and then used in each namespace. `ok` means it worked;
anything else is what went wrong.

**Values.** The expression-start words collide, and nothing refuses them where
they are declared, so the failure shows up at the use, or not at all:

- `let true = 7; debuglog true` prints `true`. The binding is accepted and
  silently never read. Likewise `false`.
- `let some = 7` binds, and `some` cannot be read back (P008); likewise `ok`,
  `er`, `data`, `error`, `not`, `icall`, and `none` (F011).
- `let atom = 7` and `let term = 7` are parse errors at the binding.
- Parameters and match bindings behave the same way.
- Parameters named `mut`, `out`, `ref` or `const` are P011: the mode prefix
  wins.
- Everything else, including `if`, `fun`, `type`, `and`, `enum`, `list`,
  `int`, works as a value name.

**Functions.** Two separate collisions:

- A function named after an expression-start word is silently not called:
  `fun some(): int` then `some()` evaluates to `some ()`, an option holding
  unit. Likewise `data()`, `error()`, `ok()`.
- A function named after a statement-start word cannot be called as a
  statement: `set(7)` is read as a `set` statement, `if(7)` as an `if`. It can
  still be called inside an expression. `debuglog(7)` "works" only because the
  builtin prints the same thing.

**Types.** Aliases and type parameters collide with every type-position word:

- `is_primitive_name`, which refuses an alias's name, lists the primitives
  plus `tuple`, `enum`, `map` and `set`. It does not list `list`, `table`,
  `tensor`, `atom` or `term`, so `type list: u32` is accepted and the alias can
  never be used. Type parameters are not checked against it at all.
- The refusal it does make has no diagnostic: `type u32: u32` prints
  `Type error: CannotShadowPrimitive("u32")`, and `type enum: u32` prints an
  empty `Type error:`.
- The parser's "did you mean" refusals run before any alias is looked up, so
  the natural alias names `Error`, `Data`, `String`, `Index`, `List`, `Map`,
  `Table` and every other miscased primitive or collection name can be declared
  and never used, as an alias or a type parameter. `Atom` and `Enum` work,
  because only the lowercase words are special.

**Modules.** The standard library's modules are named `bool`, `int`, `u8`,
`list`, `map`, `set`, `string`, `tensor` and so on, and that works because
module names are read only in import paths. Nothing should be reserved here.

**Fields, columns, variants.** Every word tested works. A field or column name
appears only where a name can, and a variant name only after `atom` or `term`.

## Recommendation

Reserve a word for a namespace exactly when it can appear where that
namespace's names are read and mean something else, refuse it where the name is
declared, and nowhere else.

| Namespace | Reserved | Why |
|---|---|---|
| value | `true` `false` `none` `some` `ok` `er` `data` `error` `atom` `term` `not` `icall` | start an expression where a value name would be read |
| function | the value list, plus the statement-start words | calls read like values, and call statements start where statements do |
| parameter | the value list, plus `mut` `out` `ref` `const` | the mode prefix is read where the name would be |
| type | the primitive type names and `atom` `term` `enum` | they are types, or start one, where an alias would be read |
| module, field, column, variant | nothing | no position lets a word there mean anything else |

This drops `tuple`, `map`, `set` (and `list`, `table`, `tensor`, never
reserved) from the type namespace: they are not types, and a program is free to
call an alias `table`. The collection-name and miscasing hints move from the
type parser to where a type name fails to resolve, so they fire only when no
alias of that name exists: T059 in datalit, the unknown-type error in datafun.
`enum` stays reserved for types but not values, since as an expression it is
special only before `{`.

`and`, `or`, `xor` need nothing: they are read only after an operand, and a
value named `and` already works everywhere. The construct words (`else`, `end`,
`case`, `with`, `is` ...) need nothing either.

Separately:

- the refusals need diagnostics (the two alias errors above print nothing
  useful);
- the botspec's keyword section should be replaced by the table above, and
  `for`, `in` and `table` dropped from it;
- `is_primitive_name` should be the one list the type parser and the alias and
  type-parameter checks share, rather than one of three copies.
