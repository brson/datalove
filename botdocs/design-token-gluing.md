# Design: Token Gluing and Operator Fixity

Status: Implemented
Date: 2026-09-14

## Overview

Two rules that both read one fact - whether two tokens were written with
nothing between them.

- **Gluing** decides what a single literal may span. `1.5` is a float and
  `1 . 5` is not.
- **Fixity** decides what an operator attaches to. `a - b` and `a-b` subtract,
  `a -b` is `a` followed by `-b`.

They are separate rules with separate jobs, and neither mentions any
particular container. Together they settle a list of questions the two
languages currently answer by accident, or answer differently from each other.

## Why

The lexer breaks a word at character-class boundaries and never puts one back
together, so `1.5` arrives as `Word "1"`, `Sigil(.)`, `Word "5"`. Whitespace
and comments are filtered out before a parser sees a token, which leaves the
spans as the only evidence that two tokens touched. Nothing in either parser
consults them today, so:

- `1 . 5` parses as the float `1.5`, `2.5e - 10` as `2.5e-10`, and `- 5` as
  the integer `-5`, none of which the grammar in `datalit-ebnf.md` allows -
  `float_lit` and `int_lit` have no `ws` anywhere inside them.
- A float assembles across a line break: `[1 .` newline `5]` is a
  one-element list holding `1.5`.
- `1u8` is reported as `unexpected identifier '1u8'`, which does not mention
  numbers.
- `[| 1 -2, 3 4 |]` is a 2x2 tensor in datalit and a shape error in datafun,
  because datafun's element parser reads binary operators and datalit's does
  not. The same literal syntax gives two answers.

The last one is the only place in the language where members are separated by
nothing: every other run is delimited by `,`, `;` or a newline, so no amount
of spacing can change how many members it has. A tensor's innermost axis is
separated by whitespace, and whitespace is exactly what the parser cannot see.

## Gluing

Two tokens are glued when the first one's span ends where the second one's
begins. A comment between them leaves a gap, so `1 /*x*/ . 5` is not glued,
which is the wanted answer for free.

A numeric literal is a maximal-munch glued run: an optional sign, digits, a
`.` and a fraction, an exponent marker with its sign and digits, and a
trailing run of letters. Any gap ends the literal.

```
-1.5e-7   one literal
1 . 5     the integer 1, and then a `.` that is not part of it
2.5e - 10 a float that ran out at the exponent
50pct     digits 50 with the suffix `pct`
```

The suffix is reported rather than interpreted. datalove has no numeric
suffixes, so it rejects any suffix and says what to write instead; a caller
that has units reads them off the same field.

A `.` written against the digits commits to a float whatever follows it, so
`1.foo` asks for the digits of a float. One with a space before it commits
only where digits follow: `1 . 5` is a float written apart from itself, and
`1 .foo` is a field read written apart from its base, which the postfix rule
below complains about instead.

## Fixity

Swift's rule. An operator's spacing says what it attaches to:

| position | reading |
| --- | --- |
| no left operand | prefix, whatever the spacing |
| left operand, glued both sides or spaced both sides | infix |
| left operand, spaced left and glued right | prefix |
| left operand, glued left and spaced right | postfix |

The first row needs no code. A binary operator is only looked for after an
operand has been parsed, and a prefix operator is only looked for when none
has, so the call site already answers it. That is what makes `f(-1)`,
`lcm(-4, 6)` and `ret -1` keep working: there is no operand to the left of
those, so the spacing is not consulted.

Fixity is for sigil operators. A word operator - `and`, `or`, `xor`, `not` -
is delimited by being a word, and gluing it to an operand would make it part
of the operand.

Postfix operators are the mirror: `?`, `!`, `@`, `.field` and `[index]`
require gluing on the left. `x?` is a try, `x ?` is not.

### What this does to tensors

Nothing, directly. The tensor parser is unchanged: it still asks for an
expression and asks again until the tokens run out. But `[| 1 -2 |]` now ends
the first expression at `1`, because the `-` is spaced on its left and glued
on its right and so is not a binary operator. The second call starts at the
`-`, which has no left operand, and reads `-2`.

| written | reading | elements |
| --- | --- | --- |
| `1 -2` | prefix | two |
| `1 - 2` | infix | one |
| `1-2` | infix | one |
| `-1 -2` | prefix, prefix | two |

datalit reaches the same answer by a different route, because it has no
binary operators at all and its literal takes the glued sign. The two
languages agree on every spelling.

## The intersection

The literal reader runs first and takes what is glued to it; fixity only
judges the operators it declined. That is why `2.5e-10` is one number - the
inner `-` is consumed as part of the literal before any operator is looked
for - while `x-1` subtracts, the reader never being positioned on that `-`.

## Interface

In `bcts`:

```rust
// bracer.rs
impl<'db> TreeToken<'db> {
    pub fn span(&self) -> Range<usize>;
}

// parser_util.rs
pub trait TokenStream<'db> {
    /// The end of the last token consumed, absent at the start of a run.
    fn prev_end(&self) -> Option<usize>;
    /// The token after the one at the cursor.
    fn peek_next(&self) -> Option<&TreeToken<'db>>;
}

pub trait TokenStreamExt<'db> {
    fn glued_left(&self) -> bool;
    fn glued_right(&self) -> bool;
    /// Whether the operator at the cursor is spaced as an infix one.
    fn is_infix_spacing(&self) -> bool;
}

pub enum Radix { Dec, Hex }

pub enum NumberError {
    Digits, FloatSpaced, FractionMissing, ExponentSpaced, ExponentMissing,
}

pub struct Number {
    pub negative: bool,
    pub radix: Radix,
    /// Whether a fraction or an exponent makes this a float.
    pub float: bool,
    /// The leading word, its radix prefix included and any suffix taken off.
    pub digits: String,
    /// The word after the `.`, which carries an in-word exponent with it.
    pub fraction: Option<String>,
    /// An exponent's sign and digits, where they were tokens of their own.
    pub exponent_tail: Option<String>,
    /// Letters written onto the digits, meaning whatever the caller says.
    pub suffix: Option<String>,
    pub span: Range<usize>,
    pub error: Option<NumberError>,
}

impl Number {
    /// The message and label for a number that was not written as one.
    pub fn complaint(&self) -> Option<(String, String)>;
    /// The literal as it was written, without its suffix.
    pub fn text(&self) -> String;
}

/// Read one numeric literal, as far as it is glued.
pub fn eat_number<'db, S: TokenStreamExt<'db>>(stream: &mut S) -> Option<Number>;

/// Whether a word begins a number, which a name cannot.
pub fn is_number_word(word: &str) -> bool;
```

A number is read whole and reported whole: the pieces are consumed even where
the spacing was wrong, so a parser makes one complaint about the number rather
than meeting its pieces again as something else. `complaint` lives in bcts so
that the two languages say the same thing, and quotes the number the reader
meant: `1 . 5` is answered with ``write it as `1.5```.

A suffix is a field rather than an error, since bcts does not know whether the
caller has units. datalove's own message for one is in
`datalove-datalit::parser_util::suffix_complaint`, shared by both its parsers.

There is no sign policy. `eat_number` always takes a glued leading `-`, and
whether it is ever asked is the grammar's business: datafun's prefix operator
claims the `-` first in expression position, and datalit has no prefix
operator, so there the literal owning it is the only reading.

`float_exponent`, `eat_exponent_tail` and `FloatExponent` become internals of
`eat_number`. `is_numeric_literal`, `is_decimal_run` and `strip_separators`
stay as they are.

## Diagnostics

Every one of these is a confusing downstream error without a message of its
own, so they are part of the change rather than a follow-up.

| written | said |
| --- | --- |
| `1 . 5` | a float is written without spaces: `1.5` |
| `2.5e - 10` | an exponent is written without spaces: `2.5e-10` |
| `1u8` | numeric suffixes are not supported; write `: u8 / 1` |
| `a -1` | this `-` is a prefix operator by its spacing; write `a - 1` or `a-1` |
| datalit `- 5` | the sign belongs to the number: write `-5` |

## What changes

| written | today | after |
| --- | --- | --- |
| `1.5`, `a-b`, `a + b`, `f(-1)`, `ret -1` | fine | unchanged |
| `1 . 5` | float `1.5` | error |
| `2.5e - 10` | float `2.5e-10` | error |
| datalit `- 5` | integer `-5` | error |
| datafun `- 5` | negation of `5` | unchanged, no left operand |
| `a -1`, `a- 1` | subtraction | error |
| `p . 0`, `x ?` | field, try | error, postfix must be glued |
| `1u8` | unexpected identifier | error naming suffixes |
| datafun `[\| 1 -2, 3 4 \|]` | shape error | 2x2, agreeing with datalit |

No datalove source in this repo changes meaning. Every asymmetric operator in
the tree is already a prefix one in a prefix position - `ret -1`,
`ret -self`, `ret -1.0`, `from_int(ref -123)`, `div_checked(-7, -2)` - and
there is no `a- b`, no spaced field access, no spaced postfix, no loose float
or loose sign, and no tensor element holding arithmetic. Worldgen emits
infix operators spaced on both sides and prefix ones glued, so its output
conforms already.

## Staging

1. The primitive: spans on tree tokens, `prev_end`, `peek_next` and the
   gluing predicates. No behaviour change.
2. `eat_number`, with both parsers switched onto it, and the literal
   diagnostics. This is where gluing starts being enforced.
3. Fixity in datafun's binary and postfix operators, with its diagnostic.
   Tensors agree as a consequence.
4. The docs: a spacing section in `datalit-ebnf.md`, a spacing line in
   `design-clone-and-coerce.md`, and `sigil-assignments.md` brought back up
   to date.

Steps 1 and 2 are conformance with the grammar already written down. Step 3
is the language commitment: whitespace becomes significant inside an
expression.

## Notes

- `require module /pkg/mod` is read by the statement parser rather than the
  expression grammar, so the leading `/` never meets fixity.
- A newline inside a branch is a gap like any other, so an expression split
  over lines inside parentheses keeps its operators spaced on both sides and
  keeps working.
- `a--b` reads as `a - (-b)`: the first `-` has an operand on its left and is
  glued on both sides, the second follows an operator and so has none.
