# Datalove Literals

Datalove literals is
a typed declarative expression language for
serializing, storing and transmitting
common data types.




## Datalove literal expressions

All expressions
may be prefixed with a type hint,
`: type / expr`.

Datalove literals are typechecked
with a simple bidirectional discipline,
either checking against a type hint,
or synthesizing a type.
All Datalove literal expressions
synthesize some type in absence of type hints,
though in some cases it may be required
to use type hints to reliably produce a desired type.

```ebnf
datalit        = ws, full_expr, ws ;
full_expr      = [ type_hint ], expr ;
type_hint      = ":", ws, type, ws, "/" ;
expr           =
               | bool_lit
               | numeric_lit
               | string_lit
               | tuple_expr
               | struct_expr
               | option_expr
               | result_expr
               | list_expr
               | map_expr
               | set_expr
               | table_expr
               | tensor_expr
               | existential_expr ;
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




## Collections

| Name   | Type                             | Literal                          |
|--------|----------------------------------|----------------------------------|
| list   | `[T]`                            | `[1, 2, 3]`                      |
| map    | `%{ K = V }`                     | `%{ 0 = 5, 1 = 2 }`              |
| set    | `#{ K }`                         | `#{ 1, 2, 3 }`                   |
| table  | `{\| col1: T1, col2: T2 \|}`     | `{\| col1, col2; 1, 2; 3, 4 \|}` |
| tensor | `[\|T, N\|]`                     | `[\| 1 2 3, 4 5 6 \|]`           |



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




## Dynamic Types

| Type                          | Literal                        |
|-------------------------------|--------------------------------|
| `data`                        | `data 1` <br> `data : u32 / 2` |
| `error`                       | `error 1` <br> `error "oops"`  |




## Datalove Literals grammar




## Datalove Literals typing rules

