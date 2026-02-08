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
// Lists
let a: [int] = [1, 2, 3]

// Maps
let a: map<int, int> = map { 0 = 5, 1 = 2 }

// Sets
let a: set<int> = set { 1, 2, 3 }

// Tables
let a: {|
  col1: T1,
  col2: T2
|} = {|
  x, y
  1, 2
  3, 4
|}

// Tensors
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
| `?T`                          | `some 1` <br> `none`             | `some a` <br> `none` † |
| `!T`                          | `ok 1` <br> `er 2`               | `ok a` <br> `er b` † |
| `atom Foo`                    | `atom Foo`                    | `let atom Foo`                |
| `tag Foo T`                   | `tag Foo 1`                   | `let tag Foo a`               |
| `enum { atom A, tag B T }`    | `enum { atom A }`             | †                             |
| `data`                        | `data 1` <br> `data : u32 / 2`   | n/a                             |
| `error`                       | `error 1` <br> `error : u32 / 2` | n/a                             |

† Sum types need to use `match` or `if` for destructuring. See below.

```datalove
// Tuples
let t: (bool, u32) = (true, 42)
let (a, b) = t

// Structs
let s: { x: f32, y: f32 } = { x = 1.0, y = 2.0 }
let { x, y } = s
let { x = my_x, y = my_y } = s

// Option
let o: ?int = some 1

// Result
let r: !int = ok 1

// Atoms and tags
let a: atom Foo = atom Foo
let t: tag Bar int = tag Bar 1
let atom Foo = a
let tag Bar x = t

// Enums
type Shape: enum {
  atom Circle,
  tag Rect (f32, f32),
}

let s: Shape = enum { atom Circle }
let s: Shape = enum { tag Rect (1.0, 2.0) }
let s: Shape = atom Circle@
let s: Shape = tag Rect (1.0, 2.0)@

// Data and error
let d: data = data 42
let d: data = data : u32 / 42
let e: error = error "oops"
```




## Destructuring sum types with `if` and `match`

Optional and result types with `if`.

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
  tag Rect (f32, f32),
  tag Tri (f32, f32, f32),
}

let s: Shape = tag Rect (3.0, 4.0)@

var area: f64 = 0.0
match s
case atom Circle
  area = 0.0
case tag Rect dims
  // dims: (f32, f32), the whole payload bound to one name
  area = 0.0
case tag Tri sides
  // sides: (f32, f32, f32)
  area = 0.0
end match
```
