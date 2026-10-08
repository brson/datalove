# Datalove Language Specification

## Contents

- [1. Introduction](#user-content-1-introduction)
- [2. Lexical Conventions](#user-content-2-lexical-conventions)
  - [2.4 Spacing](#user-content-24-spacing)
  - [2.5 Names](#user-content-25-names)
- [3. Types](#user-content-3-types)
- [4. Type Hints](#user-content-4-type-hints)
- [5. Copy and Linear Types](#user-content-5-copy-and-linear-types)
- [6. Expressions](#user-content-6-expressions)
- [7. Place Expressions and Indexing](#user-content-7-place-expressions-and-indexing)
- [8. Statements](#user-content-8-statements)
  - [8.4 Const Parameters](#user-content-84-const-parameters)
  - [8.5 Generic Functions](#user-content-85-generic-functions)
- [9. Module System](#user-content-9-module-system)
  - [9.4 Qualified Calls](#user-content-94-qualified-calls)
  - [9.5 Native Riders](#user-content-95-native-riders)
  - [9.6 Data](#user-content-96-data)
- [10. Numeric Widening](#user-content-10-numeric-widening)
- [11. Ownership Analysis](#user-content-11-ownership-analysis)
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

## 2. Lexical Conventions

### 2.1 Reserved Words

Datalove has no words reserved everywhere. A word is special in a position --
the start of an expression, a type -- and is reserved only for the names that
are read in that position, where it would mean something else. It is refused where such a name is declared (P064), and free
everywhere else.

| Name | Reserved | Why |
|------|----------|-----|
| value (`let`, `var`, `const`, `if`/`else`/`case` bindings) | `true` `false` `none` `some` `ok` `er` `data` `error` `atom` `term` `enum` `not` `icall` | they start an expression |
| parameter | the value words, and `mut` `out` `ref` `const` | the mode is written where the name is |
| function | the value words | a call is read like a value |
| type alias, type parameter | `bool` `u8`..`u64` `i8`..`i64` `index` `offset` `f32` `f64` `int` `string` `data` `error` `atom` `term` `enum` | they are types, or start one |
| module or rider (the alias `require` gives it) | the value words | a qualified call is read like a value |
| field, column, variant | nothing | no word means anything else there |

So a value may be called `type` or `list`, a function `set` or `match`, a type
alias `table` or `Error`, and a module `u8` or `set` -- the standard library's
modules are named after the types they serve.

Every statement begins with its own keyword, a call made for its effect
included (`call`, Section 8.9), so no name is ever read at the start of a
statement and the statement words are reserved for nothing. A new statement
keyword therefore never takes a name away from existing code.
`and`, `or` and `xor` are read only after an operand, and the words
inside constructs (`else`, `end`, `case`, `default`, `with`, `is`, `while`
after `loop`, `module` and `rider` after `require`) only where a name cannot be,
so none of them is reserved.

The lists are in `datalove-datalit/src/parser_util.rs`, which both parsers
read.

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

Bare integer literals synthesize as `int` (arbitrary-precision).
Use a type hint for fixed-width types: `: u32 / 42`.

A hex literal is an unsigned integer: it checks against `int`, the unsigned
fixed-width types and `index`, and against `f32` and `f64` as their bit
patterns. It does not check against a signed fixed-width type or `offset`.

A hex literal takes no sign. A `-` before one is the negation operator, as it
would be before a name, so `-0x10` is the `int` -16 and, bare `-` not being
defined on fixed-width integers, no `i32` or `u32`. A decimal literal does take
its sign: `let x: i32 = -5` is an `i32`. Datalit, having no operators, reads
`-0x10` as an error.

Underscores may group digits, and say nothing about the value:

```datalove
1_000_000
0xFF_FF
```

A separator goes between digits, so a literal begins and ends with one.
`_1` is a name rather than a number, and `1_` is neither.

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

This is also how floats are printed, so a value that comes out of the
compiler can be typed back into it. Magnitudes from `1e-5` up to `1e16`
print positionally, the rest with an exponent, and every finite float
prints with a point or an exponent so that none of them reads as an
integer: `42.0`, not `42`.

**Strings** are enclosed in double quotes:

```datalove
"hello, world"
```

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
answer for the same spelling.

**Literal extent.** A numeric literal is a maximal glued run: an optional sign,
digits, a `.` and a fraction, an exponent marker with its sign and digits. A
gap anywhere inside ends it.

```datalove
-1.5e-7        // one literal
1 . 5          // error: a float is written without spaces: `1.5`
2.5e - 10      // error: an exponent is written without spaces: `2.5e-10`
```

Letters written onto the digits are read as a suffix and reported as one, since
the language has none: `1u8` says to write `: u8 / 1` rather than complaining
about an unexpected identifier.

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
a -b           // `a`, then `-b`: two expressions, not one
p . 0          // error: a postfix operator is written against the expression before it
x ?            // error: likewise
f (1)          // error: a call's `(` is written against what is called, as a postfix operator is
a--b           // `a - (-b)`
```

Fixity is for the sigil operators. A word operator -- `and`, `or`, `xor`, `not`
-- is delimited by being a word, and gluing it to an operand would make it part
of the operand. The postfix operators `?`, `!`, `@`, `.field` and `[index]`
require gluing on their left.

The literal reader runs first and takes what is glued to it, and fixity judges
only the operators it declined. That is why `2.5e-10` is one number while `x-1`
subtracts.

This is what settles how many elements a tensor literal's innermost axis has,
that being the one place in the language where members are separated by nothing
at all (Section 3.2).

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

## 3. Types

### 3.1 Primitive Types

| Type | Description |
|------|-------------|
| `bool` | Boolean value |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers |
| `i8`, `i16`, `i32`, `i64` | Signed integers |
| `index` | Addressable size of indexed collections |
| `offset` | Signed addressable size etc. |
| `f32`, `f64` | Floating-point numbers |
| `int` | Arbitrary-precision integer |
| `string` | UTF-8 string |

The `index` and `offset` types are 32-bit by default, or 64-bit when the
`index-64` feature is enabled.
`index` is not the same as the platform pointer size,
which is not exposed to the language.
The size of `index` is less than or equal to the platform pointer size.

**A count is always a `u32`.** Every bit count, shift amount, rotate amount
and exponent in `sys/std` is a `u32`, whatever the width of the value it
counts or shifts: `u8.count_ones(self: u8): u32`, `u64.bits(): u32`,
`i64.shift_left(self: i64, n: u32): ?i64`, `u8.pow_checked(self: u8, exp: u32)`.
This is Rust's rule, and it is here for the same two reasons. A count is
bounded by the widest integer, so `u32` always holds it and never needs a
register wider than one; and hardware takes a shift amount in a fixed narrow
place and masks it, so a shift amount of the shifted type asks a question the
machine does not answer. It also lets counts compose: a count out of one width
feeds a shift at another, which a same-width rule cannot express.

`abs_diff` is the exception, because a magnitude is not a count: it gives the
unsigned type of the same width, so `i32.abs_diff` gives a `u32` and
`offset.abs_diff` gives an `index`.

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

Semicolons permit single-line format: `{| x, y; 1, 2; 3, 4 |}`. A `;` or a `,`
goes between two things, so one with nothing before it is an error
(`{| x, y;; 1, 2 |}`); a trailing one closes what it follows and is fine. This
holds wherever the two languages read a delimiter -- table rows and columns,
tensor axes, and the `;` between two statements.

A table is opaque: nothing can look inside one. Column projection
(`table.x`) is not implemented and is refused as a projection on a value with
no fields (F068); the options are written up in
[Tables: what the type system is missing](design-table-rows.md).

A projection of a field whose type is linear may not be read as a value: it
would move the field out of an aggregate that still holds it. It may be
borrowed -- `debuglog p.a`, or `f(ref p.a)` and the `mut` and `out` forms -- or
cloned out with `@`, as in `let x = p.a@`. A field of a copy type reads
directly.

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
only between two parts, so a tensor takes no trailing comma.

A shape the separators cannot show -- a zero extent above the innermost axis,
or a leading extent of 1 -- is written in a header before a `|`, and the body
after it either flat, in row-major order, or shaped as the header says:
`[| 1 3 | 1 2 3 |]` has shape [1, 3], `[| 0 3 | |]` is an empty 0x3 tensor,
and `[| 2 3 | 1 2 3 4 5 6 |]` is a header over a flat body. A tensor has at
least one axis: `[|T, 0|]` is refused.

Because the innermost axis is separated by nothing but whitespace, an
operator's spacing decides how many elements a row has (Section 2.4):
`[| 1 -2 |]` holds two elements and `[| 1 - 2 |]` holds one.

Indexing a tensor is exposed: `t[i]?` reads along axis 0, and `set t[i]? = v`
writes there. `sys/std/tensor` is written over that, at rank 1. Transpose,
slice and reshape are not exposed; the runtime supports them internally.

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
*primary* expression as their payload -- a literal, variable, function call,
parenthesized expression, hinted expression or prefix operator -- together with
the postfix operators written on it, so `some x@` is `some (x@)` and
`some o?` is `some (o?)`. This is the same with a hint over the constructor:
`: ?int / some o?` is `some (o?)` too. A binary operator after a payload is
P065: `some a + b` is refused, and is written `some (a + b)`, or `(some a) + b`
if the operator is meant for what is built:

```datalove
some 42                  // ok: literal is primary
some(a +? b)             // ok: parenthesized expression
ok result                // ok: variable is primary
ok(x +! y)              // ok: parenthesized expression
er(error "msg")          // ok: parenthesized expression
```

### 3.5 Data Type

The `data` type is a universal container that can hold any value:

```datalove
data 42
data : int / 100
data(a + b)              // parenthesized for binop payload
```

The `error` type is an existential error value:

```datalove
error "message"
error(some_expr)         // parenthesized for non-primary payload
```

Both `data` and `error` take a primary expression as their payload (see Section 3.4).
A value is a `data` only when written as one. Nothing becomes a `data` by being
where one is expected: `let d: data = x` is a mismatch, and `let d: data = data
x` is how to write it. Datalit is the same.

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

Enum variants are matched by name. Atoms and terms can stand alone as types,
or combine into enums.

**Enum values.** An atom or term literal is a value of any enum that lists
it, so where an enum type is expected it is written bare, or wrapped as an
enum literal:

```datalove
let c: enum { atom Red, atom Blue } = atom Red
let d: enum { atom Red, atom Blue } = enum { atom Blue }
let e: enum { atom Red, term Custom string } = term Custom "hello"
```

An enum literal has no type of its own and needs one expected. This is the
same in datalit.

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

**Match.** Enums are destructured with `match` (see Section 8.6).

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

The expression may be any expression, not only a literal, and the hint is what
it is checked against:

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
`some` and its fellows: `: u32 / o?` hints what the `?` produces, not the
option it unwraps. A hint takes nothing of its own, so the expression under it
moves what it would have moved written without one.

A bare name in type position is a type alias (Section 3.7), or, within a generic
function's signature or body, one of its type parameters (Section 8.5). Data
literals have neither, so a name in a data literal's type is always an error.
A name that is neither is F064, wherever the type is written: a `let`, `var` or
`const` annotation, a hint, a parameter, a return type, or inside any of those.

A collection type is written with its sigil rather than its name, so `tuple`,
`list`, `map`, `set`, `table` and `tensor` are not types. They are names like any
other, free for an alias or a type parameter; where none of that name is in
scope the F064 saying so adds how the collection is written. A primitive written
in another case, `Int`, gets the same treatment, with the spelling it meant.

## 5. Copy and Linear Types

Types are classified as *copy* or *linear*.

**Copy types** can be freely duplicated: `bool`, fixed-width integers (`u8`
through `u64`, `i8` through `i64`), `index`, `offset`, `f32`, `f64`.
Atoms are always copy. Terms are copy if their payload type is copy.
Enums are copy if all variant payloads are copy.

**Linear types** have move semantics: `int`, `string`, `[T]`, `%{K = V}`,
`#{T}`, `{| ... |}`, `[|T, N|]`, `data`, `error`. Terms and enums with
linear payloads are linear.

A **type parameter** is linear, whatever it is instantiated with, because the
caller may supply a linear type and the function is compiled once for all of
them (Section 8.5).

A linear value can be used exactly once. After a value is moved, subsequent
uses are compile-time errors:

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
| 2 | `-` `-?` `-!` `not` `some` `ok` `er` `data` `error` | Unary prefix |
| 3 | `.field` `.0` `[i]` `@` `?` `!` | Postfix (field, index, adapt, try) |
| 4 | `*` `/` `*!` `/!` `*?` `/?` | Multiplicative |
| 5 | `+` `-` `+!` `-!` `+?` `-?` | Additive |
| 6 | `.<` `.>` `<=` `>=` `==` `!=` | Comparison |
| 7 | `and` | Logical AND |
| 8 | `or` `xor` | Logical OR/XOR |

Binary operators are left-associative, so `a - b - c` is `(a - b) - c`, except
that comparisons do not chain (Section 6.3). Postfix operators chain left to
right: `a.b[0]?@` applies each in turn.

Postfix binds looser than unary prefix, so a postfix operator applies to the
prefix expression as a whole: `-x@` is `(-x)@`. The payload keywords `some`,
`ok`, `er`, `data` and `error` are the exception, taking postfix onto their
payload instead, so `some x@` is `some (x@)`.

Which of the three a sigil operator is read as is decided by its spacing before
precedence is consulted at all: `a - b` and `a-b` are the binary operator in
this table, and `a -b` is not one (Section 2.4).

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

Bigint division `/?` yields `int`. Unary `-?` is permitted only for signed
fixed integers.

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

The ordering operators `.<`, `.>`, `<=` and `>=` take numeric operands only.
Order strings with `string.cmp`, and anything else with `sys/std/ord`.

`==` and `!=` take a number, `bool`, `string`, unit or an atom, and an
option, tuple, struct, term or enum whose parts all do. Such a value is
compared part by part. Floats compare by IEEE 754 wherever they are, so
`some nan != some nan` and `(0.0, 1) == (-0.0, 1)`. Lists, sets, maps,
tables, tensors, results, `data`, `error` and functions have no equality
operator yet, and neither does an aggregate holding a type parameter.

A construction with no type of its own takes the other operand's: with
`x: ?u32`, `x == none` and `x == some 1` compare `?u32`s.

Comparisons do not chain. `a == b == c` is refused; parenthesize the
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
    ret some (x + 1)
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

### 6.6 Operator Argument Semantics

All operators treat their operands as immutable references. Operands are not
consumed:

```datalove
let x: int = 42
let a = x + 1       // x is cloned for the operation
let b = x + 2       // x can be used again
ret x               // x is still valid
```

### 6.7 Intrinsic Calls

Intrinsics are low-level operations that compile to machine instructions:

```datalove
icall intrinsic_name(args)
```

Available intrinsics include bitwise operations (`bitnot_u32`, `bitand_u32`,
`bitor_u32`, `bitxor_u32`), shifts (`shl_u32`, `shr_u32`), bit counting
(`popcount_u32`, `clz_u32`, `ctz_u32`), byte manipulation (`swap_bytes_u32`,
`reverse_bits_u32`), wrapping arithmetic (`add_wrapping_u32`,
`sub_wrapping_u32`, `mul_wrapping_u32`), and type reinterpretation
(`u32_to_i32`, `i32_to_u32`).

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

**Widen**: For fixed integers, `@` widens to a larger type:

```datalove
let n: u8 = 42
let x: int = n@         // widen u8 to int
```

**Cross-sign widen**: Unsigned integers can widen to larger signed types:

```datalove
let n: u8 = 255
let x: i16 = n@         // u8 widens to i16 (value fits)
```

**Atom/term to enum**: A value of atom or term type widens to an enum type
that lists it. Like every `@`, it copies, and the original stays usable:

```datalove
type Color: enum { atom Red, atom Blue }
let a = atom Red
let c: Color = a@            // atom widens to enum
```

A literal needs no `@` for this: `let c: Color = atom Red` checks the atom
against the enum directly (Section 3.6). An enum does not widen to another
enum.

Valid widening chains:

- Same-sign: `u8` -> `u16` -> `u32` -> `u64` -> `int`
- Same-sign: `i8` -> `i16` -> `i32` -> `i64` -> `int`
- Cross-sign: `u8` -> `i16`, `i32`, `i64`, `int`
- Cross-sign: `u16` -> `i32`, `i64`, `int`
- Cross-sign: `u32` -> `i64`, `int`
- Index/offset: `index` -> `int`, `offset` -> `int`

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
- An operand of an operator whose operands have the same type as its result:
  `+`, `-`, `*` and `/` where that type is one they accept, the checked and
  optional operators, and `and`, `or` and `xor`. The expected type of the
  whole expression reaches both operands, so `let c: int = a@ + b@` widens
  both to `int`.
- An operand of a comparison, when the other operand has a type of its own.
  A comparison returns `bool`, which says nothing about its operands, so the
  type comes from across the operator instead: `a@ .< b` takes `b`'s type,
  and `a@ .< 0` takes the literal's. With `@` on both sides there is nothing
  to take, so each clones and the two have to agree on their own.

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
used as a `set` target or `mut` argument.

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

Field access on a linear-type field requires the place to be in a reference
context (`ref` param, `mut` param, binop operand, `set` LHS). In consume
context (`let`, `ret`, `in` param), linear fields require explicit `@` clone:

```datalove
let t = (1, 2)
let x = t.0                   // error: int is linear
let x = t.0@                  // ok: explicit clone
call foo(ref t.0)                   // ok: borrow
```

Copy-type fields (bool, fixed integers, floats) are freely extracted.

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
a non-copy element in consume context requires `@` (clone): `t[i]?@`.

**View type restrictions:** Views can be passed to `ref` params but not to
`mut` or `out` params. Whole-value replacement of a view (`set row = ...`)
would overwrite the view struct without affecting the parent's data, causing
leaks. The `set t[i]? = new_row` form is likewise rejected for rank > 1
tensors. Element-level mutation through views (e.g. `set row[j]? = val`)
is semantically correct but currently also rejected because the typechecker
cannot distinguish it from whole-view replacement at the call site. The view
concept is general; tensor sub-views are the only instance today.

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
| binop operand | Immutable borrow | `a[i]? + 1` |

In consume context (`let`, `ret`, `in` param), copy-type elements are copied
out freely. Linear-type elements require explicit `@` clone -- moving an
element out of its container would leave a hole.

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

A struct pattern names every field, and `let (a)` is an error (P070): a
pattern has nothing to group, so it is spelled `(a,)` or `a`. Patterns are one
level deep, and an enum is taken apart with `match` rather than `let`. A `var`
without a value binds a plain name.

**Const** binds a value the compiler evaluates. It is valid at script top
level, in a script or module function body, and at module top level, where it
is in scope for every function in the module regardless of where it is written:

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
`@`. A move without one is D003, whose help says where the `@` goes;
auto-adapt supplies it. A const of a copy type is copied as any copy type is.

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

**Ordering.** A module-level const is evaluated after the functions it calls are
compiled, and before the functions that name it. Functions naming no
module-level const are therefore compiled first, which is what lets a const call
a function while another function reads that const. If a const calls a function
that itself names a module-level const, the two depend on each other and the
compiler reports it rather than choosing an order.

**Set** mutates a var binding, mutable parameter, or indexed/chained target:

```datalove
set x = x + 1
set a[i]? = 5
set a[i]?.field = 10
set m[key]? = v            // update: fail if key absent
set m[key] = v             // upsert: insert or overwrite
```

The `set` target is a place expression (see Section 7). The root must be
`var` or `mut` (F055 otherwise). Mutability propagates backward through the
chain: `set a[i]?.x = 5` needs mutable access to `a[i]?` and so to `a`, so a
`let` or `ref` root is rejected however deep the target. For indexed targets,
the `?` or `!` on each index step provides early-return on failure; the write
only happens if all checks pass.

**Map upsert.** Bare `set m[key] = v` (without `?` or `!`) is valid only for
maps. It inserts if the key is absent, overwrites if present. Lists reject
bare index on `set` LHS -- list elements must exist to be overwritten.

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
set xs[i]!.n *?= 2
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
and `?` forms an option, and on failure return early, leaving the place as it
was (F049). Another operator or type is F026.

The place is evaluated once, in the order of any `set`: the value, then its
keys, then its lookups, each a single time, and then the operator reads the
place and the value and stores its result there.

The place has to hold a value when it's reached, since it is read: a
moved-out variable, an unwritten `out` parameter, or one the value moves
(`set x += f(x)` with `f` taking `x` by value) is an error. The value is an operand, so it is
borrowed rather than consumed, and it may be the place itself (`set x += x`).
A bare map index is rejected (F074): it inserts a missing key, and then there
is no value to update.

### 8.2 Functions

Function definition:

```datalove
fun name(param1: T1, param2: T2): ReturnType
    // body
    ret value
end fun
```

Void functions omit the return type and may omit `ret`:

```datalove
fun log(msg: string)
    // body
end fun
```

### 8.3 Parameter Modes

All parameters are passed by reference. The mode determines permitted
operations:

| Mode | Syntax | Semantics |
|------|--------|-----------|
| `in` | `x: T` | Caller transfers ownership; callee consumes |
| `ref` | `ref x: T` | Caller retains ownership; callee reads only |
| `mut` | `mut x: T` | Caller retains ownership; callee may mutate |
| `out` | `out x: T` | Callee initializes; caller receives value |

**Restrictions:**
- `ref`, `mut`, and `out` parameters cannot be moved.
- `out` parameters must be initialized before the function returns.
- `ref` parameters cannot be passed to `mut` parameters.
- `out` parameters must be written as a whole, not field-by-field.
- View types (e.g. tensor sub-views from rank > 1 indexing) cannot be
  passed to `mut` or `out` parameters. Use `ref` instead.

**Call-site markers.** Every argument repeats its parameter's mode, so that
borrowing and mutation are visible where the call is written rather than only
in the callee's signature. `in` is written by omitting the marker:

```datalove
fun mixed(a: u32, ref b: u32, mut c: u32): u32

call mixed(x, ref y, mut z)
```

A marker that disagrees with the declared mode is an error (F057), including
a missing marker for a `ref`, `mut` or `out` parameter and a marker on an
`in` parameter. Intrinsics take every argument by value, so `icall` arguments
never carry a marker.

### 8.4 Const Parameters

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
- Cannot combine `const` with a passing mode. A const parameter is passed by
  reference, as any const is borrowed: a specialized copy has the value written
  in and is passed nothing, and a call that is not specialized borrows the
  argument. `ref` would say what it already is, and `out` and `mut` would write
  to a constant
- A const parameter's type cannot be a type parameter (`const n: T` in a
  generic function is an error). A const parameter of a concrete type in a
  generic function is fine: specialization removes const parameters and
  erasure replaces type parameters, so one copy per const instantiation
  serves every type instantiation
- Nothing else is accepted, not a literal and not an expression: `repeat(3, ref s)`
  is refused as surely as `repeat(2 + 1, ref s)`. The value a const parameter takes
  has to be one the compiler already holds, and a `const` binding is the one
  form that says so on its face. A const parameter counts, being a const
  binding within the body

**Implementation:** The compiler monomorphizes, keeping the original function
and adding a copy per instantiation beside it. See
[Const Parameter Specialization](compiler-guide.md#user-content-const-parameter-specialization)
in the compiler guide.

### 8.5 Generic Functions

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
arguments. Each argument is matched against its parameter's type, and wherever
the parameter has a type parameter, whatever the argument has in that position
is what it stands for:

```datalove
call unwrap_or(some (: u32 / 7), : u32 / 0)      // T is u32
call unwrap_or(some "hello", "fallback")         // T is string
```

The first argument to reach a type parameter fixes it, and the rest are checked
against the result, so a disagreement is a type mismatch rather than a
reinterpretation:

```datalove
call pick_first(: u32 / 1, "not a u32")          // error: expected u32, found string
```

An argument that cannot say what it is on its own binds nothing, and is checked
afterwards against whatever another argument fixed. So `none` is usable where
something else determines the type:

```datalove
call unwrap_or(none, "fallback")                 // T is string, from the second
```

Because the first argument fixes it, argument order decides whether a call
type-checks at all. An unsuffixed integer literal with nothing to fix it is
`int`, so `pick_first(99, : u32 / 1)` makes `T` `int`, and the `u32` is a
mismatch, since nothing widens implicitly. Written the other way round,
`pick_first(: u32 / 1, 99)`, the `u32` fixes `T` first and the literal is
checked against it, so `99` is a `u32`.

**Where a type parameter may appear.** Anywhere: on its own, under `?` or `!`
to any depth, and inside a list, set, map, tensor, table, tuple, struct, term
or enum payload, in any parameter mode and in the return.

```datalove
fun flatten_or<T>(x: ??T, default: T): T
fun sorted<T>(x: [T]): [T]
fun swap<T>(x: (T, T)): (T, T)
fun tagged<T>(x: enum { term Some T, atom None }): bool
```

**Representation.** A generic function is compiled once, not once per type, so
a type parameter has no fixed size and the shapes it stands in have to be made
to fit one. The mode decides whether a value is converted at all, and the shape
decides how:

A type parameter standing alone is carried as a `data`, which holds a value of
any type along with what is needed to clone and drop it. An option, a result, a
tuple, a struct, a term and an enum payload hold what they hold inline, so
converting one means converting each part and writing it where the other side
keeps it -- a walk over as many parts as the type has, and no more.

A collection is a different case. A list is a pointer, a length and a capacity
whatever its elements are, so it is already the right size; what it lacks is the
element type. An owned one is wrapped whole into a `data`, which costs one small
allocation and leaves the elements untouched.

That is all for a value the callee owns -- an `in` or `out` parameter, or the
return. A `ref` or `mut` parameter is not converted at all: the value is passed
as it stands, and the descriptor saying what it really is comes from the call
site.

None of this is visible in a program except in what it costs. The call site
converts on the way in and moves the value back out on the way back, because it
is the place that knows the type. An `out` parameter goes both ways: whatever
the destination held is dropped at the call, and what the callee writes is moved
back out into whatever the caller keeps there.

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

**Bounds.** A signature may end with a `with` clause, which bounds a type
parameter:

```
fun scaled<T>(a: T, b: T): T with { T is float, }
  ret a * b
end fun
```

A bound says the parameter is one of a fixed set of types, and in exchange the
body may do what all of them have in common. The bounds sit apart from the
names so that the signature reads as a signature. The clause may be spread over
several lines, but the `with` has to be on the signature's own line, because
that is where the signature ends.

There are three bounds:

- **`ord`** is every type there is, and says that the total order every value
  has may be used. It is the weakest bound: what it rules out is a type with no
  ordering, of which the language has none today. It is worth asking for
  because the order is otherwise out of reach — `.<` is for numbers only, and
  `==` is refused for a list, a result, a type parameter inside an aggregate
  and more (section 6.3), whether or not a generic is involved. `sys/std/ord`
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
  the comparisons and the checked and optional arithmetic — `+!`, `+?` and the
  rest. Bare `+` is not among them, because a fixed-width integer does not have
  one. Negation is the result form `-!` and not the optional `-?`, which is
  refused for unsigned operands and so refused for a parameter that may turn
  out to be one. `sys/std/fixedint` is what is written on top of that: the
  arithmetic, and the constants. Comparing and sorting are not there, because
  they ask nothing of a fixed-width integer that another type cannot answer.

Erasure still compiles one body, so the machine code cannot hold the
instruction for every type the bound admits. The operands arrive with their
descriptors and the runtime reads which type it is, the same way a collection
of a type parameter has its elements walked by a size read off a descriptor. A
call site that binds the parameter to anything else is refused.

A value of a bounded parameter is still linear, so handing one to a function
twice asks for a clone; reading one as the operand of an operator does not move
it.

A literal cannot be written at type `T`, because a literal has to be written at
some type and inside a generic the type is what nobody has picked yet. The
constants come from the standard library instead — `zero`, `one`, `min_value`
and `max_value` in `sys/std/fixedint`, and those plus `nan`, `infinity`,
`epsilon`, `pi` and the rest in `sys/std/float` — which take theirs from a
descriptor the call site hands over. That works because the type parameter appears only in
the return, so the call site is the one place that knows; see
[the native ABI](native-abi.md). The consequence is that such a call has to say
what it wants:

```
let z: T = zero()      -- the binding says `T`, so this works
ret self == zero()     -- refused: nothing on this line says what `T` is
```

An operand is read for what it is rather than checked against what is wanted,
so a call in operand position has nothing to take its type from. That is
refused rather than guessed at.

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
which is what `list.zip` and `map.entries` are. An element crossing into such a
collection is converted position by position against the collection's own
descriptor, the same way an owned parameter crossing a call is. A collection that arrives
carries a descriptor saying what its elements are; one written here has no value
to read one off, so the call site hands one over — it is the only place that
knows what the parameter was bound to. Nothing is worked out at run time,
because `[T]` with `T` bound to a concrete type is a concrete type, and that
has a descriptor already.

Which descriptors a function is handed is what its own body builds, closed over
its calls: a generic that passes its type parameter to one that builds a
collection of it carries a descriptor too, and forwards it. A generic that
builds nothing carries none.

One program is refused. A generic that builds a collection of its type
parameter and calls a generic at a strictly larger type needs a description one
level deeper at every call, so the set never settles. Erasure means such a
function compiles to one body and would simply recurse for ever at run time;
what cannot be written down is the descriptors, not the code.

How this is implemented is in [Generics: how it works](generics.md); what is
refused and why is
[What is refused](generics.md#user-content-what-is-refused).

### 8.6 Control Flow

**If statement:**

```datalove
if condition
    // then branch
else
    // else branch
end if
```

**If with binding** unwraps an option or result:

```datalove
if opt |value|
    // value is bound here
end if
```

A result's `else` must bind the error, `else |e|` (F046). A binding `if` takes
no part in an `else if` chain, on either side (P073): it cannot be followed by
`else if`, and an `else if` cannot bind. Nest the `if` in an `else` body
instead. An `else |e|` cannot be followed by `if` either (P066).

**Loop:**

```datalove
loop
    if done
        break
    end if
end loop
```

**Conditional loop:**

```datalove
loop while condition
    // body
end loop
```

`break` exits the innermost loop. `continue` jumps to the next iteration.

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
match must be exhaustive (all enum variants must be covered). Duplicate cases
are an error, and so is a second `case default`.

The input expression is consumed (moved) by the match.

A `case term` arm's binding belongs to that arm. It may take a name already in
scope, and the outer one means what it meant again after `end match`.

### 8.7 Return

`ret` returns a value from a function:

```datalove
ret 42
```

Void functions may use bare `ret` for early exit:

```datalove
ret
```

### 8.8 Debug Log

`debuglog` prints a value:

```datalove
debuglog "red"
debuglog p.a
```

It accepts an expression of any type, and borrows rather than consumes it, so
a linear value may be logged and then used. That is why a projection of a
linear field may be written under it without a `@` (Section 3.2).

### 8.9 Call Statements

A function called for its effect is written after `call`:

```datalove
call bump(mut v)
```

`call` takes only a function call. Anything else after it is P031, since a
value computed and discarded is more likely a mistake than an intention. A
line that begins with no statement keyword at all is P001.

## 9. Module System

### 9.1 Hierarchy

The module system has three levels: library, package, module.

A **library** is a directory of packages (e.g. `sys/`, `local/`).
A **package** is a directory of modules and data (e.g. `sys/std/`).
A **module** is a single `.dfm` file (e.g. `sys/std/u32.dfm`).
A **data file** is a single `.dlt` file of datalit (e.g. `local/shop/orders.dlt`),
which `require data` reads; see Section 9.6.

The `sys` library is compiled into the `datalove` binary. The `local` library
is a workspace's own, read from disk by the command line (`script`,
`script-ir`, `aot-compile`, `repl`) from a directory named `local`:

- beside the script, when there is one;
- else, for a script in a directory named `scripts`, beside that directory;
- else, with no script (the interactive REPL), in the current directory.

A command given a script never looks in the current directory, and nothing
further up the tree is looked at. There is no workspace manifest. The pipeline
itself takes whatever libraries it is given; the rule is the command line's.

A package in `local` may have a rider. The system library's riders are linked
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

A `require` or `import` is read only at the top level of a module or script,
and one inside a block is P074. Where it stands at the top level does not
matter: the requires and imports are gathered before anything is resolved, so
a call above the `require` it goes through is fine.

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
is an error rather than a shadowing. Two modules exporting the same name is
ordinary - `min` is in every numeric module - so only one of them can be
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
native fun string_contains(ref haystack: string, ref needle: string): bool
native fun list_push<T>(mut self: [T], elem: T)
```

A native function may take type parameters. Nothing is converted at one:
because every parameter arrives as a pointer and a descriptor, the value goes
across as it stands whatever the mode, where an ordinary function converts an
owned one into the shape it was compiled for (Section 8.5). The implementation
reads the element type off the descriptor at runtime, so one implementation
serves every element type.

#### Rider Crate

The rider crate (`rider/src/lib.rs`) implements native functions as
`extern "C-unwind"` functions following the rider C ABI. Each parameter
is a `(ptr, tydesc)` pair, with an out-param for the return value:

```rust
#[unsafe(no_mangle)]
pub extern "C-unwind" fn dlr_std__string_len(
    rt: LocalRtHandle,
    self_ptr: *const u8,
    self_tydesc: *const TyDesc,
    result_out: *mut u8,
    result_tydesc: *const TyDesc,
) -> RtStatus { ... }
```

Symbol naming convention: `dlr_{rider_name}__{function_name}`.

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

All three execution backends support native rider calls:

- **Interpreter**: function pointers are registered in `NativeFunctionTable`
  and dispatched by symbol name. A rider linked into the running binary
  supplies its own addresses; one found on disk is built into a `.so` and
  loaded via `dlopen`.
- **AOT**: Native functions are declared as `Linkage::Import` and resolved by
  the linker against the archive holding the runtime and the riders.
- **JIT**: Native functions are declared as Cranelift imports with their C ABI
  signatures. Symbol addresses are registered via the JIT's symbol lookup
  mechanism from the same table the interpreter uses.

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
named, cloned out with `@`, in scope for every function of the module or for
the units of a session after it, and built once. The file is datalit, which
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

There is no implicit numeric widening in the language. All numeric conversions
require the explicit `@` operator.

Fixed integers cannot use bare arithmetic operators. To perform arithmetic,
either widen to `int` using `@`, or use checked/optional operators:

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

fun example(): int
    let n: u32 = 42
    ret process(n@)     // explicit widen u32 to int
```

## 11. Ownership Analysis

Ownership analysis runs after type checking to verify correct use of linear
values.

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

### 11.2 Mutable Arguments

A `mut` or `out` argument is written through, so it must denote something the
caller can assign to: a `var` binding, a projection of one, or a `mut`/`out`
parameter being forwarded.

```datalove
fun bump(mut x: u32)

var v: u32 = 1
call bump(mut v)            // ok
call bump(mut v.0)          // ok for a var aggregate

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
fun grow(mut self: string, ref other: string)

var s: string = "hi"
call grow(mut s, ref s)    // error: aliased mutable argument
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
var b: int = 5
loop
    set a = b      // error: cannot move 'b' in loop
end loop
```

Copy types and operator operands (which are borrowed) are exempt.

### 11.5 Branch Consistency

If a value is moved in one branch, it must be moved in all branches:

```datalove
if cond
    call consume(x)     // moves x
else
    // error: x not moved here
end if
```

### 11.6 Tracking Categories

Bindings are categorized for drop scheduling:

- **Copy**: No tracking needed.
- **Precise**: Ownership state known statically.
- **Tracked**: Runtime tracking byte used.

Tracked bindings include `var` bindings, `out` parameters, and uninitialized
variables.

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

With no expected type for a binary operator, one operand can supply the other's.
When exactly one side is a bare numeric literal and the other has a numeric type
of its own, the literal checks against that type:

```datalove
fun f(n: u32): bool
    ret n == 0         // 0 infers u32 from n
end fun
```

With a literal on both sides there is nothing to propagate and both synthesize
`int`, so `1 == 2` compares bigints. A non-numeric operand supplies nothing
either: `1 + "hello"` is a plain mismatch.

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
| `script-world` | Execute a worldfile's modules and script section |
| `aot-compile` | Compile a .dfs script to native code |
| `repl` | Interactive REPL |
| `lit-tycheck` | Type check a datalit expression |
| `lit-ast` | Print datalit AST |
| `lit-pretty` | Pretty-print datalit |
| `lit-op` | Perform datalit operations |
| `typecheck-std` | Type check the standard library |
| `worldgen` | Generate a random worldfile |
| `docs` | Generate HTML documentation from `mandocs/` |

`script` takes `--jit` to compile hot functions rather than interpreting them,
and `aot-compile` takes `--link`, `--run` and `--c`, the last emitting C and
compiling that rather than emitting an object file directly. `script`,
`script-ir`, `script-world` and `aot-compile` each take `--no-sys` to leave the
system library out.

## Appendix B. Unimplemented Features

The following features appear in design documents but are not yet implemented:

- Tensor operations (transpose, slice, reshape) --
  tensor literals and axis-0 indexing work (`t[i]?`/`t[i]!`),
  but higher-level manipulation operations are not exposed.
  Mutation of sub-tensor views via `mut` params is rejected;
  element-level mutation through a view requires direct `set` on the
  original tensor with chained indexing.
- Arena blocks
- Memoization
- Type introspection (`@type`)
- Panic statement
- Full Datalove layer (procedures and objects)
