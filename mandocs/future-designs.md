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

