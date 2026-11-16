# Sigil-logic

Datalove reserves some symbols to strongly mean one thing.

| When you see | it means                       |
|--------------|--------------------------------|
| `:`          | annotate: has type             |
| `=`          | define: stands for             |
| `->`         | maps to (map key to value)     |
| `?`          | option                         |
| `!`          | result                         |
| `@`          | adapt (clone / widen / coerce) |
| `;`          | statement break / newline      |

`:` annotates: what follows it is the type of the name before it,
`let x: u32`, or of the expression a hint precedes, `: u32 / 1`.
`=` defines: what follows it is what a name stands for,
a value for `let`, `const` or a struct field,
and a type for `type`.
So every declaration reads _keyword name: annotation = definition_:

```datalove
let x: u32 = 1
const LIMIT: u32 = 10
type Point = { x: f32, y: f32 }
```

A struct field is annotated in the struct's type and defined in its value,
`{ x: T }` and `{ x = e }`.
Where nothing is annotated or defined, a type and its value are written alike:
`[T]` and `[e]`, `(T, U)` and `(e, f)`.
A map's key is a value rather than a name, neither annotated nor defined,
so a map uses `->` in both:
`%{string -> u32}` and `%{"a" -> 1}`.

Likewise some bracket pairs are strongly suggestive.

| When you see | it means                                      |
|--------------|-----------------------------------------------|
| `(` … `)`    | positional and fixed: tuples, params, calls   |
| `[` … `]`    | positional and growable: lists                |
| `{` … `}`    | keyed: structs and enums, by name             |
| `<` … `>`    | generics                                      |

`[ ]` holds members found by position,
and `{ }` members found by name or key rather than position.
A struct's fields and an enum's variants are found by name,
and a sigil before the `{` says what else keys it:

| When you see | it means                                      |
|--------------|-----------------------------------------------|
| `%{` … `}`   | map: members found by key, `%{k -> v}`        |
| `#{` … `}`   | set: members found by themselves, `#{e}`      |

Earmuffs, a `|` inside the bracket, make the many-dimensional form
of what the bracket holds:

| When you see  | it means                                |
|---------------|-----------------------------------------|
| `[\|` … `\|]` | tensor: a list along several axes       |
| `{\|` … `\|}` | table: rows of a struct, kept by column |
| `(\|` … `\|)` | reserved                                |
| `<\|` … `\|>` | reserved                                |
