# Sigil-logic

Datalove reserves some symbols to strongly mean one thing.

| When you see | it means                       |
|--------------|--------------------------------|
| `:`          | type                           |
| `=`          | value binding                  |
| `->`         | maps to (map key to value)     |
| `?`          | option                         |
| `!`          | result                         |
| `@`          | adapt (clone / widen / coerce) |
| `;`          | statement break / newline      |

`:` and `=` only ever follow a name:
`:` gives the name's type and `=` its value,
so a struct's type is `{ x: T }` and its value `{ x = e }`.
Where there is no name, a type and its value are written alike:
`[T]` and `[e]`, `(T, U)` and `(e, f)`.
A map's key is a value rather than a name,
so a map takes neither name sigil, and uses `->` in both:
`%{string -> u32}` and `%{"a" -> 1}`.

Likewise some bracket pairs are strongly suggestive.

| When you see | it means                        |
|--------------|---------------------------------|
| `<` … `>`    | generics                        |
| `{` … `}`    | struct-like, structs and tables |
| `[` … `]`    | array-like, lists and tensors   |
