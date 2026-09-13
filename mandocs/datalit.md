# Datalove Literals

Datalove literals is
a typed declarative expression language for
serializing, storing and transmitting
common data types.
We often refer to it as _datalit_,
and its types and datalit types.

```datalove
: {
  name: string,
  version: (u32, u32, u32),
  tag: (bool,),
  unit: (),
  enabled: bool,
  score: f64,
  flags: u32,
  offset: i32,
  tags: [string],
  counts: %{string = int},
  ids: #{int},
  matrix: [|f64, 2|],
  cube: [|int, 3|],
  metrics: {| name: string, value: f64 |},
  config: ?{ retries: u32, timeout: f64 },
  backup: ?string,
  status: !string,
  failure: !u32,
  state: atom Ready,
  event: term Click (int, int),
  kind: enum { atom Normal, atom Debug, term Custom string },
  payload: data,
} / {
  name = "datalove",
  version = (0, 1, 0),
  tag = (true,),
  unit = (),
  enabled = true,
  score = 99.5,
  flags = 0xFF,
  offset = -1,
  tags = ["fast", "typed", "portable"],
  counts = %{ "a" = 1, "b" = 2, "c" = 3 },
  ids = #{ 10, 20, 30 },
  matrix = [| 1.0 0.0, 0.0 1.0 |],
  cube = [| 1 2, 3 4,, 5 6, 7 8 |],
  metrics = {| name, value; "latency", 0.5; "throughput", 1000.0 |},
  config = some { retries = 3, timeout = 30.0 },
  backup = none,
  status = ok "healthy",
  failure = error "oops",
  state = atom Ready,
  event = term Click (100, 200),
  kind = enum { term Custom "experiment" },
  payload = data [1, 2, 3],
}
```


## Expressions

All expressions
may be prefixed with a type hint,
`: type / expr`.

Expressions are typechecked
with a simple bidirectional discipline,
either checking against a type hint,
or synthesizing a type.
All expressions
synthesize some type in absence of type hints
(ideally, not currently true).
For many uses datalit expressions are checked
against an external type context.

### EBNF · Expressions

```ebnf
datalit        = ws, full_expr, ws ;
full_expr      = [ type_hint ], expr ;
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
               | tuple_expr
               | struct_expr
               | option_expr
               | result_expr
               | list_expr
               | map_expr
               | set_expr
               | table_expr
               | tensor_expr
               | dynamic_expr ;
```




## Primitive Types

| Type              | Literal          |
|-------------------|------------------|
| `bool`            | `true`, `false`  |
| `int`             | `42`             |
| `f32`, `f64`      | `3.14`           |
| `string`          | `"hello"`        |
| `u8` .. `u64`     | `: u32 / 42`     |
| `i8` .. `i64`     | `: i32 / -1`     |
| `index`           | `: index / 0`    |
| `offset`          | `: offset / 0`   |

Bare integer literals synthesize as `int`, the big integer type.
Fixed-width integers require a type hint or checking context.

`index` is an unsigned integer
representing the addressable size of collection types,
`offset` is the signed version of the same size,
both 32-bit by default.

Integers and floats can be created from hex literals.
Hex literals synthesize `int`, but check
as unsigned fixed ints or floats if provided a type hint.
This is the only way to write bit-exact floats,
NaNs and infinity.

```datalove
: {
  foo: int,
  bar: u32,
  baz: f32,
} / {
  foo = 0x01,
  bar = 0x02,
  baz = 0x03000000, // float hex literals must have correct # digits
}
```

### EBNF · Primitive types

```ebnf
primitive_type = "bool"
               | "u8" | "u16" | "u32" | "u64"
               | "i8" | "i16" | "i32" | "i64"
               | "index" | "offset"
               | "f32" | "f64"
               | "int" | "string" ;

primitive_lit  = bool_lit | numeric_lit | string_lit
bool_lit       = "true" | "false" ;
numeric_lit    = int_lit | float_lit | hex_lit ;
int_lit        = [ "-" ], digit, { digit } ;
float_lit      = [ "-" ], digit, { digit }, ".", digit, { digit } ;
hex_lit        = [ "-" ], "0", ( "x" | "X" ), hex_digit, { hex_digit } ;
string_lit     = '"', { string_char }, '"' ;
```




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


### List

An ordered sequence of homogeneous elements.

```datalove
[1, 2, 3]                     // [int]
: [u32] / [1, 2, 3]           // [u32] via type hint
[]                             // [()], empty list
: [string] / []                // [string], empty with hint
```

All elements must have the same type.
The first element's type determines the expected type for the rest.
Bare integer elements synthesize as `int;`
use a type hint for fixed-width element types.


### Map

An ordered key-value mapping.

```datalove
%{ "a" = 1, "b" = 2 }         // %{string = int}
: %{u32 = string} / %{ 1 = "x", 2 = "y" }
%{}                            // %{() = ()}, empty map
```

Entries use `=` to separate keys from values,
same as struct field assignment.
All keys must have the same type,
and all values must have the same type.
All keys have a [total ordering].


### Set

A collection of unique elements.

```datalove
#{ 1, 2, 3 }                  // #{int}
: #{u32} / #{ 10, 20, 30 }
#{}                            // #{()}, empty set
```

All elements must have the same type.
All keys have a [total ordering].


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
Each data row must have exactly as many values as there are columns.

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
[| |]                          // empty tensor, shape [0]
```

Shape is inferred from the multi-comma structure:
spaces separate elements along the innermost axis,
`,` separates rows (2nd axis),
`,,` separates slabs (3rd axis),
`,,,` separates blocks (4th axis), and so on.
The element count must equal the product of the shape dimensions.

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


### EBNF - Collections

```ebnf
list_type      = "[", ws, type, ws, "]" ;
map_type       = "%{", ws, type, ws, "=", ws, type, ws, "}" ;
set_type       = "#{", ws, type, ws, "}" ;
table_type     = "{|", ws, type_field_list, ws, "|}" ;
tensor_type    = "[|", ws, type, ws, ",", ws, int_lit, ws, "|]" ;

list_expr      = "[", ws, [ expr_list ], ws, "]" ;
map_expr       = "%{", ws, [ entry_list ], ws, "}" ;
set_expr       = "#{", ws, [ expr_list ], ws, "}" ;
table_expr     = "{|", ws, table_header, table_rows, ws, "|}" ;
tensor_expr    = "[|", ws, [ tensor_body ], ws, "|]" ;

expr_list      = full_expr, { ws, ",", ws, full_expr }, [ ws, "," ] ;
entry_list     = entry, { ws, ",", ws, entry }, [ ws, "," ] ;
entry          = full_expr, ws, "=", ws, full_expr ;

table_header   = ident, { ws, ",", ws, ident }, row_sep ;
table_rows     = { ws, table_row } ;
table_row      = full_expr, { ws, ",", ws, full_expr }, [ row_sep ] ;
row_sep        = ";" | newline ;

tensor_body    = tensor_group, { multi_comma, ws, tensor_group },
                 [ multi_comma ] ;
tensor_group   = tensor_row, { ws, ",", ws, tensor_row } ;
tensor_row     = full_expr, { ws, full_expr } ;
multi_comma    = ",", ",", { "," } ;
```



## Aggregates

| Name    | Type                          | Literal                       |
|---------|-------------------------------|-------------------------------|
| unit    | `()`                          | `()`                          |
| 1-tuple | `(T1,)`                       | `(true,)`                     |
| n-tuple | `(T1, T2)`                    | `(true, 42)`                  |
| struct  | `{ x: T1, y: T2}`             | `{x = 1, y = 2}`              |
| option  | `?T`                          | `some 1` <br> `none`          |
| result  | `!T`                          | `ok 1` <br> `er 2`            |
| atom    | `atom Foo`                    | `atom Foo`                    |
| term    | `term Foo T`                  | `term Foo 1`                  |
| enum    | `enum { atom A, term B T }`   | `enum { atom A }`             |


### Tuple

An ordered sequence of heterogeneous values.
Two tuples are the same type
if they have the same length and element types in the same order.

```datalove
()                             // unit: the zero-element tuple
(true,)                        // 1-tuple (trailing comma required)
(true, 42)                     // (bool, int)
(1, "hello", 3.14)            // (int, string, f32)
: (u32, i32) / (1, -1)        // with type hint
```

Unit `()` is both a type and a value.
A 1-tuple requires a trailing comma to distinguish it
from a parenthesized expression.
Each element synthesizes its type independently:
`(true, 42, 3.14)` synthesizes as `(bool, int, f32)`.


### Struct

A collection of named fields.
Two structs are the same type
if they have the same field names, types, and order.

```datalove
{ x = 1, y = 2 }              // {x: int, y: int}
: { x: u32, y: f32 } / { x = 1, y = 2.0 }
```

Types use `:` between field names and types,
while expressions use `=` between field names and values.
Field order matters &mdash;
`{x: u32, y: bool}` and `{y: bool, x: u32}` are different types.
Each field value synthesizes its type independently.


### Option

An optional value: either present or absent.
The type is written `?T` with a prefix `?`.

```datalove
some 42                        // ?int
some "hello"                   // ?string
none                           // absent value (requires type context)
: ?u32 / some 1
: ?u32 / none
```

`some` wraps a value; `none` represents absence.
`some e` synthesizes as `?T` where `T` is the type of `e`.
`none` cannot synthesize a type &mdash;
it requires a type hint or checking context.


### Result

A success-or-failure value.
The type is written `!T` with a prefix `!`.

```datalove
ok 42                          // !int
er error "failed"              // result with error
: !u32 / ok 1
: !u32 / error "oops"
```

`ok` wraps a success value; `er` wraps an error.
`ok e` synthesizes as `!T` where `T` is the type of `e`.
`er` cannot synthesize a type &mdash;
it requires a type hint or checking context.
The payload of `er` must be an `error` or `data` expression.
An `error` expression can also check directly against `!T`,
acting as implicit error wrapping.


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

: enum { atom Red, atom Blue } /
  enum { term Custom "hello" }
```

The type lists all variants;
the expression provides a single variant.
Enum type equivalence compares variants by name,
regardless of the order they are declared.
Enum literals require a type hint.


### EBNF - Aggregates

```ebnf
tuple_type     = "(", ws, [ type_list ], ws, ")" ;
struct_type    = "{", ws, [ type_field_list ], ws, "}" ;
option_type    = "?", type ;
result_type    = "!", type ;
atom_type      = "atom", ws, ident ;
term_type      = "term", ws, ident, ws, type ;
enum_type      = "enum", ws, "{", ws, enum_variant_list, ws, "}" ;

tuple_expr     = "(", ws, [ expr_list ], ws, ")" ;
struct_expr    = "{", ws, [ field_list ], ws, "}" ;
option_expr    = "some", ws, full_expr ;
result_expr    = ( "ok" | "er" ), ws, full_expr ;

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




## Comparison and total ordering

todo




[total ordering]: #user-content-comparison-and-total-ordering