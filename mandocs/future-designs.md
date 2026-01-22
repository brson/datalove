# Future designs




## 2026/01/22 - Atoms and enums

The design of enums is intertwined with the design of named types.

Datalove is mostly structurally typed,
with a single mechanism to introduce a named version of
any structural type.

Enum variants are also named types,
so we treat them as the primary mechanism of naming,
with enums summing named types.

```datalove
// Anonymous atom
let a = atom Foo (1)

// The name is followed by a single expr
let a = atom Foo (1)    // tuple
let a = atom Foo 1      // etc
var b = atom Bar set { 1 }

// For anonymous enums the name must match
// for compatibility but the names still don't have
// (identity?) - any two names match.
set b = atom Bar set { 2 }

// Named atom
atom Foo: int

// Anonymous atom coerces to named atom
let c: Foo = atom Foo 1
```

Anonymous enums

```datalove
let a = atom Foo 1

let b: enum Bunny {
  Foo: int,
} = a
```

todo




## 2026/01/21 - Ergonomic switches

- fixed int math op widening to int
- auto-@ operator

merge former into @.

Add "auto-coerce" feature,
off for modules,
on for scripts.

Could be toggleable:

```datalove
feature auto-coerce off
```

Comparison to visual basic modes that I've forgotten, js strict mode.

Makes intro scripting easy,
gives options when moving to writing modules.




## 2026/01/21 - Postfix operator `@` - lossless clone/coerce

A single operator for making expression types "fit" their destination.
Balances correctness with scripting ergonomics.

Performs whatever lossless conversion is needed:
- **Widen** fixed integers along their signedness chain
- **Clone** linear types so the original remains valid
- **Both** when widening produces a linear type

Target type inferred from context (assignment, parameter, binary op).

### Clone (linear types)

```
let x: int = 42
let y = x@          // clone x
let z = x           // x still valid
```

Use in loops where a linear value is consumed repeatedly:

```
fun sum_n_times(val: int, n: u32): int
    var acc: int = 0
    var i: u32 = 0
    loop while i .< n
        set acc = acc + val@  // clone each iteration
        set i = i + 1
    end loop
    ret acc
end fun
```

Multiple consumption in a single call:

```
fun consume_both(a: int, b: int): int
    ret a + b
end fun

let x: int = 100
let result = consume_both(x@, x)  // clone for first, move for second
```

### Widen (fixed integers)

```
let a: u8 = 10
let b: u32 = a@     // widen u8 to u32
```

Cross-sign conversion is allowed when lossless:

```
let a: u8 = 5
let b: i16 = a@
```

Valid widening chains:

```
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
usize -> int
isize -> int
u8 -> i16 ...
u16 -> i32 ...
u32 ->
```

`usize` and `isize` don't participate in fixed-int widening.
Conversion to/from these types are always considered lossy.


Binary operators propagate expected type:

```
let a: u8 = 100
let b: u8 = 50
let sum: u32 = a@ + b@    // + propagates u32 to both sides

let bytes: u32 = 4096
let limit: u64 = 1000000
if bytes@ .< limit        // .< propagates u64 to left side
    // ...
end if
```

Function parameters propagate their types:

```
fun lerp(a: u64, b: u64, t: u64): u64
    ret a + (b - a) * t / 100
end fun

let lo: u8 = 0
let hi: u8 = 255
let pct: u16 = 50
let mid = lerp(lo@, hi@, pct@)  // each @ gets u64 from param type
```

### Widen + clone

Widening to `int` produces a linear type, so `@` clones if needed:

```
let a: u8 = 10
let b: int = a@     // widen to int (linear), clone happens implicitly
let c: int = a@     // can do it again
```

### Errors

```
let x: u8 = 10
let y = x@          // ERROR: no type context for coercion
let w: i32 = x@     // ERROR: crosses sign boundary (unsigned to signed)
```

### Behavior on copy types without widening

When applied to a copy type where source and target are the same type, `@` is a no-op.

```
let x: u32 = 10
let y: u32 = x@     // no-op, x is copy type, no widening needed
```

### Interaction with try operators

`@` composes with `?` and `!`:

```
let val: u32 = get_byte()?@   // unwrap option, then widen
let data: int = fetch()!@     // unwrap result, then clone
```




### 2026-01-17 - `assert` statements

todo

```datalove
fun test_thing()

end fun
```




# Bitwise operators

Shift operators are logical.
For arithmetic right shift use divide by 2.

Can't have << and >> because of ambiguous lex.
Well we can have .<< and .>>.

bitand
bitor
bitnot
bitxor

.<< .>>
& | ~ ^

to use `|` we would need to change the `if expr |arg|` syntax.

