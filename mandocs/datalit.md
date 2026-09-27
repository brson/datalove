# Datalove Literals

Datalove literals is
a typed declarative expression language for
serializing, storing and transmitting
common data types.
We often refer to it as _datalit_,
and its types and datalit types.

Datalove literals is the data sublanguage of Datalove:
every datalit expression is written the same way in Datalove,
where it means the same value.
Datalit has no names, no operators and no computation.
Datalit files use the `.dlt` extension.
The format is intended for use in the Datalove ecosystem
and not as a general-purpose serialization format.

This document is an informal specification for Datalove literals
and serves as a gentle introduction to the data model
and syntax of Datalove in general.


## Contents

- [An example](#user-content-an-example)
- [Lexical structure](#user-content-lexical-structure)
- [Expressions](#user-content-expressions)
- [Primitive Types](#user-content-primitive-types)
- [Collections](#user-content-collections)
- [Aggregates](#user-content-aggregates)
- [Dynamic Types](#user-content-dynamic-types)
- [Types](#user-content-types)
- [Comparison and total ordering](#user-content-comparison-and-total-ordering)
- [Canonical forms](#user-content-canonical-forms)
- [Discrepancies, unknowns and bugs](#user-content-discrepancies-unknowns-and-bugs)




## An example

```datalove
{
  name = "datalove",
  version = (0, 1, 0),
  tag = (true,),
  unit = (),
  enabled = true,
  score = 99.5,
  flags = 0xFF,
  diff = -1,
  tags = ["fast", "typed", "portable"],
  counts = %{ "a" = 1, "b" = 2, "c" = 3 },
  ids = #{ 10, 20, 30 },
  matrix = [| 1.0 0.0, 0.0 1.0 |],
  cube = [|
    1 2,
    3 4,,
    5 6,
    7 8,
  |],
  metrics = : {| name: string, value: f64 |} / {|
    name, value
    "latency", 0.5
    "throughput", 1000.0
  |},
  config = some {
    retries = 3,
    timeout = 30.0,
  },
  backup = : ?string / none,
  status = ok "healthy",
  failure = : !string / er error "oops",
  state = atom Ready,
  event = term Click (100, 200),
  kind = : enum { atom Ready, term Custom string } /
    enum { term Custom "experiment" },
  payload = data [1, 2, 3],
}
```




## Lexical structure

Source text is UTF-8.

Whitespace is any Unicode whitespace character.
It separates tokens and is otherwise insignificant,
with two exceptions:
newlines separate table rows,
and spaces separate the elements of a tensor row.
A newline is `\n`; a `\r` before it is ordinary whitespace.

Comments are either `//` to end of line,
or `/* ... */`, which nest.
Comments are whitespace.

Identifiers name struct fields, table columns,
and atom, term and enum variants.
An identifier is a run of letters, digits and `_`
not beginning with a digit.
Letters are Unicode alphabetic characters, so `café` is an identifier.

These words are keywords:

```
true  false  none  some  ok  er  data  error  atom  term  enum
```

In type position the primitive type names are also reserved:
`bool`, `int`, `string`, `u8` .. `u64`, `i8` .. `i64`,
`index`, `offset`, `f32`, `f64`, `data`, `error`.

A keyword may still be used as a struct field or table column name,
where only a name can appear: `{ none = 1 }` is a struct.

Tokens that are written against each other are _glued_.
Gluing decides how far a numeric literal reaches:
a number is a maximal glued run of sign, digits, point and exponent,
so `-1.5e-7` is one literal
and `- 1`, `1 . 5` and `2.5e - 10` are errors.
Letters glued to the end of a number are an error,
as datalove has no numeric suffixes:
`1u8` is written `: u8 / 1`.
Inside a tensor, gluing is what separates `[| 1 -2 |]`, two elements,
from `[| 1 - 2 |]`, an error.


### EBNF · Lexical

```ebnf
ws             = { whitespace | comment } ;
whitespace     = ? any Unicode whitespace character ? ;
newline        = "\n" ;
comment        = line_comment | block_comment ;
line_comment   = "//", { ? any char except newline ? } ;
block_comment  = "/*", { block_comment | ? any char ? }, "*/" ;

ident          = ident_start, { ident_start | digit } ;
ident_start    = ? any Unicode alphabetic character ? | "_" ;
digit          = "0" .. "9" ;
hex_digit      = digit | "a" .. "f" | "A" .. "F" ;
```




## Expressions

All expressions
may be prefixed with a type hint,
`: type / expr`.

Expressions are typechecked
with a simple bidirectional discipline,
drawing their expected type from a type hint
or synthesizing a type directly.
Most expressions synthesize types unambiguously.
Type hints are sometimes required, in predictable positions.
For many uses datalit expressions are checked
against an external type context.

```datalove
// A single top-level type hint.
: {
  name: string,
  version: (u32, u32, u32),
  tag: (bool,),
  counts: %{string = int},
} / {
  name = "datalove",
  version = (0, 1, 0),
  tag = (true,),
  counts = %{ "a" = 1, "b" = 2, "c" = 3 },
}
```


```datalove
// Embedded type hints.
{
  name = : string / "datalove",
  limits = {
    low = 1,
    // A deeply-embedded hint.
    high = : u32 / 3,
  },
  kind = : enum {
    atom ProcessData,
    term Custom string,
  } / enum {
    term Custom "experiment",
  },
}
```

Types are generally spelled the same way as their values,
but with the type sigil, `:`, replaced with the value-assignment sigil, `=`.

```datalove
: {
  retries: int,
  timeout: f32,
} / {
  retries = 3,
  timeout = 30.0,
}
```

A hint covers the one expression after its `/`.
An expression carries at most one hint;
`: u32 / : u32 / 1` does not parse.
Parentheses group without changing meaning:
`(1)` is `1`, and `(: u32 / 1)` is `: u32 / 1`.

A type hint may not name a type beyond the built-ins:
datalit has no type aliases,
so any bare name in a type is an error.
Collections are written with their sigils,
and `list`, `map`, `set`, `table` and `tensor` are not types.

A datalit document is a single expression.
Nothing but whitespace may follow it.


### EBNF · Expressions

```ebnf
datalit        = ws, full_expr, ws ;
full_expr      = [ type_hint, ws ], expr ;
type_hint      = ":", ws, type, ws, "/" ;
type           = primitive_type
               | tuple_type
               | struct_type
               | option_type
               | result_type
               | list_type
               | map_type
               | set_type
               | table_type
               | tensor_type
               | atom_type
               | term_type
               | enum_type
               | dynamic_type ;
expr           = primitive_lit
               | group_expr
               | tuple_expr
               | struct_expr
               | option_expr
               | result_expr
               | list_expr
               | map_expr
               | set_expr
               | table_expr
               | tensor_expr
               | atom_expr
               | term_expr
               | enum_expr
               | dynamic_expr ;
group_expr     = "(", ws, full_expr, ws, ")" ;
```




## Primitive Types

| Type              | Literal          |
|-------------------|------------------|
| `bool`            | `true`, `false`  |
| `int`             | `42`             |
| `f64`             | `3.14`, `1.0e10` |
| `f32`             | `: f32 / 3.14`   |
| `string`          | `"hello"`        |
| `u8` .. `u64`     | `: u32 / 42`     |
| `i8` .. `i64`     | `: i32 / -1`     |
| `index`           | `: index / 0`    |
| `offset`          | `: offset / 0`   |

`index` is an unsigned integer
representing the addressable size of collection types,
`offset` is the signed version of the same size,
both 32-bit by default, 64-bit by compile-time option.
They are distinct types from the fixed-width integers of the same size.


### Integers

Bare integer literals synthesize as `int`, the big integer type,
which has no range limit.
Fixed-width integers require a type hint or checking context,
and a literal out of the range of the type it checks against is an error.

Underscores may group digits and say nothing about the value.
A separator goes between two digits:
`1_000_000` is an integer, `1_` is an error, and `_1` is an identifier.

A sign is written glued to the digits: `-1`.
There is no `+` sign.
`-0` is zero.


### Floats

A float literal is written with a point, an exponent, or both:
`42.0`, `4.2e1`, `1e-7`, `6.022E23`.
Digits are required on both sides of a point, so `1.` and `.5` are errors.
The exponent marker is `e` or `E`, and its sign is optional.
Separators group digits in any run: `1_0.000_1e1_0`.

Float literals synthesize `f64`,
and check against `f32` or `f64`.
An integer literal does not check against a float type,
nor a float literal against an integer type.

A decimal literal becomes the float nearest its exact value,
ties to even,
converted directly to the target width.
A literal too large for the target width becomes infinity:
`: f32 / 1e300` is positive infinity.
`-0.0` is negative zero.


### Hex literals

Integers and floats can be created from hex literals,
written `0x` or `0X` and hex digits of either case,
which may be grouped with underscores.
Hex literals synthesize `int`, but check
as `int`, unsigned fixed ints, `index`,
or floats if provided a type hint.
Hex literals do not check against signed fixed-width types or `offset`.
A hex literal takes no sign: `-0x10` is an error.
Write a negative number in decimal.

A hex literal checked against a float type is its IEEE 754 bit pattern,
and must fit in the float's width: 32 bits for `f32`, 64 for `f64`.
This is the only way to write bit-exact floats,
NaNs and infinity.

```datalove
: {
  foo: int,
  bar: u32,
  baz: f32,
  inf: f64,
} / {
  foo = 0x01,
  bar = 0x02,
  baz = 0x3F80_0000,         // 1.0
  inf = 0x7FF0000000000000,
}
```


### Strings

A string literal is written between double quotes,
and holds any characters but an unescaped `"` or `\`,
including literal newlines.
The value is a sequence of Unicode scalar values.
No normalization is performed.

| Escape      | Character                    |
|-------------|------------------------------|
| `\"`        | double quote                 |
| `\\`        | backslash                    |
| `\n`        | line feed                    |
| `\r`        | carriage return              |
| `\t`        | tab                          |
| `\0`        | NUL                          |
| `\u{H..}`   | Unicode scalar value, 1 to 6 hex digits |

Any other escape is an error,
as is a `\u{..}` naming a surrogate or a value above `10FFFF`.


### EBNF · Primitive types

```ebnf
primitive_type = "bool"
               | "u8" | "u16" | "u32" | "u64"
               | "i8" | "i16" | "i32" | "i64"
               | "index" | "offset"
               | "f32" | "f64"
               | "int" | "string" ;

primitive_lit  = bool_lit | numeric_lit | string_lit ;
bool_lit       = "true" | "false" ;
numeric_lit    = float_lit | hex_lit | int_lit ;
int_lit        = [ "-" ], digit_run ;
float_lit      = [ "-" ], digit_run,
                 ( ".", digit_run, [ exponent ] | exponent ) ;
exponent       = ( "e" | "E" ), [ "+" | "-" ], digit_run ;
hex_lit        = "0", ( "x" | "X" ), hex_run ;
digit_run      = digit, [ { digit | "_" }, digit ] ;
hex_run        = hex_digit, [ { hex_digit | "_" }, hex_digit ] ;

string_lit     = '"', { string_char }, '"' ;
string_char    = escape_seq | ? any char except '"' and '\' ? ;
escape_seq     = "\", ( '"' | "\" | "n" | "r" | "t" | "0"
                      | "u{", hex_digit, { hex_digit }, "}" ) ;
```

A numeric literal contains no whitespace or comments.




## Collections

| Name   | Type                             | Literal                          |
|--------|----------------------------------|----------------------------------|
| list   | `[T]`                            | `[1, 2, 3]`                      |
| map    | `%{ K = V }`                     | `%{ 0 = 5, 1 = 2 }`              |
| set    | `#{ K }`                         | `#{ 1, 2, 3 }`                   |
| table  | `{\| col1: T1, col2: T2 \|}`     | `{\| col1, col2; 1, 2; 3, 4 \|}` |
| tensor | `[\|T, N\|]`                     | `[\| 1 2 3, 4 5 6 \|]`           |

Empty collections synthesize with unit element types
(`[()]`, `#{()}`, `%{() = ()}`),
but check against any element type:
`: [u32] / []` is valid.

Lists, maps and sets accept a trailing comma.


### List

An ordered sequence of homogeneous elements.

```datalove
[1, 2, 3]                      // [int]
: [u32] / [1, 2, 3]            // [u32] via type hint
[]                             // [()], empty list
: [string] / []                // [string], empty with hint
```

All elements must have the same type.
Without an expected type,
each element synthesizes its own type
and all must agree with the first:
`[: u8 / 1, 2]` is an error, since `2` synthesizes `int`.
Use a type hint on the list for fixed-width element types.


### Map

A key-value mapping, ordered by key.

```datalove
%{ "a" = 1, "b" = 2 }          // %{string = int}
: %{u32 = string} / %{ 1 = "x", 2 = "y" }
%{}                            // %{() = ()}, empty map
```

Entries use `=` to separate keys from values,
same as struct field assignment.
All keys must have the same type,
and all values must have the same type.
Any type may be a key.

A map's entries are kept in the [total ordering] of their keys,
not in the order they were written:
`%{ 2 = 0, 1 = 0 }` and `%{ 1 = 0, 2 = 0 }` are the same map.
A key written more than once keeps the last value written for it.


### Set

A collection of unique elements, ordered by value.

```datalove
#{ 1, 2, 3 }                   // #{int}
: #{u32} / #{ 10, 20, 30 }
#{}                            // #{()}, empty set
```

All elements must have the same type.
Any type may be an element.
A set's elements are kept in their [total ordering],
and an element written more than once is held once.


### Table

A data structure with named, typed columns.

```datalove
: {| x: int, y: string |} / {|
  x, y
  1, "a"
  2, "b"
|}
```

The first row names the columns;
subsequent rows provide data.
Rows are delimited by newlines or semicolons.

```datalove
: {| x: int, y: int |} / {| x, y; 1, 2; 3, 4 |}
```

Table literals always require a type hint &mdash;
they cannot synthesize a type.
Column names in the literal must match the type hint in order.
Each column is named by exactly one identifier,
and no two columns share a name.
Each data row must have exactly as many values as there are columns.
A table with no rows is written with just its header row.

A semicolon is written to go between two rows,
so one with nothing before it is an error,
as is a comma with no column before it:
`{| x, y;; 1, 2 |}` and `{| x,, y |}` do not parse.
A newline is not written to separate anything in particular,
so blank lines are free.
A semicolon or comma at the very end
closes the row or value before it and is fine.


### Tensor

A multi-dimensional array with fixed shape,
typed by element type and rank.

```datalove
[| 1 2 3 |]                    // 1D, shape [3]
[| 1 2 3, 4 5 6 |]             // 2D, shape [2, 3]
[| 1 2, 3 4,, 5 6, 7 8 |]      // 3D, shape [2, 2, 2]
: [|u32, 2|] / [| 1 2, 3 4 |]  // typed: element u32, rank 2
[| |]                          // empty tensor, rank 1, shape [0]
```

Shape is inferred from the multi-comma structure:
spaces separate elements along the innermost axis,
`,` separates rows (2nd axis),
`,,` separates slabs (3rd axis),
`,,,` separates blocks (4th axis), and so on.
The rank is one more than the longest run of commas written.
Whitespace between the commas of a run does not break it:
`, ,` is `,,`.
Every group along an axis must have the same shape,
so the element count equals the product of the shape dimensions.

Higher-dimensional tensors benefit from multiline layout,
using blank lines to visually separate the higher axes:

```datalove
// 3D tensor with shape [2, 3, 3]
: [|f64, 3|] / [|
  1.0 0.0 0.0,
  0.0 1.0 0.0,
  0.0 0.0 1.0,,

  2.0 0.0 0.0,
  0.0 2.0 0.0,
  0.0 0.0 2.0,
|]
```

When the outermost dimension is 1,
the highest comma level never appears as a separator.
Trailing commas preserve rank in this case:
`[| 1 2 3, |]` is a rank-2 tensor with shape [1, 3],
not a rank-1 tensor with shape [3].
The number of trailing commas equals rank minus one.

Trailing is the only place a separator may have nothing beside it.
A comma with nothing before it separates nothing,
so `[| ,1 2 |]` does not parse.
Blank lines are whitespace here and mean nothing to the shape,
which is what lets the layout above breathe.

The type specifies element type and rank: `[|T, N|]`.
Shape is not part of the type &mdash;
two tensors of the same element type and rank
but different shapes have the same type.
All elements must have the same type.


### EBNF · Collections

```ebnf
list_type      = "[", ws, type, ws, "]" ;
map_type       = "%{", ws, type, ws, "=", ws, type, ws, "}" ;
set_type       = "#{", ws, type, ws, "}" ;
table_type     = "{|", ws, [ type_field_list ], ws, "|}" ;
tensor_type    = "[|", ws, type, ws, ",", ws, digit, { digit }, ws, "|]" ;

list_expr      = "[", ws, [ expr_list ], ws, "]" ;
map_expr       = "%{", ws, [ entry_list ], ws, "}" ;
set_expr       = "#{", ws, [ expr_list ], ws, "}" ;
table_expr     = "{|", ws, table_header, table_rows, ws, "|}" ;
tensor_expr    = "[|", ws, [ tensor_body ], ws, "|]" ;

expr_list      = full_expr, { ws, ",", ws, full_expr }, [ ws, "," ] ;
entry_list     = entry, { ws, ",", ws, entry }, [ ws, "," ] ;
entry          = full_expr, ws, "=", ws, full_expr ;

table_header   = ident, { ws, ",", ws, ident }, [ ws, "," ], [ row_sep ] ;
table_rows     = { ws, table_row } ;
table_row      = full_expr, { ws, ",", ws, full_expr }, [ ws, "," ], [ row_sep ] ;
row_sep        = ";" | newline ;

tensor_body    = tensor_row, { ws, comma_run, ws, tensor_row },
                 [ ws, comma_run ] ;
tensor_row     = full_expr, { ws, full_expr } ;
comma_run      = ",", { ws, "," } ;
```

Within a table, `ws` does not include newlines, which are `row_sep`.
The grammar does not express the tensor shape rules above.




## Aggregates

| Name    | Type                          | Literal                       |
|---------|-------------------------------|-------------------------------|
| unit    | `()`                          | `()`                          |
| 1-tuple | `(T1,)`                       | `(true,)`                     |
| n-tuple | `(T1, T2)`                    | `(true, 42)`                  |
| struct  | `{ x: T1, y: T2 }`            | `{ x = 1, y = 2 }`            |
| option  | `?T`                          | `some 1` <br> `none`          |
| result  | `!T`                          | `ok 1` <br> `er error 2`      |
| atom    | `atom Foo`                    | `atom Foo`                    |
| term    | `term Foo T`                  | `term Foo 1`                  |
| enum    | `enum { atom A, term B T }`   | `enum { atom A }`             |

Tuples and structs accept a trailing comma.


### Tuple

An ordered sequence of heterogeneous values.
Two tuples are the same type
if they have the same length and element types in the same order.

```datalove
()                             // unit: the zero-element tuple
(true,)                        // 1-tuple (trailing comma required)
(true, 42)                     // (bool, int)
(1, "hello", 3.14)             // (int, string, f64)
: (u32, i32) / (1, -1)         // with type hint
```

Unit `()` is both a type and a value.
A 1-tuple requires a trailing comma to distinguish it
from a parenthesized expression.
Each element synthesizes its type independently:
`(true, 42, 3.14)` synthesizes as `(bool, int, f64)`.


### Struct

A collection of named fields.
Two structs are the same type
if they have the same field names, types, and order.

```datalove
{ x = 1, y = 2 }               // {x: int, y: int}
: { x: u32, y: f32 } / { x = 1, y = 2.0 }
{}                             // the empty struct
```

Types use `:` between field names and types,
while expressions use `=` between field names and values.
Field order matters &mdash;
`{x: u32, y: bool}` and `{y: bool, x: u32}` are different types.
Checked against a struct type,
a struct literal must give every field, in the type's order.
Field names are unique within a struct.
Each field value synthesizes its type independently.


### Option

An optional value: either present or absent.
The type is written `?T` with a prefix `?`.

```datalove
some 42                        // ?int
some "hello"                   // ?string
: ?u32 / some 1
: ?u32 / none
: ??u32 / some none
```

`some` wraps a value; `none` represents absence.
`some e` synthesizes as `?T` where `T` is the type of `e`.
`none` cannot synthesize a type &mdash;
it requires a type hint or checking context.
A value does not check against an option type without `some`:
`: ?u32 / 1` is an error.


### Result

A success-or-failure value.
The type is written `!T` with a prefix `!`.
The failure side always holds an `error`.

```datalove
ok 42                          // !int
: !u32 / ok 1
: !u32 / er error "oops"
```

`ok` wraps a success value; `er` wraps an error.
`ok e` synthesizes as `!T` where `T` is the type of `e`.
`er` cannot synthesize a type &mdash;
it requires a type hint or checking context.
The payload of `er` is checked against `error`,
so it is written as an `error` expression, `er error "oops"`,
and is the error the result holds.
An `error` is not a result by itself:
`: !u32 / error "oops"` is an error.


### Atom

A named unit type with no payload.
An atom is both a type and a value &mdash;
the syntax is the same in both positions.

```datalove
atom Red                       // type and value
atom Blue
```

Two atoms are the same type if they have the same name.


### Term

A named type carrying a typed payload.

```datalove
term Foo 42                    // term Foo int (value)
term Bar "hello"               // term Bar string (value)
```

In type position, the payload is a type: `term Foo int`.
In expression position, the payload is a value: `term Foo 42`.
Two terms are the same type
if they have the same name and the same payload type.


### Enum

A closed union of atom and term variants.

```datalove
: enum { atom Red, atom Blue, term Custom string } /
  enum { atom Red }

: enum { atom Red, atom Blue, term Custom string } /
  enum { term Custom "hello" }
```

The type lists all variants;
the expression provides a single variant,
which must be one the type lists.
Variant names are unique within an enum.
Enum type equivalence compares variants by name,
regardless of the order they are declared.
Enum literals require a type hint.

An atom or term checks against any enum that lists it,
so the `enum { }` around a variant may be left off
where the enum type is expected:

```datalove
: [enum { atom Red, atom Blue, term Custom string }] /
  [atom Red, term Custom "hello", enum { atom Blue }]
```

Enum variants are held in name order,
which is the order values of an enum compare in.


### EBNF · Aggregates

```ebnf
tuple_type     = "(", ws, [ type_list ], ws, ")" ;
struct_type    = "{", ws, [ type_field_list ], ws, "}" ;
option_type    = "?", type ;
result_type    = "!", type ;
atom_type      = "atom", ws, ident ;
term_type      = "term", ws, ident, ws, type ;
enum_type      = "enum", ws, "{", ws, enum_variant_list, ws, "}" ;

tuple_expr     = "(", ws, ")"
               | "(", ws, full_expr, ws, ",", ws, ")"
               | "(", ws, full_expr, ws, ",", ws, expr_list, ws, ")" ;
struct_expr    = "{", ws, [ field_list ], ws, "}" ;
option_expr    = "none" | "some", ws, full_expr ;
result_expr    = ( "ok" | "er" ), ws, full_expr ;
atom_expr      = atom_type ;
term_expr      = "term", ws, ident, ws, full_expr ;
enum_expr      = "enum", ws, "{", ws, ( atom_expr | term_expr ), ws, "}" ;

type_list      = type, { ws, ",", ws, type }, [ ws, "," ] ;
type_field_list= type_field, { ws, ",", ws, type_field }, [ ws, "," ] ;
type_field     = ident, ws, ":", ws, type ;
field_list     = field, { ws, ",", ws, field }, [ ws, "," ] ;
field          = ident, ws, "=", ws, full_expr ;
enum_variant_list = enum_variant, { ws, ",", ws, enum_variant },
                    [ ws, "," ] ;
enum_variant   = atom_type | term_type ;
```




## Dynamic Types

| Type                          | Literal                        |
|-------------------------------|--------------------------------|
| `data`                        | `data 1` <br> `data : u32 / 2` |
| `error`                       | `error 1` <br> `error "oops"`  |

A `data` value holds a value of any type
together with that type.
`data e` synthesizes `data`,
and its payload `e` must synthesize a type of its own,
which is the type the `data` carries:
`data 1` holds an `int`, `data : u32 / 1` a `u32`,
and `data none` is an error.

An `error` is the same thing under a different type,
used for the failure side of results.
`error e` synthesizes `error`,
and becomes a result's failure under `er`: `er error e`.

A value is only a `data` or `error` when written as one:
`: data / 5` is an error, and is written `data 5`.


### EBNF · Dynamic types

```ebnf
dynamic_type   = "data" | "error" ;
dynamic_expr   = ( "data" | "error" ), ws, full_expr ;
```




## Types

### Type equivalence

All datalit types are structural:
two types are the same when they are spelled the same,
with these rules for the aggregates.

| Type            | Same type when                                            |
|-----------------|-----------------------------------------------------------|
| primitives      | same name; `index` is not `u32` or `u64`, whatever its width |
| tuple           | same length, same element types in order                  |
| struct          | same field names and types, in the same order             |
| table           | same column names and types, in the same order            |
| list, set       | same element type                                         |
| map             | same key and value types                                  |
| tensor          | same element type and rank; shape does not count          |
| option, result  | same inner type                                           |
| atom            | same name                                                 |
| term            | same name and payload type                                |
| enum            | same set of variants, regardless of declared order        |


### Synthesis and checking

Every expression either synthesizes a type from itself,
or is checked against an expected type
from a type hint or an enclosing expression.
An expression that synthesizes can always be checked:
it checks against a type if it synthesizes that type.
There are no implicit conversions:
an integer does not become a float, a value does not become an option,
and a `u8` does not become a `u32`.
An expression with a hint has the hinted type,
and checks only against that type:
`: {a: u32} / {a = : u8 / 1}` is an error.

| Expression      | Synthesizes                  | Also checks against                    |
|-----------------|------------------------------|----------------------------------------|
| `true`, `false` | `bool`                       |                                        |
| integer         | `int`                        | fixed-width ints, `index`, `offset`, if in range |
| float           | `f64`                        | `f32`                                  |
| hex             | `int`                        | unsigned ints, `index`, `f32`, `f64`, if it fits |
| string          | `string`                     |                                        |
| tuple           | tuple of element types       | tuple of same length, elementwise      |
| struct          | struct of field types        | struct of same fields in order, fieldwise |
| list, set       | of the first element's type  | any element type, elementwise          |
| empty list, set | element type `()`            | any element type                       |
| map             | of the first entry's types   | any key and value types, entrywise     |
| tensor          | first element's type, rank as written | same rank, elementwise        |
| table           | &mdash;                      | same columns in order, cellwise        |
| `some e`        | `?T` where `e` synthesizes `T` | `?T`, checking `e` against `T`       |
| `none`          | &mdash;                      | any `?T`                               |
| `ok e`          | `!T` where `e` synthesizes `T` | `!T`, checking `e` against `T`       |
| `er e`          | &mdash;                      | any `!T`, checking `e` against `error` |
| `data e`        | `data`                       |                                        |
| `error e`       | `error`                      |                                        |
| `atom A`        | `atom A`                     | an enum listing `atom A`               |
| `term A e`      | `term A T` where `e` synthesizes `T` | `term A T`, or an enum listing `term A T`, checking `e` against `T` |
| `enum { v }`    | &mdash;                      | an enum listing `v`                    |

So a type hint or context is required for
`none`, `er`, tables, enum literals,
fixed-width numbers and `f32`,
and empty collections of anything but `()`.




## Comparison and total ordering

Every datalit type has a total ordering,
so any value can be a map key or set element,
and any two values of the same type can be compared.
Values of different types are not compared.

The ordering is the one the runtime uses to keep maps and sets,
so it is also what decides whether two keys are the same key.

| Type            | Order                                                          |
|-----------------|----------------------------------------------------------------|
| `bool`          | `false` before `true`                                          |
| integers        | numeric                                                        |
| floats          | IEEE 754 `totalOrder`: `-NaN`, `-inf`, negatives, `-0.0`, `0.0`, positives, `inf`, `NaN` |
| `string`        | lexicographic by UTF-8 byte, which is code point order         |
| tuple, struct   | lexicographic by element, in declared order                    |
| list            | lexicographic by element; a prefix comes first                 |
| set             | lexicographic by element, in element order; a prefix comes first |
| map             | lexicographic by entry in key order, key then value; a prefix comes first |
| table           | lexicographic by row, each row by column in declared order; a prefix comes first |
| tensor          | by shape, lexicographically, then by element in row-major order |
| option          | `none` first, then `some` by payload                           |
| result          | `er` first, then `ok`; each by payload                         |
| `data`, `error` | by the carried type, then by value                             |
| atom            | its one value                                                  |
| term            | by payload                                                     |
| enum            | by variant name, then by payload                               |

Floats are totally ordered by their bit patterns,
so `-0.0` and `0.0` are different values and different keys,
and a NaN is a value like any other,
equal to itself and to no other NaN bit pattern.
`#{ 0.0, -0.0 }` has two elements.

The order among the types a `data` or `error` may carry
is arbitrary and fixed by the implementation.
Enum variant names order by UTF-8 byte.

The runtime has a second, IEEE equality,
which differs from the total ordering only for floats:
under it `-0.0` equals `0.0` and no NaN equals anything.
It is the equality datafun computes with,
and is not used for keys.




## Canonical forms


Datalove literals has no canonical serialized form
as might be used for consistent hashing.

There is no canonical form for floats.

Required type-hint insertion points are unknown.

Will be revisited.

Two printers exist today.
The syntax printer, `lit-pretty`, reprints a parsed expression
with the literals spelled as written, including hex,
and entries in the order written.
The value printer prints a runtime value:
maps and sets in their total ordering,
integers in decimal,
and floats in their shortest round-tripping form,
positional from `1e-5` up to `1e16` and with an exponent otherwise,
always with a point or exponent.
It prints no type hints.




## Discrepancies, unknowns and bugs

This section compares the spec above with
`botdocs/botspec.md`, `botdocs/datalit-ebnf.md`, `botdocs/datalit-typing-rules.md`,
and the implementation in `crates/datalove-datalit`
as of this writing.
Behaviour was checked with the `lit-tycheck`, `lit-pretty` and `lit-op` commands
and against the parser's own diagnostics.

### Not implemented

- **`int` is not arbitrary precision when values are built.**
  The checker accepts any integer for `int`,
  but instantiation parses decimal through `i128` and hex through `u128`,
  so `170141183460469231731687303715884105728` fails with
  "number too large to fit in target type".

### Bugs

- **Empty tensors of rank above 1 cannot be written.**
  `[| |]` is always rank 1 with shape [0],
  and `[| , |]` does not parse,
  so `: [|u32, 2|] / ...` has no empty value.
  A rank-0 type `[|u32, 0|]` parses but has no literal.

### Divergences from the botspec

- **`tuple ( ... )` types.**
  The type parser accepts `tuple (u32, u32)` as a spelling of `(u32, u32)`.
  Nothing documents it.
- **Payload syntax.**
  The botspec says `some`, `ok`, `er`, `data` and `error`
  take a primary expression in datafun.
  In datalit they take a `full_expr`, including a type hint:
  `some : u32 / 1`.
  Datalit has no binary operators, so the two agree on every datalit input.

### Unknowns

- Whether a float literal that overflows to infinity,
  or underflows to zero, should be an error.
  It is silently rounded today.
- Whether duplicate map keys and set elements should be errors
  rather than resolved last-wins.
- Whether field and column names may be keywords.
  They may today.
- Whether a byte order mark is allowed.
  Nothing strips one.
- The value printer writes non-finite floats as `nan`, `inf` and `-inf`,
  which do not parse.
  Printed fixed-width values have no hints,
  so they read back as `int` and `f64`.
  Round-tripping through the value printer is only exact
  for values whose types synthesize.

### Open questions

- Should empty collections synthesize?
- What are the rules for printing disambiguating type hints?
- What are the rules for printing disambiguating floats, etc?
- How can we make tensors and tables accept trailing commas?
- Should dupe map/set keys be an error?
- Why can't tables synthesize?
- Why are semicolons special inside tables?
- Should we glue commas in tensors?
- re tables: "the grammar does not express the tensor shape rules above"?
- Enum accept trailing comma?
- Should grouping parens actually be allowed? Are they needed?
- Should structs require identical field order?
- Should atom/term actually check against enums?
- Result types shouldn't accept data payloads.
- Should error actually check against results?
- What does "enum variants are held in name order" mean practically in datalit?
- result and error check against _any_ !T?
- Need to think harder about float total order and ergonomics.
- "order among the types a data or error may carry is arbitrary"??




[total ordering]: #user-content-comparison-and-total-ordering
