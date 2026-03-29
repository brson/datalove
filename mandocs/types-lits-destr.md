# Types, Literals, and Destructuring

Quick reference for type syntax, literal expression syntax,
and destructuring syntax for all datalove types.

Type syntax looks visually similar to literal syntax.
Destructuring syntax looks identical to literal syntax.
Curly braces `{ }` are for structish types,
square braces `[ ]` are for arrayish types, etc.


## Primitives

| Type              | Literal          | Destructuring    |
|-------------------|------------------|------------------|
| `bool`            | `true`, `false`  | n/a (copy)       |
| `int`             | `42`             | n/a (move)       |
| `f32`, `f64`      | `3.14`           | n/a (copy)       |
| `string`          | `"hello"`        | n/a (move)       |
| `u8` .. `u64`     | `: u32 / 42`     | n/a (copy)       |
| `i8` .. `i64`     | `: i32 / -1`     | n/a (copy)       |
| `index`           | `: index / 0`    | n/a (copy)       |
| `offset`          | `: offset / 0`   | n/a (copy)       |

Bare integer literals synthesize as `int`.
Fixed-width integers require a type hint or checking context.


## Collections

| Type               | Literal                       |
|--------------------|-------------------------------|
| `[T]`              | `[1, 2, 3]`                   |
| `⦇K ↦ V⦈`         | `⦇ 0 ↦ 5, 1 ↦ 2 ⦈`           |
| `⦃T⦄`              | `⦃ 1, 2, 3 ⦄`                |
| `⟦ col: T ⟧`       | `⟦ x, y; 1, 2; 3, 4 ⟧`       |
| `⟪T, N⟫`           | `⟪ 1 2 3, 4 5 6 ⟫`           |

No collection types support destructuring.

```datalove
// Lists
let a: [int] = [1, 2, 3]

// Maps
let a: ⦇int ↦ int⦈ = ⦇ 0 ↦ 5, 1 ↦ 2 ⦈

// Sets
let a: ⦃int⦄ = ⦃ 1, 2, 3 ⦄

// Tables
let a: ⟦ col1: int, col2: int ⟧ = ⟦
  x, y
  1, 2
  3, 4
⟧

// Tensors
let a: ⟪int, 2⟫ = ⟪
  1 2 3,
  4 5 6,
⟫
let b: ⟪int, 3⟫ = ⟪
  1 2 3,
  4 5 6,,
  1 2 3,
  4 5 6,,
⟫
```


## Aggregates

| Type                          | Literal                        | Destructuring                 |
|-------------------------------|--------------------------------|-------------------------------|
| `()`                          | `()`                           | `let ()`                      |
| `(T1,)`                       | `(true,)`                      | `let (a,)`                    |
| `(T1, T2)`                    | `(true, 42)`                   | `let (a, b)`                  |
| `{ x: T1, y: T2 }`           | `{ x = 1, y = 2 }`            | `let { x, y }`               |
| `?T`                          | `some 1` / `none`              | via `if` binding              |
| `!T`                          | `ok 1` / `er 2`               | via `if` binding              |
| `atom Foo`                    | `atom Foo`                     | `case atom Foo`               |
| `term Foo T`                  | `term Foo 1`                   | `case term Foo x`             |
| `enum { atom A, term B T }`   | `(atom A)@`                    | via `match`                   |

```datalove
// Tuples
let t: (bool, u32) = (true, 42)
let (a, b) = t

// Structs
let s: { x: f32, y: f32 } = { x = 1.0, y = 2.0 }
let { x, y } = s

// Option
let o: ?int = some 1

// Result
let r: !int = ok 1

// Atoms and terms
let a: atom Foo = atom Foo
let t: term Bar int = term Bar 1

// Enums -- use @ to coerce atom/term into enum type
type Shape: enum {
  atom Circle,
  term Rect (f32, f32),
}

let s: Shape = (atom Circle)@
let s: Shape = (term Rect (1.0, 2.0))@

// Data and error
let d: data = data 42
let d: data = data : u32 / 42
let e: error = error "oops"
```


## Destructuring sum types with `if` and `match`

Option and result types with `if` binding.

```datalove
let o: ?int = some 42

if o |value|
  debuglog value
end if

let r: !int = ok 42

if r |value|
  debuglog value
else |e|
  debuglog e
end if
```

Enums with `match`.

```datalove
type Shape: enum {
  atom Circle,
  term Rect (f32, f32),
  term Tri (f32, f32, f32),
}

let s: Shape = (term Rect (3.0, 4.0))@

match s
case atom Circle
  debuglog "circle"
case term Rect dims
  // dims: (f32, f32), the whole payload bound to one name
  debuglog dims
case term Tri sides
  debuglog sides
end match
```

No deep destructuring -- just a single binding for the whole payload.
Match consumes (moves) its input.
Match must be exhaustive; use `case default` for a catch-all.

```datalove
match s
case atom Circle
  debuglog "circle"
case default
  debuglog "not a circle"
end match
```


## Dynamic types

| Type    | Literal                          |
|---------|----------------------------------|
| `data`  | `data 1` / `data : u32 / 2`     |
| `error` | `error "oops"` / `error(expr)`   |

Any type coerces to `data`.
