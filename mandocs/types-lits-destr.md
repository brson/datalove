# Types, Literals, and Destructuring

Quick reference for type syntax, literal expression syntax,
and destructuring syntax for all datalove types.




## Primitives

| Type              | Literal          | Destructuring    |
|-------------------|------------------|------------------|
| `bool`            | `true`, `false`  | n/a (copy)       |
| `f32`, `f64`      | `3.14`           | n/a (copy)       |
| `int`             | `42`             | n/a (move)       |
| `string`          | `"hello"`        | n/a (move)       |
| `u8` .. `u64`     | `: u32 / 42`     | n/a (copy)       |
| `i8` .. `i64`     | `: i32 / -1`     | n/a (copy)       |
| `index`           | `: index / 0`    | n/a (copy)       |
| `offset`          | `: offset / 0`   | n/a (copy)       |

Bare integer literals synthesize as `int`.
Fixed-width integers require a type hint or checking context.




## Collections

| Type                             | Literal                          | Destructuring                    |
|----------------------------------|----------------------------------|----------------------------------|
| `[T]`                            | `[1, 2, 3]`                      | n/a                              |
| `map<K, V>`                      | `map { 0 = 5, 1 = 2 }`           | n/a                              |
| `set<T>`                         | `set { 1, 2, 3 }`                | n/a                              |
| `{\| col1: T1, col2: T2 \|}`     | `{\| col1, col2; 1, 2; 3, 4 \|}` | n/a                              |
| `tensor<T, N>`                   | `tensor [2, 3] [1 2 3, 4 5 6]`   | n/a                              |

```datalove
let a: [int] = [1, 2, 3]

let a: map<int, int> = map { 0 = 5, 1 = 2 }

let a: set<int> = set { 1, 2, 3 }

let a: {|
  col1: T1,
  col2: T2
|} = {|
  x, y
  1, 2
  3, 4
|}

let a: tensor<T, N> = tensor [2, 3] [
  1 2 3,
  4 5 6,
]
```




## Aggregates

| Type                          | Literal                       | Destructuring                 |
|-------------------------------|-------------------------------|-------------------------------|
| `()`                          | `()`                          | n/a                           |
| `(T1, T2)`                    | `(true, 42)`                  | `let (a, b)`                  |
| `{ x: T1, y: T2}`             | `{x = 1, y = 2}`              | `let {x, y}` <br> `let {x = my_x, y = my_y}` |
| `?T`                          | `some 1` <br> `none`             | †                             |
| `!T`                          | `ok 1` <br> `er 2`               | †                             |
| `atom Foo`                    | `atom Foo`                    | `let atom Foo`                |
| `tag Foo T`                   | `tag Foo 1`                   | `let tag Foo a`               |
| `enum { atom A, tag B T }`    | `enum { atom A }`             | †                             |
| `data`                        | `data 1` <br> `data : u32 / 2`   | n/a                             |
| `error`                       | `error 1` <br> `error : u32 / 2` | n/a                             |

† Sum types need to use `match` or `if` for destruction.

```datalove
todo

// Enums can be created with coercion.
let a: enum { atom A } = atom A@
```




## Destructuring sum types with `if` and `match`

todo