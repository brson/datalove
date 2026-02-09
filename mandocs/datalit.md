# Datalove Literals

Datalove literals is
a typed declarative expression language for
serializing, storing and transmitting
common data types.

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




## Collections

| Type                             | Literal                          |
|----------------------------------|----------------------------------|
| `[T]`                            | `[1, 2, 3]`                      |
| `%{ K = V }`                     | `%{ 0 = 5, 1 = 2 }`              |
| `#{ K }`                         | `#{ 1, 2, 3 }`                   |
| `{\| col1: T1, col2: T2 \|}`     | `{\| col1, col2; 1, 2; 3, 4 \|}` |
| `[\|T, N\|]`                     | `[\| 1 2 3, 4 5 6 \|]`           |



## Aggregates

| Type                          | Literal                       |
|-------------------------------|-------------------------------|
| `()`                          | `()`                          |
| `(T1,)`                       | `(true,)`                     |
| `(T1, T2)`                    | `(true, 42)`                  |
| `{ x: T1, y: T2}`             | `{x = 1, y = 2}`              |
| `?T`                          | `some 1` <br> `none`          |
| `!T`                          | `ok 1` <br> `er 2`            |
| `atom Foo`                    | `atom Foo`                    |
| `term Foo T`                  | `term Foo 1`                  |
| `enum { atom A, term B T }`   | `enum { atom A }`             |




## Dynamic Types

| Type                          | Literal                       |
|-------------------------------|-------------------------------|
| `data`                        | `data 1` <br> `data : u32 / 2`   |
| `error`                       | `error 1` <br> `error : u32 / 2` |




## Datalove Literals grammar




## Datalove Literals typing rules

