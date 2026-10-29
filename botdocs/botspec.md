# Datalove Language Specification

The reference for Datalit and Datafun as the compiler implements them,
audited against it in October 2026. Designs not yet implemented are listed in
Appendix B rather than described.

## Contents

- [1. Introduction](#user-content-1-introduction)
  - [1.1 Notation](#user-content-11-notation)
  - [1.2 Scripts, Modules and Sessions](#user-content-12-scripts-modules-and-sessions)
- [2. Lexical Conventions](#user-content-2-lexical-conventions)
  - [2.1 Reserved Words](#user-content-21-reserved-words)
  - [2.2 Literals](#user-content-22-literals)
  - [2.3 Comments](#user-content-23-comments)
  - [2.4 Spacing](#user-content-24-spacing)
  - [2.5 Names](#user-content-25-names)
  - [2.6 Scope](#user-content-26-scope)
- [3. Types](#user-content-3-types)
  - [3.1 Primitive Types](#user-content-31-primitive-types)
  - [3.2 Collection Types](#user-content-32-collection-types)
  - [3.3 Aggregate Types](#user-content-33-aggregate-types)
  - [3.4 Option and Result](#user-content-34-option-and-result)
  - [3.5 Data and Error Types](#user-content-35-data-and-error-types)
  - [3.6 Atom, Term, and Enum Types](#user-content-36-atom-term-and-enum-types)
  - [3.7 Type Aliases](#user-content-37-type-aliases)
  - [3.8 Recursive Types](#user-content-38-recursive-types)
- [4. Type Hints](#user-content-4-type-hints)
- [5. Copy and Linear Types](#user-content-5-copy-and-linear-types)
- [6. Expressions](#user-content-6-expressions)
  - [6.1 Operators](#user-content-61-operators)
  - [6.2 Arithmetic](#user-content-62-arithmetic)
  - [6.3 Comparison](#user-content-63-comparison)
  - [6.4 Logical Operators](#user-content-64-logical-operators)
  - [6.5 Try Operators](#user-content-65-try-operators)
  - [6.6 Operator Argument Semantics](#user-content-66-operator-argument-semantics)
  - [6.7 Intrinsic Calls](#user-content-67-intrinsic-calls)
  - [6.8 Adapt Operator](#user-content-68-adapt-operator)
- [7. Place Expressions and Indexing](#user-content-7-place-expressions-and-indexing)
  - [7.1 Place vs Value Expressions](#user-content-71-place-vs-value-expressions)
  - [7.2 Field Access](#user-content-72-field-access)
  - [7.3 Fallible Indexing](#user-content-73-fallible-indexing)
  - [7.4 Index and Field Chains](#user-content-74-index-and-field-chains)
  - [7.5 Destination Contexts](#user-content-75-destination-contexts)
  - [7.6 Parallel with Checked Arithmetic](#user-content-76-parallel-with-checked-arithmetic)
- [8. Statements](#user-content-8-statements)
  - [8.1 Bindings](#user-content-81-bindings)
  - [8.2 Const](#user-content-82-const)
  - [8.3 Set](#user-content-83-set)
  - [8.4 Functions](#user-content-84-functions)
  - [8.5 Parameter Modes](#user-content-85-parameter-modes)
  - [8.6 Const Parameters](#user-content-86-const-parameters)
  - [8.7 Generic Functions](#user-content-87-generic-functions)
  - [8.8 Control Flow](#user-content-88-control-flow)
  - [8.9 Return](#user-content-89-return)
  - [8.10 Debug Log](#user-content-810-debug-log)
  - [8.11 Call Statements](#user-content-811-call-statements)
- [9. Module System](#user-content-9-module-system)
  - [9.1 Hierarchy](#user-content-91-hierarchy)
  - [9.2 Require](#user-content-92-require)
  - [9.3 Import](#user-content-93-import)
  - [9.4 Qualified Calls](#user-content-94-qualified-calls)
  - [9.5 Native Riders](#user-content-95-native-riders)
  - [9.6 Data](#user-content-96-data)
- [10. Numeric Widening](#user-content-10-numeric-widening)
- [11. Ownership Analysis](#user-content-11-ownership-analysis)
  - [11.1 Errors](#user-content-111-errors)
  - [11.2 Mutable Arguments](#user-content-112-mutable-arguments)
  - [11.3 Argument Aliasing](#user-content-113-argument-aliasing)
  - [11.4 Loop Restrictions](#user-content-114-loop-restrictions)
  - [11.5 The Collection a `for` Walks](#user-content-115-the-collection-a-for-walks)
  - [11.6 Branch Consistency](#user-content-116-branch-consistency)
  - [11.7 Tracking Categories](#user-content-117-tracking-categories)
- [12. Bidirectional Type Inference](#user-content-12-bidirectional-type-inference)
- [Appendix A. Command-Line Interface](#user-content-appendix-a-command-line-interface)
- [Appendix B. Unimplemented Features](#user-content-appendix-b-unimplemented-features)

## 1. Introduction

Datalove is a statically-typed scripting language designed for data manipulation
and incremental computation. The language emphasizes safety through a linear
type system that tracks ownership, preventing use-after-move errors at compile
time.

The language has a three-layer design:

- **Datalit** (.dlt) - A pure data literal sublanguage for representing values.
- **Datafun** (.dfs, .dfm, .dli) - A pure functional layer with functions,
  modules, native rider interfaces, and control flow.
- **Full Datalove** (.dls, .dlm) - Procedures and mutable objects. (Not yet
  implemented.)

This specification describes the Datalit and Datafun layers.

### 1.1 Notation

In syntax descriptions, the following conventions apply:

- `monospace` denotes literal syntax
- *italics* denote syntactic categories
- `[...]` denotes optional elements
- `...` denotes repetition

The complete grammars, as EBNF checked against the parsers, are
[Datalit Grammar](datalit-ebnf.md) and [Datafun Grammar](datafun-ebnf.md),
and the type systems are [Datalit Typing Rules](datalit-typing-rules.md) and
[Datafun Typing Rules](datafun-typing-rules.md). They state formally what this
document states in prose; a disagreement between them is a bug in one or the
other.

### 1.2 Scripts, Modules and Sessions

A **script** (`.dfs`) is a program whose top level runs from top to bottom. A
**module** (`.dfm`) holds only declarations, for scripts and other modules to
use (Section 9.1). The REPL reads a **session**: a sequence of **units**, each
a line or fragment compiled as script code that continues the units before it,
so that a binding, function or `require` from an earlier unit is in scope in a
later one.

A script's top level, and each unit of a session, is checked as the body of a
function returning `!()`: the `!` operators work there, and the `?` operators
are F049, as in any function that does not return an option. An error returned
from a script's top level ends the script, which prints it.

## 2. Lexical Conventions

### 2.1 Reserved Words

Datalove has no words reserved everywhere. A word is special in a position --
the start of an expression, a type -- and is reserved only for the names that
are read in that position, where it would mean something else. It is refused
where such a name is declared (P064), and free everywhere else.

| Name | Reserved | Why |
|------|----------|-----|
| value (`let`, `var` and `const` names and patterns, `if`/`else` `\|name\|` and `case term` bindings, `for` names, the const `require data` binds) | `true` `false` `none` `some` `ok` `er` `data` `error` `atom` `term` `enum` `not` `icall` | they start an expression |
| parameter | the value words, and `mut` `out` `ref` `const` | the mode is written where the name is |
| function | the value words | a call is read like a value |
| type alias, type parameter, `rec` name | `bool` `u8`..`u64` `i8`..`i64` `index` `offset` `f32` `f64` `int` `string` `data` `error` `atom` `term` `enum` `rec` | they are types, or start one |
| module or rider (the alias `require` gives it) | the value words | a qualified call is read like a value |
| field, column, variant | nothing | no word means anything else there |

So a value may be called `type` or `list`, a function `set` or `match`, a type
alias `table` or `Error`, and a module `u8` or `set` -- the standard library's
modules are named after the types they serve.

Every statement begins with its own keyword, a call made for its effect
included (`call`, Section 8.11), so no name is ever read at the start of a
statement and the statement words are reserved for nothing. A new statement
keyword therefore never takes a name away from existing code.
`and`, `or` and `xor` are read only after an operand, and the words
inside constructs (`else`, `end`, `case`, `default`, `where`, `is`, `while`
after `loop`, `in` and `ref` after a `for`'s names, `module` and `rider` after
`require`) only where a name cannot be,
so none of them is reserved.

The lists are in `datalove-datalit/src/parser_util.rs`, read by the Datafun
parser and type checker. Datalit declares no names but fields, columns and
variants, so it reserves nothing.

A mode word written as a parameter's name, `fun f(mut: int)`, is read as the
mode and leaves the name missing (P011), rather than being refused as a name;
P064 is reached only past the mode, as in `fun f(mut mut: int)`. The same
holds at the start of an argument: with `let out = 1`, `f(out)` reads `out` as
the marker and finds no expression after it (P081). Such a value is passed
parenthesized, `f((out))`.

**A core principle: a special word is recognized by the word alone.** The
parser does not look past a word to decide whether it is acting as a keyword
or as a name. Where a word would collide with a name, the collision is settled
by reserving the word for that kind of name in the table above, never by a
lookahead rule that treats it as a keyword in some contexts and a name in
others. `enum` is reserved for values and functions for this reason, even
though only `enum {` is an enum literal. New syntax, and new kinds of name,
follow the same rule: extend the table rather than the parser.

### 2.2 Literals

**Integers** may be written in decimal or hexadecimal:

```datalove
42
0xFF
```

Bare integer literals synthesize as `int` (arbitrary-precision), and bare
float literals as `f64` (Section 12). Use a type hint for other types:
`: u32 / 42`. An integer literal is not a float literal, so `let a: f64 = 1`
is a mismatch (F016).

The hex prefix may be `0x` or `0X`, and hex digits are of either case. There
are no binary or octal literals: `0b101` and `0o17` are a `0` with a suffix
(P053).

A hex literal is an unsigned integer: it checks against `int`, the unsigned
fixed-width types and `index`, and against `f32` and `f64` as their bit
patterns. It does not check against a signed fixed-width type or `offset`.

A hex literal takes no sign. A `-` before one is the negation operator, as it
would be before a name, so `-0x10` is the `int` -16. Bare `-` is not defined
on fixed-width integers, so `-0x10` is never an `i32` or a `u32`. A decimal
literal does take its sign: `let x: i32 = -5` is an `i32`, and `-128` fits an
`i8`. Datafun
parses `-5` as the negation of `5`, except right after a hint, and the type
checker reads a negated literal where a fixed-width integer is expected as the
signed literal, so the two are one value (`Check-NegLit` in
[Datafun Typing Rules](datafun-typing-rules.md)). Datalit, having no
operators, reads `-0x10` as an error.

Underscores may group digits, and say nothing about the value:

```datalove
1_000_000
0xFF_FF
```

A separator goes between digits, so a literal begins and ends with one.
`_1` is a name rather than a number, and `1_` and `0x_FF` are neither (P052).
Separators may be doubled: `1__0` is 10.

**Floating-point** numbers use decimal notation, with an optional exponent:

```datalove
3.14
1.0e300
2.5e-10
1e-7
6.022E23
```

A point or an exponent is what makes a literal a float; `42` is an integer
and `42.0` and `4.2e1` are floats. The exponent's sign may be written or
left out, and its marker may be `e` or `E`. The fraction may be left out
when there is an exponent: `1e-7` needs no `.0`. Separators group the
digits here too, in any of the three runs: `1_0.000_1e1_0`.

This is also how floats are printed, so a finite value that comes out of
the compiler can be typed back into it. Magnitudes from `1e-5` up to but not
including `1e16` print positionally, the rest with an exponent, and every
finite float prints with a point or an exponent so that none of them reads as
an integer: `42.0`, not `42`.

A float literal too large for its type rounds to infinity rather than being
refused: `1.0e400` is an `f64` infinity, and `: f32 / 1.0e300` an `f32` one.
Infinity and NaN print as `inf` and `nan`, which are not literals; they are
had from `sys/std/f32` and `sys/std/f64`.

**Strings** are enclosed in double quotes:

```datalove
"hello, world"
"tab\there, \"quoted\", \u{e9}"
```

The escapes are `\"`, `\\`, `\n`, `\r`, `\t`, `\0` and `\u{...}`, the last
taking one to six hex digits naming a Unicode scalar value. Any other escape,
or a `\u{...}` that names no scalar value (`\u{D800}`), is D039, in both
languages. A string may span lines; the newline is part of it.

**Booleans** are `true` and `false`.

### 2.3 Comments

Line comments begin with `//` and extend to end of line.
Block comments are delimited by `/*` and `*/` and may be nested.

### 2.4 Spacing

Whitespace is significant inside an expression. Two tokens are **glued** when
the first one's span ends where the second one's begins; a space, a newline or
a comment between them leaves a gap. Two rules read that one fact: gluing
decides how far a literal reaches, and fixity decides what an operator attaches
to. Both are shared by Datalit and Datafun, so the two languages give the same
answer for the same spelling, where Datalit has the construct at all: it has
no operators, so a spelling that is an operator in Datafun is an error there.

**Literal extent.** A numeric literal is a maximal glued run: an optional sign,
digits, a `.` and a fraction, an exponent marker with its sign and digits. A
gap anywhere inside ends it.

```datalove
-1.5e-7        // one literal
1 . 5          // error: a float is written without spaces: `1.5`
2.5e - 10      // error: an exponent is written without spaces: `2.5e-10`
```

The two errors are P052 in Datafun.

Letters written onto the digits are read as a suffix and reported as one
(P053 in Datafun, D019 in Datalit), since the language has none: `1u8` says to
write `: u8 / 1` rather than complaining about an unexpected identifier.

**Fixity.** An operator's spacing says what it attaches to:

| Position | Reading |
|----------|---------|
| No left operand | Prefix, whatever the spacing |
| Left operand, glued both sides or spaced both sides | Infix |
| Left operand, spaced left and glued right | Prefix |
| Left operand, glued left and spaced right | Postfix |

The first row needs no spacing at all, which is what keeps `f(-1)`, `lcm(-4, 6)`
and `ret -1` reading as they always did: nothing precedes the `-`, so nothing is
consulted.

```datalove
a - b          // subtraction
a-b            // subtraction
a -b           // error (P021): `-` spaced as a prefix operator after an operand
p . 0          // error: a postfix operator is written against the expression before it
x ?            // error: likewise
f (1)          // error: a call's `(` is written against what is called, as a postfix operator is
a--b           // `a - (-b)`
```

A misplaced operator is P021. Inside a tensor row, where elements are
separated by spacing, `a -b` is the two elements `a` and `-b`.

Fixity is for the sigil operators. A word operator -- `and`, `or`, `xor`, `not`
-- is delimited by being a word, and gluing it to an operand would make it part
of the operand. The postfix operators `?`, `!`, `@`, `.field` and `[index]`
require gluing on their left.

The literal reader runs first and takes what is glued to it, and fixity judges
only the operators it declined. That is why `2.5e-10` is one number while `x-1`
subtracts.

The same rule settles how many elements a row of a tensor literal has, where
elements are separated by nothing but spacing (Section 3.2).

### 2.5 Names

A name is ASCII letters, digits and `_`, and does not begin with a digit:
`[A-Za-z_][A-Za-z0-9_]*`. Strings and comments may hold any text.

A name written with a letter or digit outside ASCII is refused (P075 in
Datafun, D043 in Datalit), once for each place it is written, and the parse
goes on as though it were a name. Letters and digits of any script are read
into one word, so the whole of `café` is reported rather than `caf` followed by
a stray character. A word that begins with an ASCII digit is a number, whose
letters are a suffix and are reported as one (Section 2.4).

```datalove
let café = 1       // error: `café` is not an ASCII name
let x = "café"     // a string may hold anything
```

Names are ASCII because they leave the language: a native rider defines its
functions under symbols made from them (Section 9.5), and the AOT backends
write them into generated code. Text that looks the same can also be different
code points -- `é` precomposed or as `e` and a combining accent, a Latin `a` or
a Cyrillic one -- and two such names would be two names.

### 2.6 Scope

A scope is read top to bottom. **A declaration is in scope from where it is
written to the end of its scope, and a function throughout it**, so functions
may call each other in any order. A function's signature and body see what is
written above the function, and every function of the scope.

```datalove
require module sys/std/u32

fun even(n: u32): bool        // calls `odd`, written below: fine
    if n == 0
        ret true
    end if
    ret odd(u32.sub_wrapping(n, 1))
end fun

fun odd(n: u32): bool
    if n == 0
        ret false
    end if
    ret even(u32.sub_wrapping(n, 1))
end fun

const LIMIT: u32 = : u32 / 10
```

`even` and `odd` may name `u32`, and not `LIMIT`. This is one rule for every
scope: a function body, a script, a module (which has no `let` or `var`), and
a session, whose units continue one scope, so that a unit sees the units
before it and a function is in scope throughout only its own unit. A name used
above its declaration is the error it would be anywhere it is not in scope:
F001 for a value, F002 for an imported function, F064 for a type, F078 for a
module or rider.

## 3. Types

### 3.1 Primitive Types

| Type | Description |
|------|-------------|
| `bool` | Boolean value |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers |
| `i8`, `i16`, `i32`, `i64` | Signed integers |
| `index` | Addressable size of indexed collections |
| `offset` | Signed counterpart of `index`, for differences between indexes |
| `f32`, `f64` | Floating-point numbers |
| `int` | Arbitrary-precision integer |
| `string` | UTF-8 string |

The `index` and `offset` types are 32-bit by default, or 64-bit when the
`index-64` feature is enabled.
`index` is not the same as the platform pointer size,
which is not exposed to the language.
The size of `index` is less than or equal to the platform pointer size.

**A count is always a `u32`.** Every bit count, shift amount and rotate
amount in `sys/std`, and the exponent of every fixed-width integer's `pow_*`,
is a `u32`, whatever the width of the value it counts or shifts:
`u8.count_ones(self: u8): u32`, `u64.bits(): u32`,
`i64.shift_left(self: i64, n: u32): ?i64`, `u8.pow_checked(self: u8, exp: u32)`.
This is Rust's rule, and it is here for the same two reasons. A count is
bounded by the widest integer, so `u32` always holds it and never needs a
register wider than one; and hardware takes a shift amount in a fixed narrow
place and masks it, so a shift amount of the shifted type asks a question the
machine does not answer. It also lets counts compose: a count out of one width
feeds a shift at another, which a same-width rule cannot express.

`int.pow` takes its exponent as an `index`, since an `int` has no width to
bound it, and the float `pow`s take a float.

`abs_diff` is the exception, because a magnitude is not a count: it gives the
unsigned type of the same width, so `i32.abs_diff` gives a `u32` and
`offset.abs_diff` gives an `index`. `int.abs_diff` gives an `int`, and the
generic `fixedint.abs_diff<T>` gives a `?T`, which is `none` where the
difference does not fit `T`.

### 3.2 Collection Types

**List.** An ordered sequence of elements.

```datalove
[T]              // type
[1, 2, 3]        // literal
```

**Map.** A key-value mapping.

```datalove
%{K = V}         // type
%{ 0 = 5 }       // literal
```

**Set.** An unordered collection of unique elements.

```datalove
#{T}             // type
#{ 1, 2, 3 }     // literal
```

**Table.** A columnar data structure with named columns.

```datalove
{| col1: T1, col2: T2 |}    // type
```

Table literals use a line-oriented syntax:

```datalove
{|
  x, y           // column names
  1, 2           // row 1
  3, 4           // row 2
|}
```

Semicolons permit single-line format: `{| x, y; 1, 2; 3, 4 |}`. A `;` is a
line break written on the line, here and between two statements alike, so a
run of them is a blank line and not an error. A `,` between table columns goes
between two things, so one with nothing before it is an error (D033,
`{| x,, y |}`); a trailing one closes what it follows and is fine. A list,
struct or tuple takes a trailing comma as well.

A table is opaque: nothing can look inside one. Column projection
(`table.x`) is not implemented and is refused as a projection on a value with
no fields (F068), and indexing a table is refused too; the options are written
up in [Tables: what the type system is missing](design-table-rows.md).

**Tensor.** A multi-dimensional array with fixed shape.

```datalove
[|T, N|]                 // type: element type T, rank N
[| 1 2 3 |]             // 1D literal (shape inferred: [3])
[| 1 2 3, 4 5 6 |]      // 2D literal (shape inferred: [2, 3])
[| 1 2, 3 4,, 5 6, 7 8 |]  // 3D literal (shape inferred: [2, 2, 2])
```

Type hints specify element type and rank:

```datalove
: [|u32, 2|] / [| 1 2, 3 4 |]
```

Shape is inferred from the multi-comma structure: spaces separate elements
along the innermost axis, `,` separates rows (2nd axis), `,,` separates
slabs (3rd axis), `,,,` separates blocks (4th axis), etc. A comma run goes
only between two parts, so a tensor takes no leading or trailing comma
(D033).

A shape the separators cannot show -- a zero extent above the innermost axis,
or a leading extent of 1 -- is written in a header before a `|`, and the body
after it either flat, in row-major order, or shaped as the header says:
`[| 1 3 | 1 2 3 |]` has shape [1, 3], `[| 0 3 | |]` is an empty 0x3 tensor,
and `[| 2 3 | 1 2 3 4 5 6 |]` is a header over a flat body. A tensor has at
least one axis: `[|T, 0|]` is refused (D042).

Because the innermost axis is separated by nothing but whitespace, an
operator's spacing decides how many elements a row has (Section 2.4):
`[| 1 -2 |]` holds two elements and, in Datafun, `[| 1 - 2 |]` holds one.
Datalit has no operators, so there `[| 1 - 2 |]` is an error (D050).

A tensor is indexed along axis 0 (Section 7.3). `sys/std/tensor` is written
over that, at rank 1. Transpose, slice and reshape are not exposed; the
runtime supports them internally.

A tensor's rank is part of its type rather than a parameter, so a function
takes a tensor of one rank -- `[|T, 1|]` and `[|T, 2|]` are different types and
neither can be written as "a tensor of any rank".

### 3.3 Aggregate Types

**Tuple.** An ordered sequence of heterogeneous values.

```datalove
(bool, u32)              // type
(true, 42)               // literal
()                       // unit type and value
```

**Struct.** A collection of named fields.

```datalove
{ x: f32, y: f32 }       // type
{ x = 1.0, y = 2.0 }     // literal
```

A struct's fields are in the order written, and the order is part of the type:
`{ y = 1, x = 2 }` is not a `{ x: int, y: int }` (F075). A field, table column
or enum variant named twice is D038.

### 3.4 Option and Result

**Option** represents an optional value:

```datalove
?T                       // type: value or none
some primary             // wrap value
none                     // absent value
```

**Result** represents success or failure:

```datalove
!T                       // type: value or error
ok primary               // wrap success value
er primary               // wrap error value
error "message"          // error literal
```

The payload of `er` is checked against `error`, so a result's failure is
written `er error "message"`. An `error` is not a result by itself: where a
`!T` is expected, `error "message"` is a mismatch.

The `some`, `ok`, `er`, `data`, `error` and `term Name` constructors take a
*unary* expression as their payload -- a literal, variable, function call,
parenthesized expression or hinted expression, with any postfix and prefix
operators written on it -- so `some x@` is `some (x@)`, `some o?` is `some (o?)`
and `some -x` is `some (-x)`. This is the same with a hint over the constructor:
`: ?int / some o?` is `some (o?)` too. A binary operator after a payload is
P065: `some a + b` is refused, and is written `some (a + b)`, or `(some a) + b`
if the operator is meant for what is built:

```datalove
some 42                  // ok: a literal
some(a +? b)             // ok: parenthesized
ok result                // ok: a variable
ok(x +! y)               // ok: parenthesized
er(error "msg")          // ok: parenthesized
```

### 3.5 Data and Error Types

The `data` type is a universal container that can hold any value:

```datalove
data 42
data : int / 100
data(a + b)              // parenthesized for binop payload
```

The `error` type is an existential error value:

```datalove
error "message"
error(some_expr)         // parenthesized, as any payload may be
```

Both `data` and `error` take a unary expression as their payload (Section 3.4).
A value is a `data` only when written as one. Nothing becomes a `data` by being
where one is expected: `let d: data = x` is a mismatch, and
`let d: data = data x` is how to write it. Datalit is the same.

### 3.6 Atom, Term, and Enum Types

**Atom.** A named unit type with no payload.

```datalove
atom Red                 // type and value
```

An atom is both a type and a value. Two atoms are the same type if they have
the same name.

**Term.** A named type with a typed payload.

```datalove
term Foo int             // type
term Foo 42              // value
```

Two terms are the same type if they have the same name and payload type.

**Enum.** A closed union of atom and term variants.

```datalove
enum { atom Red, atom Blue, term Custom string }   // type
```

Enum variants are matched by name, and their order is not part of the type:
`enum { atom Red, atom Blue }` and `enum { atom Blue, atom Red }` are one
type. Atoms and terms can stand alone as types, or combine into enums.

**Enum values.** An atom or term literal is a value of any enum that lists
it, so where an enum type is expected it is written bare, or wrapped as an
enum literal:

```datalove
let c: enum { atom Red, atom Blue } = atom Red
let d: enum { atom Red, atom Blue } = enum { atom Blue }
let e: enum { atom Red, term Custom string } = term Custom "hello"
```

An enum literal has no type of its own and needs one expected (F011). This is
the same in Datalit.

**Coercion.** An atom or term that is not a literal -- a variable, a
parameter, a call -- has its own atom or term type, and does not become an
enum by being used as one. The `@` operator widens it into a compatible enum
type (Section 6.8):

```datalove
let a = atom Red
let c: enum { atom Red, atom Blue } = a     // error: expected enum, found `atom Red`
let d: enum { atom Red, atom Blue } = a@    // ok
```

`@` does not widen one enum into another with more variants.

**Match.** Enums are destructured with `match` (see Section 8.8).

### 3.7 Type Aliases

Type aliases provide names for structural types:

```datalove
type Point: { x: f32, y: f32 }
type Age: u32
```

Aliases are purely syntactic; they introduce no new types. Enum types are
commonly given aliases:

```datalove
type Color: enum { atom Red, atom Blue, term Custom string }
```

### 3.8 Recursive Types

`rec Name T` is the type `T`, in which `Name` stands for the whole. It is the
one way to write a type that contains itself, since an alias cannot name
itself. A value of one is written as the body is, with no mark:

```datalove
: rec Node { name: string, kids: [Node] } / {
  name = "root",
  kids = [{ name = "a", kids = [] }],
}
```

The name is bound inside `T` only, so `rec` needs no statement and is the same
in datalit. Each use of the name has a list, map, set or table between it and
its `rec`, which is what keeps a value's size finite: `rec L { next: ?L }` is
refused (T064). A cons list's tail is written `next: [L]`, holding at most
one, by convention. A `rec` that never uses its name (T062), that takes the
name of a `rec` around it (T063), or a recursive type in a map's key or a
set's element (T065) is refused too. A `rec` may sit inside another and use
its name: `rec E enum { term Lit int, term Block [rec S enum { term Let E }] }`.

The name is no part of the type: `rec A [A]` and `rec B [B]` are one type. A
recursive type is not the type its body unfolds to, `{ name: string, kids:
[rec Node ...] }`; a value checked against the `rec` type is checked against
the unfolding, so the two are written alike, but a hint of one is a mismatch
with the other (T040).

Datafun refuses recursive types for now (F090), in a type it is written and
in data it takes in with `require data`. The design, and the work left, is in
[Recursive Structural Types](plan-recursive-types.md).

## 4. Type Hints

Type hints specify expected types for expressions:

```datalove
: type / expression
```

Examples:

```datalove
: u32 / 42
: [i32] / [1, 2, 3]
: f32 / 0xABABABAB     // hex as bit pattern
: atom Red / atom Red
: enum { atom Red, atom Blue } / atom Red
```

The expression need not be a literal, and the hint is what it is checked
against. It is a unary expression, as a constructor's payload is (Section
3.4) -- a literal, a name, a call, a constructor, a parenthesized expression,
with any postfix and prefix operators written on it -- so a binary operator
after it applies to the hinted expression: `: u32 / x + 1` is
`(: u32 / x) + 1`, and `: u32 / (x + 1)` hints the sum.

```datalove
let n: u32 = 1
let a = : u32 / n           // a name
let b = : u32 / twice(n)    // a call
```

A hint is the type of the expression it is on, and where something else
expects a type of that expression, the two have to be the same. A hint is not a
conversion: `let x: u32 = (: u8 / 1)` is a mismatch, the same as it would be
for a `u8` variable, and `(: u8 / 1)@` is how to ask for the widening.
Parentheses under a hint keep the hint inside them: `: u32 / (: u8 / 1)` is two
hints that disagree, and `: u32 / (1)` is the same as `: u32 / 1`. Datalit
checks hints the same way.

Postfix lands on the expression under the hint, as it does on the payload of
`some` and its fellows: `: u32 / o?` hints what the `?` produces, not the option
it unwraps. A literal is the exception, because the hint is what makes it a
value of that type: after a number, a string, a constructor, a collection or a
parenthesized expression the hint stays on that literal and postfix comes after
it, so `: u8 / 5@` is the `u8` 5, adapted, and `: u32 / (o)?` hints `o`. A hint
directly under another, `: u8 / : u8 / 1`, is refused (P080); parenthesize the
inner one. A hint takes nothing of its own, so the expression under it moves
what it would have moved written without one.

A bare name in type position is a type alias (Section 3.7), or, within a generic
function's signature or body, one of its type parameters (Section 8.7). Data
literals have neither, so a name in a data literal's type is always an error.
A name that is neither is F064, wherever the type is written: a `let`, `var` or
`const` annotation, a hint, a parameter, a return type, or inside any of those.

A collection type is written with its sigil rather than its name, so `tuple`,
`list`, `map`, `set`, `table` and `tensor` are not types. They are names like
any other, free for an alias or a type parameter; where none of that name is in
scope the F064 saying so adds how the collection is written. A primitive written
in another case, `Int`, gets the same treatment, with the spelling it meant.

## 5. Copy and Linear Types

Types are classified as *copy* or *linear*.

**Copy types** can be freely duplicated: `bool`, fixed-width integers (`u8`
through `u64`, `i8` through `i64`), `index`, `offset`, `f32`, `f64`, and unit.
Atoms are always copy. A term, option, tuple or struct is copy if everything in
it is, and an enum if all its variant payloads are.

**Linear types** have move semantics: `int`, `string`, `[T]`, `%{K = V}`,
`#{T}`, `{| ... |}`, `[|T, N|]`, `data`, `error`, and every result `!T`,
whatever `T` is. A term, option, tuple, struct or enum holding a linear type is
linear.

A **type parameter** is linear, whatever it is instantiated with, because the
caller may supply a linear type and the function is compiled once for all of
them (Section 8.7).

A linear value is moved at most once; one never moved is dropped at the end
of its scope. Borrowing it -- as a `ref` argument, an operator's operand, under
`debuglog` -- does not count as a use. After a value is moved, subsequent uses
are compile-time errors (D001):

```datalove
let x: int = 42
let y = x              // x is moved
let z = x              // error: use of moved value
```

## 6. Expressions

### 6.1 Operators

Operators listed from highest to lowest precedence:

| Precedence | Operators | Description |
|------------|-----------|-------------|
| 1 | `()` | Grouping |
| 2 | `.field` `.0` `[i]` `(args)` `@` `?` `!` | Postfix (field, index, call, adapt, try) |
| 3 | `-` `-?` `-!` `not` | Unary prefix |
| 4 | `*` `/` `*!` `/!` `*?` `/?` | Multiplicative |
| 5 | `+` `-` `+!` `-!` `+?` `-?` | Additive |
| 6 | `.<` `.>` `<=` `>=` `==` `!=` | Comparison |
| 7 | `and` | Logical AND |
| 8 | `or` `xor` | Logical OR/XOR |

Binary operators are left-associative, so `a - b - c` is `(a - b) - c`, except
that comparisons do not chain (Section 6.3). Postfix operators chain left to
right: `a.b[0]?@` applies each in turn.

Postfix binds tighter than unary prefix, so a prefix operator applies to the
postfix expression as a whole: `-x@` is `-(x@)`, `not s.a` is `not (s.a)` and
`not list.is_empty(ref xs)` negates what the call returns. The payload keywords
`some`, `ok`, `er`, `data`, `error` and `term Name` take their payload the same
way, so `some x@` is `some (x@)`.

Whether a sigil operator is prefix, infix or postfix is decided by its spacing
before precedence is consulted at all: `a - b` and `a-b` are the binary operator
in this table, and `a -b` is not one (Section 2.4).

### 6.2 Arithmetic

**Bare arithmetic** (`+`, `-`, `*`, `/`) behaves differently by type:

- **Floats**: Operations return the same float type. Division is permitted.
- **Bigints**: Addition, subtraction, multiplication, and unary negation
  return `int`. Division is not permitted (use checked variants).
- **Fixed integers**: Bare arithmetic is not permitted. Use `@` to widen
  operands to `int` for bigint semantics, or use checked/optional operators
  (`+!`, `-!`, `*!`, `/!` or `+?`, `-?`, `*?`, `/?`) for overflow handling.

**Checked arithmetic** (`+!`, `-!`, `*!`, `/!`) operates on fixed integers and
yields the operands' type. On overflow or division by zero, the function
early-returns an error, so the enclosing function must return a result:

```datalove
fun add(a: u32, b: u32): !u32
    ret ok (a +! b)
end fun
```

Bigint division `/!` yields `int`.

**Optional arithmetic** (`+?`, `-?`, `*?`, `/?`) operates on fixed integers and
yields the operands' type. On overflow or division by zero, the function
early-returns `none`, so the enclosing function must return an option:

```datalove
fun add(a: u32, b: u32): ?u32
    ret some (a +? b)
end fun
```

Bigint division `/?` yields `int`. Unary `-!` and `-?` negate any fixed
integer. A signed value fails only at its minimum, which has no counterpart;
an unsigned one fails for any value but zero.

The error a checked operator returns is a fixed string:
`error "arithmetic overflow"`, `error "division by zero"`, or for a negation
`error "negation overflow"`.

Integer division truncates toward zero, so `-7 /? 2` is `-3`. There is no
remainder operator; derive one from the quotient, which gives it the sign of
the dividend, or use `int.rem_checked`.

### 6.3 Comparison

```datalove
.<    less than
.>    greater than
<=    less than or equal
>=    greater than or equal
==    equal
!=    not equal
```

All comparison operators return `bool`, and both operands must have the same
type; there is no implicit widening.

Less-than and greater-than are spelled `.<` and `.>` because `<` and `>` alone
are brackets, matched in pairs before an expression is read, as they are around
a function's type parameters. `a < b` is an unclosed `<` (P051).

The ordering operators `.<`, `.>`, `<=` and `>=` take numeric operands only.
Order strings with `string.cmp`, and anything else with `sys/std/ord`.

`==` and `!=` take a number, `bool`, `string`, unit or an atom, and an option,
tuple, struct, term or enum whose parts all do. Such a value is compared part by
part. Floats compare by IEEE 754 wherever they are, so an option `n` holding a
NaN has `n != n`, and `(0.0, 1) == (-0.0, 1)`. Lists, sets, maps, tables,
tensors, results, `data` and `error` have no equality operator yet, and neither
does a type parameter or an aggregate holding one (Section 8.7). Anything can be
compared with `sys/std/ord`.

A construction with no type of its own takes the other operand's: with
`x: ?u32`, `x == none` and `x == some 1` compare `?u32`s.

Comparisons do not chain. `a == b == c` is refused (P067); parenthesize the
comparison being compared, or join two with `and`.

### 6.4 Logical Operators

```datalove
and   logical AND
or    logical OR
xor   logical XOR
not   logical NOT (unary)
```

All require `bool` operands and return `bool`.

### 6.5 Try Operators

The postfix `?` operator unwraps an option, early-returning `none` on failure:

```datalove
fun get_value(opt: ?i32): ?i32
    let x = opt?           // early-return if none
    ret some (x +? 1)
end fun
```

The postfix `!` operator unwraps a result, early-returning the error on
failure:

```datalove
fun parse(s: string): !i32
    let n = do_parse(s)!   // early-return if error
    ret ok n
end fun
```

`?` in a function that does not return an option, and `!` in one that does not
return a result, is F049; `?` on a value that is not an option, or `!` on one
that is not a result, is F048.

### 6.6 Operator Argument Semantics

All operators treat their operands as immutable references. Operands are not
consumed:

```datalove
fun f(x: int): int
    let a = x + 1       // x is borrowed for the operation
    let b = x + 2       // x can be used again
    ret x               // x is still valid
end fun
```

### 6.7 Intrinsic Calls

Intrinsics are low-level operations that compile to machine instructions:

```datalove
icall intrinsic_name(args)
```

They are the layer `sys/std` is written over, and a program rarely calls one
directly. Most come in a version per fixed-width type, named by a suffix
(`_u8` through `_u64`, `_i8` through `_i64`, `_index`, `_offset`, `_f32`,
`_f64`):

- bitwise operations: `bitnot`, `bitand`, `bitor`, `bitxor`
- shifts: `shl`, `shr`, and `sshr` on signed types
- bit counting and byte order: `popcount`, `clz`, `ctz`, `swap_bytes`,
  `reverse_bits`
- wrapping arithmetic: `add_wrapping`, `sub_wrapping`, `mul_wrapping`,
  `neg_wrapping`
- remainders: `rem`, and `srem` on signed types
- float math: `sqrt`, `floor`, `ceil`, `trunc`, `round`, `abs`, `min`, `max`,
  `copysign`, `is_nan`, `is_infinite`
- conversions: reinterpretation between same-width types (`u32_to_i32`),
  float bit casts (`f32_to_bits`, `bits_to_f64`), narrowing and int-to-float
  casts, and conversions to and from `index` and `offset`

The full list, with signatures, is
`crates/datalove-datafun-intrinsics/src/lib.rs`.

Two intrinsics answer a question about the build rather than compute anything:
`is_big_endian(): bool` and `index_bits(): u32`, the latter being the width of
`index` and of `offset`. Both take no arguments, and neither can be written as
a literal because the answer is not fixed by the source. Every intrinsic,
these included, runs under compile-time evaluation, so a `const` bound to one
is folded to a literal before any backend sees it -- which is how `sys/std`'s
`index` and `offset` modules state their own edges without naming a width.

### 6.8 Adapt Operator

The postfix `@` operator performs explicit clone and/or widening conversions.
A widening takes its target from the expected type; with none, `@` clones.

**Clone**: For linear types, `@` creates a deep copy:

```datalove
let msg = "hello"
let a = consume(msg@)   // clone msg, original stays valid
let b = consume(msg)    // msg is still available
```

**Widen**: For fixed integers, `@` widens to a larger type, and `f32` widens
to `f64` (Section 10 lists the conversions):

```datalove
let n: u8 = 42
let x: int = n@         // widen u8 to int
```

Unsigned integers can widen to larger signed types too: a `u8` is an `i16`
by `@`.

**Atom/term to enum**: A value of atom or term type widens to an enum type
that lists it (Section 3.6). Like every `@`, it copies, and the original stays
usable:

```datalove
type Color: enum { atom Red, atom Blue }
let a = atom Red
let c: Color = a@            // atom widens to enum
```

**Widening needs a target; cloning does not.** Where an expected type is
known, `@` converts to it. Where none is, `@` clones, and the clone has the
type it was taken from:

```datalove
let msg = "hello"
let copy = msg@         // copy: string, and msg is still there
```

So `@` may be written anywhere, including where nothing asks for a type: the
input of a `match`, an argument to `debuglog`, a `let` with no annotation.

An expected type reaches `@` from:

- A `let` or `var` binding with a type annotation.
- Return position in a function with a declared return type.
- An argument to a function call.
- A constructor's payload or a collection literal's element, from the
  expected type of what is built: `let o: ?int = some x@`.
- An operand of a binary arithmetic operator whose operands have the same
  type as its result: `+`, `-`, `*` and `/` where that type is one they
  accept, and the checked and optional operators. The expected type of the
  whole expression reaches both operands, so `let c: int = a@ + b@` widens
  both to `int`.
- The operand of unary `-`, when the expected type is a float:
  `let y: f64 = -x@` with `x: f32` widens. An `int` is not passed on, so
  `let y: int = -x@` with `x: u8` is F026.
- An operand of a comparison, when the other operand has a type of its own.
  A comparison returns `bool`, which says nothing about its operands, so the
  type comes from across the operator instead: `a@ .< b` takes `b`'s type. A
  literal has no type of its own to give, so in `a@ .< 0` the literal goes
  along with `a@` and the comparison is at `a`'s type. With `@` on both sides
  the left one clones, and the right one is checked against its type, so with
  `a: u32` and `b: u8`, `a@ .< b@` compares `u32`s and `b@ .< a@` is an
  error.

Widening is only what was asked for, so an atom under no expectation stays an
atom: `let c = (atom Red)@` gives `c` the type `atom Red`, and using it where
the enum is wanted is the error, not the `@`.

The expected type has to be one the operator accepts, so `@` does not make an
operator available on a type that does not have it. `let r: u32 = a@ + b@`
is still an error, because `@` leaves the operands `u32` and bare `+` is not
defined there; widen to `int`, or use `+!` or `+?`.

## 7. Place Expressions and Indexing

### 7.1 Place vs Value Expressions

Expressions are either **place expressions** (denoting a storage location) or
**value expressions** (producing a fresh owned value).

Place expressions:
- Variables: `x`
- Field access: `x.field`
- Tuple element: `x.0`
- Index: `a[i]?`, `a[i]!`, `m[key]?`, `m[key]!`
- Chains: `a[i]?.field`, `a.b[0]?`

Value expressions:
- Literals, function calls, arithmetic results, constructors, `@` clones

Field access and indexing can also follow value expressions (`f(x).field`,
`f(x)[i]?`), but these produce values, not places -- the result cannot be
used as a `set` target or `mut` argument (D012). A linear part of one is
refused as a linear part of a place is (Section 7.2): `mk().0` with an `int`
first element is F070.

### 7.2 Field Access

Struct fields and tuple elements are accessed with dot notation:

```datalove
let p = { x = 1.0, y = 2.0 }
let a = p.x                    // struct field

let t = (true, 42)
let b = t.0                    // tuple element by index
```

Chained access navigates nested structures:

```datalove
let inner = outer.a.b.c
```

A projection of a field whose type is linear may not be read as a value: it
would move the field out of an aggregate that still holds it (F070). It may be
borrowed -- as a `ref`, `mut` or `out` argument, an operator's operand, a
`set` target, under `debuglog` -- or cloned out with `@`:

```datalove
let t = (1, 2)
let x = t.0                   // error (F070): int is linear
let y = t.0@                  // ok: explicit clone
call foo(ref t.0)             // ok: borrow
debuglog t.0                  // ok: borrow
```

A field of a copy type (Section 5) is copied out freely.

A borrow of a value being built does not reach its parts. The elements of a
tuple, struct, collection or constructor are moved into it, so
`debuglog (p.name,)` needs `p.name@` though `debuglog p.name` does not.

### 7.3 Fallible Indexing

The `[]` operator on lists, maps, and tensors is fallible: the index may be
out of bounds or the key may be absent. The `?` or `!` postfix resolves the
failure strategy:

```datalove
a[i]?      // early-return none on out-of-bounds
a[i]!      // early-return error on out-of-bounds
m[key]?    // early-return none on missing key
m[key]!    // early-return error on missing key
t[i]?      // early-return none on out-of-bounds (tensor axis-0)
```

Bare `a[i]` without `?` or `!` is a type error in read context. There is no
infallible/panicking index variant.

The error that `!` returns is a fixed string: `error "index out of bounds"`
for lists and tensors, `error "key not found"` for maps, in a `set` target
as anywhere else.

The enclosing function's return type determines which variant is valid: `?`
requires the function to return `?R`, `!` requires `!R`.

```datalove
fun get_elem(a: [u32], i: index): ?u32
    ret some (a[i]?)
end fun

fun get_elem_r(a: [u32], i: index): !u32
    ret ok (a[i]!)
end fun
```

Lists and tensors are indexed by `index`. Maps are indexed by their key type.
For tensors, each `[i]?` indexes along axis 0, reducing rank by 1.
A rank-1 tensor indexed produces the element type; a rank-N (N>1) tensor
indexed produces a rank-(N-1) sub-tensor **view**. A view is a non-owning
tensor struct that aliases the parent tensor's data buffer. Indexing into
a non-copy element in consume context requires `@` (clone): `t[i]?@`; without
it, F072.

**View type restrictions:** Views can be passed to `ref` params but not to `mut`
or `out` params (F073). Whole-value replacement of a view (`set row = ...`)
would overwrite the view without affecting the parent's data. The `set t[i]? = new_row` form is likewise rejected for rank > 1 tensors
(F073). Element-level mutation through views (e.g. `set row[j]? = val`) is
semantically correct but currently also rejected because the typechecker cannot
distinguish it from whole-view replacement at the call site. An element of a
rank-2 tensor is written through the tensor itself, with a chained index:
`set t[i]?[j]? = v`. The view concept is general; tensor sub-views are the only
instance today.

### 7.4 Index and Field Chains

Index and field steps can be chained:

```datalove
a[i]?.field           // index into list, then access field
m[key]?.0             // index into map, then access tuple element
a[i]?.b[j]?           // index, field, index again
```

Each `?` or `!` in the chain is an independent early-return point, checked
left to right. If any check fails, the function early-returns immediately
with no mutation.

### 7.5 Destination Contexts

A place expression's behavior depends on its destination context:

| Context | Behavior | Example |
|---------|----------|---------|
| `let` / `ret` / `in` param | Copy or clone value out | `let x = a[i]?` |
| `ref` param | Immutable borrow | `foo(ref a[i]?)` |
| `mut` param | Mutable borrow (no views) | `foo(mut a[i]?)` |
| `set` LHS | Mutation target (no views) | `set a[i]? = 5` |
| operator operand, `debuglog` | Immutable borrow | `a[i]? + 1` |

In consume context (`let`, `ret`, `in` param), copy-type elements are copied
out freely. Linear-type elements require explicit `@` clone -- moving an
element out of its container would leave a hole. A `set` target has one
indexing form a read does not: a bare index on a map, which inserts
(Section 8.3).

### 7.6 Parallel with Checked Arithmetic

Fallible indexing mirrors checked arithmetic:

| Operation | `?` variant | `!` variant |
|-----------|-------------|-------------|
| Arithmetic | `a +? b` early-returns `none` | `a +! b` early-returns `error` |
| Indexing | `a[i]?` early-returns `none` | `a[i]!` early-returns `error` |

Both yield the plain value when they succeed.

## 8. Statements

### 8.1 Bindings

**Let** binds an immutable value:

```datalove
let x: u32 = 42
let y = compute()      // type inferred
```

**Var** binds a mutable slot:

```datalove
var x: u32 = 0
var y: i32             // uninitialized; must set before use
```

**Destructuring.** `let` and `var` take apart a tuple, a struct, or a term,
spelled as the value is written. The value is consumed, and each name takes
ownership of its part; with `var`, each is a mutable binding of its own:

```datalove
let (a, b) = (true, "s")
let (c,) = ("c",)                  // a one-tuple needs its comma
let {x, y = my_y} = {x = 1, y = 2} // `x` is short for `x = x`
let term Foo t = term Foo "bar"
let atom Foo = atom Foo            // binds nothing
var (m, n) = (1, 2)
```

A struct pattern names every field (F071), and `let (a)` is an error (P070): a
pattern has nothing to group, so it is spelled `(a,)` or `a`. Patterns are one
level deep (P069 for `let (a, (b, c))`), and an enum is taken apart with
`match` rather than `let` (F071). A `var` without a value binds a plain name
and needs a type (P026); a destructuring `var` needs a value (P068).

### 8.2 Const

**Const** binds a value the compiler evaluates. It is valid at the top level
of a script or module and in a function body. Like any declaration it is in
scope from where it is written (Section 2.6), so a top-level const is in scope
for the consts and functions written below it:

```datalove
const LIMIT: u32 = : u32 / 10
const DOUBLED: u32 = LIMIT +! LIMIT     // may name a const written above it

fun clamped(x: u32): u32
    const L: u32 = LIMIT                // and so may a function-level const
    ret L
end fun
```

A const expression may only name other consts, since it is evaluated before
anything a parameter or a `let` is bound to exists. Naming a parameter or a
`let` is an error (F058) even where a const of the same name is in scope.
It may call functions, whose parameters are bound by the call. A module-level
const has no enclosing function, so it cannot use the early-return operators.
A const in a generic function is evaluated once, not once per binding of the
function's type parameters, so its type may not mention one (F088).

**Evaluation** runs the expression at compile time, by the same means that runs
it at run time, so anything a function can compute a const can hold: not only
scalars but `string`, `int`, and collections.

```datalove
const MSG: string = "hello"
const BIG: int = 99999999999999999999
const LST: [int] = [1, 2, 3]
```

A const is an immutable ref binding: it is borrowed wherever it is named, as a
`ref` parameter is. It may be read, compared, indexed and passed as `ref` any
number of times, and moving out of a const of a linear type takes a clone with
`@`. A move without one is D003, whose help says where the `@` goes. A const of
a copy type is copied as any copy type is.

```datalove
const LST: [int] = [1, 2, 3]
let n = list.len(ref LST)      // a borrow
let a = LST@                   // a move out of a const takes a clone
let b = LST                    // D003: cannot move out of const
```

This holds for a const initializer too, so `const B: int = add_ten(A@)` clones
the bigint `A` it passes by value. `match`, the destructuring `if` and `let`
destructuring move what they take apart, so a linear const is taken apart as
`match C@`.

**Ordering.** A const may depend only on what is declared above it, including
through the functions it calls. A function is in scope throughout (Section 2.6),
so a const may call one declared below it, but whatever that function names,
directly or through the functions it calls, must be declared above the const.
A const that depends on itself, or on a const declared after it, this way is
an error (F089), reported at the const with the chain of calls:

```datalove
const A: int = 2
const B: int = twice()      // fine: `twice` names `A`, which is above `B`
fun twice(): int
    ret A + A
end fun

const C: int = f()          // F089: `C` calls `f`, which names `D`
const D: int = 1
fun f(): int
    ret D
end fun
```

Consts are evaluated in source order, and each function is compiled once the
last const it names has its value.

### 8.3 Set

**Set** mutates a `var` binding, a `mut` or `out` parameter, or an
indexed or chained target rooted at one:

```datalove
set x = x + 1
set a[i]? = 5
set a[i]?.field = 10
set m[key]? = v            // update: fail if key absent
set m[key] = v             // upsert: insert or overwrite
```

The `set` target is a place expression (see Section 7). The root must be
`var`, `mut` or `out` (F055 otherwise); an `out` parameter is written whole
(Section 8.5). Mutability propagates backward through the
chain: `set a[i]?.x = 5` needs mutable access to `a[i]?` and so to `a`, so a
`let` or `ref` root is rejected however deep the target. For indexed targets,
the `?` or `!` on each index step provides early-return on failure; the write
only happens if all checks pass.

**Map upsert.** Bare `set m[key] = v` (without `?` or `!`) is valid only for
maps, and only as the last step of the target. It inserts if the key is
absent, overwrites if present. A bare index on a list, or partway along a
target, is F074 -- what it would write through must exist.

An upsert evaluates the RHS, then the key, then looks the key up. If the key
is present, the old value is dropped and the new one stored; the key already
in the map stays and the provided key is dropped. If the key is absent, the
provided key and value are inserted as a pair:

```datalove
var m: %{string = string} = %{ "a" = "one" }
set m["a"] = "uno"    // drops the provided "a" and "one", stores "uno"
set m["b"] = "two"    // inserts "b" = "two"
```

**Evaluation order** for every `set`, plain or compound:

1. Evaluate the RHS. An early return from it (`?`, `!`, a checked operator)
   happens here, before anything else of the statement has run.
2. Evaluate the keys of the target's index steps, left to right.
3. Navigate the target: each index step's bounds or existence check, in
   order, early-returning on the first that fails.
4. Drop the old value at the target (if linear type) and store the new one,
   or for a compound assignment, apply its operator.

All of the program's own code in a `set` -- the RHS and the keys -- runs
before step 3, which runs none, so nothing can move or reallocate a
collection while a reference into it is held. The RHS may borrow the target's
collection, mutably too: `set xs[0]? = grow(mut xs)` stores into the grown
list, and an index the RHS pushes into range is in range when it's checked.

Some consequences of the order:

- An RHS's effects happen even when a lookup then fails.
- When both the RHS and a lookup could fail, the RHS's failure is the one
  returned.
- A key evaluated in step 2 runs even when an earlier step's lookup fails.
- What the RHS moves is gone by step 2: `set m[k]? = k` moves `k` before the
  lookup uses it, and is an error (`set m[k]? = k@` clones it). So is an RHS
  that moves the target's root, `set xs[0]? = consume(xs)`. A plain variable
  may still be replaced by a value made from it, `set x = f(x)`.
- On a failed lookup, the evaluated RHS, and a key a bare index would have
  inserted, are dropped.

**Compound assignment** updates a place with an arithmetic operator:

```datalove
set total += amount
set count +!= 1
set xs[i]?.n *?= 2
set m[key]! -= 1
```

`set p op= v` updates the value in `p` with `op` and `v`. It takes exactly
the operators and types the binary form `p op v` does, with the place's type
on both sides and as the result:

| Compound | Types |
|---|---|
| `+=` `-=` `*=` | `f32`, `f64`, `int`, `T is float` |
| `/=` | `f32`, `f64`, `T is float` |
| `+!=` `-!=` `*!=` `/!=` | fixed-width integers, `T is fixedint`; `/!=` also `int` |
| `+?=` `-?=` `*?=` `/?=` | fixed-width integers, `T is fixedint`; `/?=` also `int` |

As with the binary operators, `!` forms need the function to return a result
and `?` forms an option (F049), and on failure return early, leaving the place
as it was. The same holds for the `?` and `!` of the target's index steps, so
one statement uses one family. Another operator or type is F026.

The place is evaluated once, in the order of any `set`: the value, then its
keys, then its lookups, each a single time, and then the operator reads the
place and the value and stores its result there.

The place has to hold a value when it's reached, since it is read: a moved-out
variable, an unwritten `out` parameter, or one the value moves (`set x += f(x)`
with `f` taking `x` by value) is an error. The value is an operand, so it is
borrowed rather than consumed, and it may be the place itself (`set x += x`). A
bare map index is rejected (F074): it inserts a missing key, and then there is
no value to update.

### 8.4 Functions

Function definition:

```datalove
fun name(param1: T1, param2: T2): ReturnType
    // body
    ret value
end fun
```

A function is defined once in a module or script: a second definition of the
name is F086, and the first is the one that is used. A function is defined at
the top level only; one inside a block is P074.

Void functions omit the return type and may omit `ret`:

```datalove
fun log(msg: string)
    // body
end fun
```

**Inline functions (temporary).** `inline`, alone as the first statement of a
body, replaces every call to the function by its body:

```datalove
fun add(a: u32, b: u32): u32
    inline
    ret icall add_wrapping_u32(a, b)
end fun
```

The calls are gone from the compiled program, in every engine; what the
function does, its parameters' modes and its ownership are as they were. An
inline function may call other inline functions, but not in a cycle, which is
an error (F085), at the first function of the cycle. `inline` anywhere else is
an error (P076). Calls in a script's top-level code are not inlined, only those
in function bodies. This is a
stand-in for an inliner that decides for itself, and will go.

### 8.5 Parameter Modes

All parameters are passed by reference. The mode determines permitted
operations:

| Mode | Syntax | Semantics |
|------|--------|-----------|
| `in` | `x: T` | Caller transfers ownership; callee consumes |
| `ref` | `ref x: T` | Caller retains ownership; callee reads only |
| `mut` | `mut x: T` | Caller retains ownership; callee may mutate |
| `out` | `out x: T` | Callee initializes; caller receives value |

**Restrictions:**
- `ref`, `mut`, and `out` parameters cannot be moved. `sys/std/mem` has
  `replace` and `swap`, which move a value out of a `mut` place and another
  in at the same step.
- `out` parameters must be initialized before the function returns.
- `ref` parameters cannot be passed to `mut` parameters.
- `out` parameters must be written as a whole, not field-by-field.
- Tensor views cannot be passed to `mut` or `out` parameters (Section 7.3).

**Call-site markers.** Every argument repeats its parameter's mode, so that
borrowing and mutation are visible where the call is written rather than only
in the callee's signature. `in` is written by omitting the marker:

```datalove
fun mixed(a: u32, ref b: u32, mut c: u32): u32
    ...
end fun

let r = mixed(x, ref y, mut z)
```

A marker that disagrees with the declared mode is an error (F057), including
a missing marker for a `ref`, `mut` or `out` parameter and a marker on an
`in` parameter. Intrinsics take every argument by value, so `icall` arguments
never carry a marker.

### 8.6 Const Parameters

The `const` modifier declares a parameter whose value must be known at compile
time:

```datalove
require module sys/std/string

fun repeat(const n: int, ref s: string): string
    var result = ""
    var i = 0
    loop while i .< n
        call string.push_str(mut result, ref s)
        set i = i + 1
    end loop
    ret result
end fun

const COUNT: int = 3
let s = "ab"
let x = repeat(COUNT, ref s)  // COUNT is a const binding
```

**Semantics:**
- The argument must be the name of a `const` binding
- The compiler specializes the function for each unique const argument value
- Within the function body, the const parameter is a compile-time constant. A
  `const` binding that names it is evaluated for each instantiation, through
  arithmetic and through calls alike, so `const M: int = n + 1` and
  `const P: int = helper(n@)` are constants in the specialized copy and may
  themselves be passed as const arguments
- Like any const, a const parameter is borrowed wherever it is named, so moving
  out of one of a linear type takes `@`. Passing it on as a const argument is
  not a move: a const argument is borrowed
- A branch on such a binding becomes a jump and the unreachable side is
  dropped. An expression over the parameter written in place, such as
  `if n .< 3`, is not folded and keeps its branch; nothing unrolls a loop

**Restrictions:**
- A const parameter may have any concrete type a const binding can hold,
  collections and linear aggregates included. Instantiations are told apart
  by structural equality of the values, floats by their bits, so `0.0` and
  `-0.0` get separate copies and every NaN with the same bits shares one
- Cannot combine `const` with a passing mode (P082). A const parameter is passed
  by reference, as any const is borrowed: a specialized copy has the value
  written in and is passed nothing, and a call that is not specialized borrows
  the argument. `ref` would say what it already is, and `out` and `mut` would
  write to a constant
- A const parameter's type cannot be a type parameter (`const n: T` in a
  generic function is F077). A const parameter of a concrete type in a
  generic function is fine: specialization removes const parameters and
  leaves the type parameters, so one copy per const instantiation serves
  every type instantiation
- Nothing else is accepted, not a literal and not an expression (F076):
  `repeat(3, ref s)` is refused as surely as `repeat(2 + 1, ref s)`. The value a
  const parameter takes has to be one the compiler already holds, and a `const`
  binding is the one form that says so on its face. A const parameter counts,
  being a const binding within the body

**Implementation:** The compiler monomorphizes, keeping the original function
and adding a copy per instantiation beside it. See [Const Parameter
Specialization](compiler-guide.md#user-content-const-parameter-specialization)
in the compiler guide.

### 8.7 Generic Functions

A function may take type parameters, written after its name:

```datalove
fun pick_first<T>(a: T, b: T): T
    ret a
end fun

fun unwrap_or<T>(x: ?T, default: T): T
    if x |value|
        ret value
    else
        ret default
    end if
end fun
```

A type parameter is a type like any other within the signature and the body. It
is equal only to itself, so nothing in the body can inspect a value of that type
or convert it to anything else.

**Binding.** The call site does not name the types; they are read off the
arguments, and off what the result is expected to be. Each type parameter is a
variable unified with the argument types in its positions and with the
expected result, the way a list's elements are typed together
([Datalove Literals](../mandocs/datalit.md#user-content-typing-together)):

```datalove
let a = unwrap_or(some (: u32 / 7), : u32 / 0)   // T is u32
let b = unwrap_or(some "hello", "fallback")      // T is string
```

Two arguments that decide a parameter differently are a type mismatch rather
than a reinterpretation:

```datalove
let c = pick_first(: u32 / 1, "not a u32")       // error: expected u32, found string
```

An argument that says nothing about a parameter, such as `none`, or a literal,
which only has a default, goes along with what the others say. So order does
not matter:

```datalove
let d = unwrap_or(none, "fallback")              // T is string, from the second
let e = pick_first(99, : u32 / 1)                // T is u32; 99 is a u32
let xs: [u32] = list.of(5)                       // T is u32, from the expected result
```

The types cannot be written at the call: `f<u32>(1)` is not a call. A
parameter nothing decides is F011. Where an argument such as `[]` leaves it
open, the argument is what is reported as needing a type.

**Where a type parameter may appear.** Anywhere: on its own, under `?` or `!`
to any depth, and inside a list, set, map, tensor, table, tuple, struct, term
or enum payload, in any parameter mode and in the return.

```datalove
fun flatten_or<T>(x: ??T, default: T): T
fun sorted<T>(x: [T]): [T]
fun swap<T>(x: (T, T)): (T, T)
fun tagged<T>(x: enum { term Some T, atom None }): bool
```

**Representation.** A generic function is compiled once, not once per type.
Inside it, a value whose type mentions a type parameter is a pointer at the
value in its real layout together with a descriptor saying what it is, and the
caller hands over one descriptor per type parameter, from which the function
builds any other it needs. Nothing is converted at a call: a `u32` passed for a
`T` is the caller's `u32`, and a `[u32]` passed for a `[T]` is the caller's
list. None of this is visible in a program except in what it costs.

**Copy and linearity.** A type parameter is never a copy type, because the
caller may supply a linear one. A value taken out of an option by
`if x |value|` has therefore moved out of `x`, so a function that wants to
return the option it destructured must rebuild it:

```datalove
fun or_option<T>(self: ?T, other: ?T): ?T
    if self |value|
        ret some value        // not `ret self`, which has been moved out of
    else
        ret other
    end if
end fun
```

**Bounds.** A signature may end with a `where` clause, which bounds a type
parameter:

```datalove
fun scaled<T>(a: T, b: T): T where { T is float, }
    ret a * b
end fun
```

A bound says the parameter is one of a fixed set of types, and in exchange the
body may do what all of them have in common. The bounds sit apart from the
names so that the signature reads as a signature. The clause may be spread over
several lines, but the `where` has to be on the signature's own line, because
that is where the signature ends.

There are three bounds:

- **`ord`** is every type there is, and says that the total order every value
  has may be used. It is the weakest bound: what it rules out is a type with no
  ordering, of which the language has none today. It is worth asking for
  because the order is otherwise out of reach -- `.<` is for numbers only, and
  `==` is refused for a list, a result, a type parameter inside an aggregate
  and more (Section 6.3), whether or not a generic is involved. `sys/std/ord`
  is what it offers: the
  comparisons, `min`, `max`, `clamp`, and sorting and searching over a list.

  The relation is the one sets and maps keep their keys in, which walks a value
  structurally. For floats that is IEEE 754-2008 `totalOrder`, so it puts NaN
  in order and tells the two zeros apart; `.<` and `==` on a float do
  neither, even inside an option or a tuple. They differ over nothing else.

  A set or a map written over a type parameter has to ask for it. A set keeps
  its elements in order and a map keeps its keys in order, so `#{T}` and
  `%{T = V}` need `T is ord` whether the collection is a parameter, a return
  type or a binding inside the body. A map's values do not, since nothing puts
  them in order, and neither does `[T]`. Every bound implies `ord`, so `float`
  and `fixedint` serve as well.

- **`float`** is `f32` or `f64`. Both have the bare arithmetic and the
  comparisons, so a parameter bounded to it has them too, along with what
  `sys/std/float` offers.
- **`fixedint`** is any of the ten fixed-width integers: `u8`, `i8`, `u16`,
  `i16`, `u32`, `i32`, `u64`, `i64`, `index` and `offset`. What all ten have is
  the comparisons and the checked and optional arithmetic -- `+!`, `+?` and the
  rest. Bare `+` is not among them, because a fixed-width integer does not have
  one. Negation is among them in both forms, `-!` and `-?`. `sys/std/fixedint`
  is what is written on top of that: the
  arithmetic, and the constants. Comparing and sorting are not there, because
  they ask nothing of a fixed-width integer that another type cannot answer.

A generic still compiles to one body, so the machine code cannot hold the
instruction for every type the bound admits. The operands arrive with their
descriptors and the runtime reads which type it is, the same way a collection
of a type parameter has its elements walked by a size read off a descriptor. A
call site that binds the parameter to anything else is refused (F016).

A value of a bounded parameter is still linear, so handing one to a function
twice asks for a clone; reading one as the operand of an operator does not move
it.

A literal cannot be written at type `T`, because a literal has to be written at
some type and inside a generic the type is what nobody has picked yet. The
constants come from the standard library instead -- `zero`, `one`, `min_value`
and `max_value` in `sys/std/fixedint`, and those plus `nan`, `infinity`,
`epsilon`, `pi` and the rest in `sys/std/float` -- which take theirs from a
descriptor the call site hands over. That works because the type parameter
appears only in the return, so the call site is the one place that knows; see
[the native ABI](native-abi.md). The consequence is that such a call has to be
somewhere its type is decided, by an expected type or by the operand beside it
(Section 12):

```datalove
let z: T = zero()      // the binding says `T`
ret self == zero()     // the other operand says `T`
debuglog zero()        // F011: nothing says what `T` is
```

**What an *unbounded* type parameter does not admit.** Anything that would need
to know what the type is. A `T` can be moved, dropped, cloned, printed, stored,
returned and handed on, and that is the whole of it: `x + y` and `x == y` on two
values of type `T` are both errors, whatever the call site supplied. Printing is
the exception because the descriptor travelling with the value is enough to
format it. A bound is what changes the rest; see **Bounds** above.

Reading a part does work, and in every mode. Indexing a collection whose
elements are a type parameter reads the stride from the descriptor that travels
with the collection rather than from the static type, whether the collection is
owned, borrowed, or reached through a field; so does looking a key up in a map
and indexing a tensor. Projecting a field of a borrowed value works the same
way, the descriptor narrowing alongside the pointer at each step. A part
borrowed out of one carries what it really is, so it can be handed to something
that takes it by reference.

A collection over a type parameter can be built as well as taken, stored,
handed on and returned. `var out: [T] = []` works, and so does a collection
whose element is a *composite* of type parameters -- `[(A, B)]`, `[?T]` --
which is what `list.zip` and `map.entries` are. Its descriptor is built when
the function is entered, from the descriptors of its type parameters. A generic
may call another, or itself, at a larger type -- `f<T>` calling `f<[T]>` --
which builds one more descriptor at each level.

How this is implemented is in [Generics: how it works](generics.md); what is
refused and why is
[What is refused](generics.md#user-content-what-is-refused).

### 8.8 Control Flow

**If statement:**

```datalove
if condition
    // then branch
else
    // else branch
end if
```

The condition is a `bool`. An `if` may be followed by any number of
`else if` arms before its `else`:

```datalove
if x == 1
    debuglog "one"
else if x == 2
    debuglog "two"
else
    debuglog "other"
end if
```

**If with binding** unwraps an option or result:

```datalove
if opt |value|
    // value is bound here
end if

if res |value|
    // value is bound here
else |e|
    // the error is bound here
end if
```

A result's `if` must have an `else`, and the `else` must bind the error,
`else |e|` (F046). An option's `else` binds nothing; `else |e|` on one is a
mismatch (F016). A binding `if` takes no part in an `else if` chain, on either
side (P073): it cannot be followed by `else if`, and an `else if` cannot bind.
Nest the `if` in an `else` body instead. An `else |e|` cannot be followed by
`if` either (P066).

**Loop:**

```datalove
loop
    if done
        break
    end if
end loop
```

`break` exits the innermost loop, and `continue` jumps to its next iteration.
Outside a loop, `break` is F050 and `continue` F051.

**Conditional loop:**

```datalove
loop while condition
    // body
end loop
```

**For** walks a list, a set, a map or a tensor:

```datalove
for item in ref xs          // a list's elements, a set's in order, a tensor's rows
    // body
end for

for i, item in ref xs       // a list or tensor: i is the index, from 0
end for

for key, value in ref m     // a map, by key
end for
```

The collection is written with `ref` (P079) and is a place: a name, a
parameter, or a field or element of one (F087). Each element binding is a
reference into the collection, as a `ref` parameter is: it cannot be moved,
assigned, or passed `mut` or `out`, and `@` clones it. A tensor of rank above 1
binds a view of each row, one rank less. An index binding is an `index`. A
`for` binds one or two names (P078), two on a map and one on a set (F087).
`break` and `continue` work as in `loop`; `continue` goes to the next element.

**Match** destructures an enum value:

```datalove
match c
case atom Red
    debuglog "red"
case atom Blue
    debuglog "blue"
end match
```

Term cases bind the payload to a variable:

```datalove
match shape
case atom Circle
    debuglog "circle"
case term Rect dims
    debuglog dims
end match
```

A `case default` arm matches any unmatched variant. Without a default, the
match must be exhaustive (all enum variants must be covered). A missing
variant and a duplicate case are errors (F016), and so is a second
`case default` (P049).

The input expression is consumed (moved) by the match.

A `case term` arm's binding belongs to that arm. It may take a name already in
scope, and the outer one means what it meant again after `end match`.

### 8.9 Return

`ret` returns a value from a function:

```datalove
ret 42
```

Void functions may use bare `ret` for early exit:

```datalove
ret
```

### 8.10 Debug Log

`debuglog` prints a value:

```datalove
debuglog "red"
debuglog p.a
```

It accepts an expression of any type, and borrows rather than consumes it, so
a linear value may be logged and then used. That is why a projection of a
linear field may be written under it without a `@` (Section 7.2).

### 8.11 Call Statements

A function called for its effect is written after `call`:

```datalove
call bump(mut v)
```

`call` takes a function call, plain or qualified, and nothing written on it
but the try operators (Section 6.5), so that a function returning `!()` or
`?()` is called for its effect:

```datalove
call map.add(mut counts, ref word, 1)!
```

`call f(x)@`, `call icall g()` and `call x!` are P031 as surely as `call x`.
The call, once its try operators have unwrapped it, must be unit (F016
otherwise), since a value computed and discarded is more likely a mistake
than an intention; a value that is wanted for nothing is bound with `let` and
left. A line that begins with no statement keyword at all is P001.

## 9. Module System

### 9.1 Hierarchy

The module system has three levels: library, package, module.

- A **library** is a directory of packages (e.g. `sys/`, `local/`).
- A **package** is a directory of modules and data (e.g. `sys/std/`).
- A **module** is a single `.dfm` file (e.g. `sys/std/u32.dfm`).
- A **data file** is a single `.dlt` file of Datalit
  (e.g. `local/shop/orders.dlt`), which `require data` reads (Section 9.6).

A module defines things for others to use, so its top level holds only
declarations: `fun`, `const`, `type`, `require` and `import`. Any other
statement there, which would never run, is P077. A `native fun` parses there
but is F062, since only a rider interface (Section 9.5) can supply its body.

The `sys` library is compiled into the `datalove` binary. The `local` library
is a workspace's own, read from disk by the command line (`script`,
`script-ir`, `aot-compile`, `repl`) from a directory named `local`:

- beside the script, when there is one;
- else, for a script in a directory named `scripts`, beside that directory;
- else, with no script (the interactive REPL), in the current directory.

A command given a script never looks in the current directory, and nothing
further up the tree is looked at. There is no workspace manifest. The pipeline
itself takes whatever libraries it is given; the rule is the command line's.

A package in `local` may have a rider (Section 9.5), declared in a
`manifest.toml` beside its modules; a package with a rider and no manifest
fails to load. The system library's riders are linked
into the binary; a workspace's own are built with cargo into a library the
first time one of their natives is called, or into the program `aot-compile`
links, so running one needs a Rust toolchain. A rider is named after its
package, in `require rider` and in its natives' symbols, and riders are not
told apart by library: a package with a rider may not share its name with
another package that has one, such as `local/std` beside `sys/std`.

### 9.2 Require

`require` loads a module, a rider or a data file:

```datalove
require module sys/std/u32
require rider std
require data local/shop/orders: [{sku: string, qty: u32}]
```

A `require` or `import` is read only at the top level of a module or script, and
one inside a block is P074, as is a `fun`, `native fun` or `type`. Where it
stands at the top level does not matter: the requires and imports are gathered
before anything is resolved, so a call above the `require` it goes through is
fine.

A `require` is refused where it is written when the module or rider does not
exist (F079), when the same one is required twice in one module or script
unit (F080), when its alias is another required module's already (F081), when
a script requires a rider (F082), and when it closes a cycle (F083): the
modules of a program may not require each other in a cycle. Across the units
of a session a repeated require is not checked, and is redundant as a repeated
import is.

### 9.3 Import

`import` brings a name into scope from a required module or rider:

```datalove
import u32.negate
import std.string_len
```

A name binds one function, so importing a second under a name already bound
is an error rather than a shadowing (F059). The same holds between an import
and a function the module or script defines: the import is refused (F086),
and the function stays. Two modules exporting the same name is
ordinary -- `min` is in every numeric module -- so only one of them can be
imported into a given scope. In a session the imports arrive on separate
lines and the rule is the same across them; importing the same function
again is redundant rather than ambiguous, and allowed.

### 9.4 Qualified Calls

A function may be called through the alias of the module or rider that has
it, without importing it:

```datalove
require module sys/std/u8
require module sys/std/i8
require module sys/std/list

let a: ?u8 = u8.from_int(ref 200)
let b: ?i8 = i8.from_int(ref 200)
call list.push(mut xs, 30)
```

This is how two functions of one name are used in one scope, which importing
both cannot do (Section 9.3). An alias required in an earlier unit of a
session serves a qualified call in a later one, as it serves an import.

`a.f(...)` is read as a field projection `a.f` until the `(`. A `(` written
against a projection of one named field from a bare name makes the whole a
call qualified by that name; against anything else -- `p.0(...)`,
`a.b.c(...)`, `xs[i]?(...)`, `f(x)(...)` -- it is P072, since no value is a
function. The name before the dot is always an alias, even where a value of
the same name is in scope: a value has no functions to call.

A qualified call names nothing when nothing is required under the alias (F078)
or when the module or rider has no function of that name (F002), the same two
errors an import gets.

### 9.5 Native Riders

A **rider** is a Rust crate that provides native functions to a package's
modules. Each package has at most one rider. The system library uses the
same mechanism as user packages.

#### Rider Interface

A rider interface file (`rider.dli`) declares native function signatures
using restricted syntax (`native fun` declarations and `type` aliases only):

```datalove
native fun string_len(ref self: string): index
native fun string_contains(ref self: string, ref pattern: string): bool
native fun list_push<T>(mut self: [T], elem: T)
```

Anything else in an interface is P084. Every package's interface is checked
whenever a program is compiled, whether or not anything requires its rider,
and what is wrong with one stops the compile as a module's errors do, reported
against the `rider.dli` it came from: a parse error, P084, a type name that
does not resolve (F064), a native declared twice (F086). A parameter's name is
a parameter's name here too, so `out` and the other mode words are refused
(P064) though nothing calls a native by its parameters' names.

A native function may take type parameters. As at a call to an ordinary
generic function (Section 8.7), nothing is converted: every parameter arrives
as a pointer and a descriptor, and the implementation reads the element type
off the descriptor at runtime, so one implementation serves every element
type.

#### Rider Crate

The rider crate (`rider/src/lib.rs`) implements native functions as
`extern "C-unwind"` functions following the rider C ABI. The runtime handle
comes first, then each parameter as a `(ptr, tydesc)` pair, then an out-pair
for the return value, then a descriptor for each type parameter that only the
return mentions; the function returns a status byte:

```rust
#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_len(
    rt: *mut u8,
    self_ptr: *const u8, self_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 { ... }
```

Symbol naming convention: `dlr_{rider_name}__{function_name}`. The ABI in full
is [The native rider ABI](native-abi.md).

#### Using Riders

Modules use `require rider` to access native functions, by import or by
qualified call:

```datalove
require rider std
import std.string_len

fun my_len(ref s: string): index
    ret string_len(ref s)
end fun
```

Call sites typecheck normally against the declared signatures. The compiler
emits standard `Call` instructions; dispatch to native code is handled by the
backend.

#### Backend Support

All four execution backends support native rider calls:

- **Interpreter**: function pointers are registered in `NativeFunctionTable`
  and dispatched by symbol name. A rider linked into the running binary
  supplies its own addresses; one found on disk is built into a `.so` and
  loaded via `dlopen`.
- **JIT**: Native functions are declared as Cranelift imports with their C ABI
  signatures. Symbol addresses are registered via the JIT's symbol lookup
  mechanism from the same table the interpreter uses.
- **Cranelift AOT**: Native functions are declared as `Linkage::Import` and
  resolved by the linker against the archive holding the runtime and the
  riders.
- **C AOT** (`aot-compile --c`): Native functions are declared as C prototypes
  and resolved by the linker the same way.

### 9.6 Data

`require data` binds a const to the value a data file holds:

```datalove
require data local/shop/orders: [{sku: string, qty: u32}]
```

The path is found exactly as a module's is, naming a `.dlt` file beside the
package's modules -- `local/shop/orders.dlt` here -- where a module path names
a `.dfm`. The last segment is the name of the const.

The statement is the const `orders: [{sku: string, qty: u32}]` whose value is
the file, and it is that const in every respect: it is borrowed wherever it is
named, cloned out with `@`, in scope below the `require` and for the units of
a session after it, and built once. The file is datalit, which
is the literal syntax of datafun, so its value is the one the same text would
have as a const's initializer: its literals take their types from the type at
the `require`, so `3` is a `u32` there and an `int` without one. With no type
the data has the type it synthesizes, as a const with no hint does.

A data file has no names, no operators and no calls, so it needs no
evaluating; a function reading one is compiled with its value in scope, and
a const may be worked out from data by calling such a function.

A data file that does not exist is F079, as a missing module is. One that is
not of the type the `require` gives is F084, at the `require`, and the
mismatch inside the file is reported where it is in the file.

## 10. Numeric Widening

There is no implicit numeric widening in the language. Every widening is
written with the explicit `@` operator (Section 6.8), and every other numeric
conversion is a function.

Fixed integers cannot use bare arithmetic operators (Section 6.2). To perform
arithmetic, either widen to `int` using `@`, or use checked/optional operators:

```datalove
let a: u32 = 10
let b: u32 = 20
let c: int = a@ + b@    // widen to int, then add
let d: u32 = (a +! b)   // checked add, returns same type
```

Valid widening chains for `@`:

```datalove
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
u8 -> i16 -> i32 -> i64 -> int   (cross-sign)
u16 -> i32 -> i64 -> int         (cross-sign)
u32 -> i64 -> int                (cross-sign)
index -> int
offset -> int
f32 -> f64
```

A chain is shorthand for widening directly to any type after the source, so
`u8` reaches `u64` in one step rather than through `u16`.

`f32` widens to `f64` by `@`, exactly, as `f64.from_f32` does. Floats do not
widen implicitly, and `f64` does not narrow by `@`: `f32.from_f64` converts
that way, rounding, with none for a finite value too large for an `f32` to
hold.

Fixed-width integers do not narrow by `@` either. Each one has `from_X` and
`from_X_wrapping` for every wider fixed-width integer of the same signedness,
so `u8.from_u64` and `i16.from_i32` but not `u8.from_i64`. `from_X` gives
none for a value outside the target's range; `from_X_wrapping` keeps the low
bits. Every fixed-width integer also has `from_int` and `from_int_wrapping`,
the wrapping form keeping the low bits of the value in two's complement.
`index.from_u64` and `offset.from_i64` give none for a value past the width
the build chose, and `index.from_int` and `offset.from_int` likewise;
`index.from_u32` and `offset.from_i32` cannot fail. The other way,
`u64.from_index` and `i64.from_offset` cannot fail, and `u32.from_index` and
`i32.from_offset` give none when a 64-bit build has a value past 32 bits.
`f32` and `f64` each have `from_u8` through `from_u64` and `from_i8` through
`from_i64`, which cannot fail and round to nearest when the value has more
significant bits than the float carries. Narrowing across signedness, and
conversion from the floats to the fixed-width integers other than through
`int`, are not provided yet.

`index` and `offset` widen only to `int`. They are 32-bit or 64-bit
depending on how the compiler is configured, so a conversion to a fixed
width would mean something different in each configuration, while `int`
holds either exactly.

Example requiring explicit widening:

```datalove
fun process(x: int): int
    ret x
end fun

fun example(): int
    let n: u32 = 42
    ret process(n@)     // explicit widen u32 to int
end fun
```

## 11. Ownership Analysis

Ownership analysis runs after type checking to verify correct use of linear
values. Its errors are D001 to D015; the codes from D017 on belong to the
Datalit parser and checker ([Datalit Grammar](datalit-ebnf.md)), which are
reached from Datafun through literals and data files.

### 11.1 Errors

| Code | Name | Description |
|------|------|-------------|
| D001 | UseAfterMove | Using a value after it was moved |
| D002 | DoubleMove | Moving a value twice |
| D003 | CannotMoveBorrowed | Moving a `ref`/`mut`/`out` parameter |
| D003 | CannotMoveConst | Moving out of a const or const parameter without `@` |
| D004 | CannotMutFromRef | Passing `ref` where `mut` required |
| D005 | ReadUninitialized | Reading before initialization |
| D006 | OutParamNotInitialized | Returning without initializing `out` param |
| D007 | MoveInLoop | Moving outer-scoped linear value in loop |
| D008 | InconsistentBranchMove | Branches disagree on whether a value is held: moved, or `set` again, in one but not another |
| D009 | OutParamPartialWrite | Writing fields of `out` param individually |
| D010 | AliasedMutableArgument | Two arguments alias, one is `mut`/`out` |
| D011 | CannotMutateImmutable | Immutable binding passed as `mut`/`out` |
| D012 | CannotMutateTemporary | Temporary passed as `mut`/`out` |
| D013 | UseAfterMoveInEarlierUnit | Using a binding an earlier script unit gave away |
| D014 | InconsistentLoopExit | Moving a value on one way out of a loop and not another |
| D015 | WalkedCollectionChanged | Moving the collection a `for` walks, or passing it `mut`/`out` |
| D015 | WalkedCollectionAssigned | A `set` to or through the collection a `for` walks |

### 11.2 Mutable Arguments

A `mut` or `out` argument is written through, so it must denote something the
caller can assign to: a `var` binding, a projection of one, or a `mut`/`out`
parameter being forwarded.

```datalove
fun bump(mut x: u32)
    set x = icall add_wrapping_u32(x, 1)
end fun

var v: u32 = 1
call bump(mut v)            // ok
var p: (u32, u32) = (1, 2)
call bump(mut p.0)          // ok: a projection of a var

let w: u32 = 1
call bump(mut w)            // error: `w` is immutable
call bump(mut compute())    // error: the write would be discarded
```

An `in` parameter is not assignable either, and a `ref` parameter reports the
more specific D004.

### 11.3 Argument Aliasing

Every parameter is passed by reference, so two arguments naming the same
binding hand the callee two references to one object. That is an error when
either is `mut` or `out`:

```datalove
require module sys/std/string

fun grow(mut self: string, ref other: string)
    call string.push_str(mut self, ref other)
end fun

var s: string = "hi"
call grow(mut s, ref s)    // error (D010): aliased mutable argument
```

Two `ref` arguments are permitted, as are two reads of a copy value:

```datalove
let equal = compare(ref x, ref x)    // ok: both ref
let n = mul_checked(b, b)            // ok: both consume a copy type
```

Arguments are compared by the binding their place is rooted at. Distinct
fields (`f(mut p.x, ref p.y)`) and distinct indexes (`f(mut a[i]?, ref a[j]?)`)
share a root and are rejected, even where they do not overlap in fact.

### 11.4 Loop Restrictions

Moving an outer-scoped linear value inside a loop is an error:

```datalove
var a: int = 0
var b: int = 5
loop
    set a = b      // error (D007): cannot move `b` in loop
end loop
```

Copy types and operator operands (which are borrowed) are exempt.

### 11.5 The Collection a `for` Walks

The collection a `for` walks is frozen for the loop's body, since its element
bindings point into it. Moving it, assigning to it or through it, and passing
it `mut` or `out` are errors (D015):

```datalove
var xs = [1, 2]
for x in ref xs
    set xs[0]! = 5     // error: cannot change `xs` while a `for` walks it
end for
```

Freezing is by the binding the place is rooted at, as aliasing is, so
`for x in ref p.items` freezes all of `p`.

### 11.6 Branch Consistency

Where branches join, each linear value must be held on every branch or on
none (D008). A value moved in one branch must be moved in the others, and one
`set` again after a move in one branch must be set again in the others:

```datalove
fun consume(s: string)
end fun

fun f(c: bool, s: string)
    if c
        call consume(s)     // moves s
    else
        // error (D008): s is not moved here
    end if
end fun
```

The same holds for the ways out of a loop (D014).

### 11.7 Tracking Categories

Bindings are categorized for drop scheduling:

- **Copy**: No tracking needed.
- **Precise**: Ownership state known statically.
- **Tracked**: Runtime tracking byte used.

Tracked bindings are `var` bindings, initialized or not, and `out`
parameters.

## 12. Bidirectional Type Inference

Expressions can *synthesize* types (bottom-up) or *check* against expected
types (top-down).

**Integer synthesis**: bare integer literals synthesize as `int`
(arbitrary-precision). When an expected type is available, integers check
against it instead:

```datalove
let x = 42              // x: int (synthesized)
let y: u32 = 42         // y: u32 (checked against binding type)
```

Checked arithmetic propagates expected types to operands:

```datalove
fun add(): !u32
    ret ok (1 +! 2)    // 1 and 2 infer u32 from context
end fun
```

With no expected type, the two operands of a binary operator are typed
together, as a list's elements are: a literal takes the type of the operand
beside it, on either side.

```datalove
fun f(n: u32): bool
    ret n == 0         // 0 infers u32 from n
end fun
```

With a literal on both sides nothing decides the type, and both take their
default, so `1 == 2` compares bigints. A literal does not take a type it could
not have: `1 + "hello"` is a plain mismatch.

The same holds for the elements of a collection literal, the arguments of a
generic call, and the payloads of `some` and `term` inside them. An empty
collection, `none` or `er` with nothing to go along with needs a type
annotation: `let xs = []` is F011, as `let x = none` is.

Float literals infer their precision from context:

```datalove
fun pi(): f64
    ret 3.14159        // infers f64
end fun
```

`f64` is what a literal falls back to with nothing to infer from, the same
way a bare integer literal falls back to `int`: a width nobody asked for
should be the one that keeps what was written. An `f32` is had by saying so,
with an annotation or a type hint. A negation does not interrupt any of this:
`-3.14159` infers `f64` in the same position, since the sign says nothing
about the width.

The fallback reaches inside collections built from bare literals, so
`[1.0, 2.0]` is a `[f64]`. Where the width is the point rather than the
precision, write it: `let xs: [f32] = [1.0, 2.0]`.

Neither family takes the other's literals. An integer literal where a float
is expected is a mismatch rather than a conversion, and the same in reverse;
`f64.from_int` and `int.from_f64` are the named conversions.

## Appendix A. Command-Line Interface

| Command | Description |
|---------|-------------|
| `script` | Execute a .dfs script |
| `script-ir` | Dump the IR a .dfs script lowers to |
| `script-world` | Execute a worldfile's modules and script units, in one REPL session |
| `aot-compile` | Compile a .dfs script to native code |
| `repl` | Interactive REPL |
| `lit-tycheck` | Type check a datalit expression |
| `lit-ast` | Print datalit AST |
| `lit-pretty` | Pretty-print datalit |
| `lit-op` | Perform datalit operations |
| `typecheck-std` | Type check the standard library |
| `worldgen` | Generate a random worldfile |
| `docs` | Generate HTML documentation from `mandocs/` and `botdocs/` |

`script` takes `--jit` to compile hot functions rather than interpreting them,
with `--jit-threshold` to say how many calls make a function hot,
`--jit-osr-threshold` how many iterations make a loop worth entering in compiled
code partway through a call, `--jit-opt-level` how hard the code generator
works, and `--jit-stats` to report what was compiled; `--time` prints how long
compiling and running took (`--jit-stats` implies it). `aot-compile` takes `-o`
for the output path, `--link`, `--run` and `--c`, the last emitting C and
compiling that rather than emitting an object file directly. `script`,
`script-ir`, `script-world` and `aot-compile` each take `--no-sys` to leave the
system library out. `repl` takes `--script` to run a script through the REPL's
engine without prompting, and `--load` to replay a session `/save` wrote.

A *worldfile* is a single text file holding several modules, data files,
riders and a script as named sections; the tests and `worldgen` use them.
`docs` builds `mandocs/` and `botdocs/` into HTML under `docs/` in the source
tree.

## Appendix B. Unimplemented Features

The following features appear in design documents but are not yet implemented:

- Tensor operations (transpose, slice, reshape), and mutation through a
  tensor view (Section 7.3)
- Recursive types in Datafun (F090); datalit has them (Section 3.8)
- Table operations beyond literals: column projection, row access, adding rows
- Arena blocks
- Memoization
- Type introspection (`@type`)
- Panic and assert statements
- Full Datalove layer (procedures and objects)
