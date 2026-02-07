# Future designs




## 2026/01/22 - Atoms, tags, enums, and match

Datalove is structurally typed.
_Atoms_ and _tags_ introduce types with a name component,
and _enums_ sum them together.

```datalove
// Anonymous atom
let a = atom Foo

let b: atom Foo = atom Foo;
```

Two atoms with the same name are type-compatible.

```datalove
var c = atom Foo;
set c = atom Foo;
```

Two atoms with different names are not type-compatible.

```datalove
var c = atom Foo;
set c = atom Bar;  // XXX error
```

Tags are like atoms but carry a typed value.

```datalove
// Anonymous tag
let a = tag Foo (1)

// The name is followed by a single expr
let a = tag Foo (1)    // tuple
let a = tag Foo 1      // etc
var b = tag Bar set { 1 }

// The name and type must match.
var c = tag Bar [1]
set c = tag Bar [1, 2]
```

With type hint.

```datalove
let d: tag Foo int = tag Foo 1
```




Anonymous enums are sets of types,
either atoms or tags.

```datalove
type MyEnum: enum {
  atom Foo,
  tag Bar int,
  tag Baz (f32, f32),
};

// The full enum literal form
let a: MyEnum = enum { atom Foo }

// Can just coerce
let b: MyEnum = atom Foo@
let c: MyEnum = (tag Bar 1)@
```

Only `atom` and `tag` types are allowed in enums.

Note the "full enum literal form" does not synthesize
a type; it must be in checking context.

Enums can be destructured in `match` statements.

```datalove
type Bunny: enum {
  atom Foo,
  tag Bar int,
  tag Baz (f32, f32),
};

let a = enum { 
```




## 2026/01/21 - Ergonomic switches

- fixed int math op widening to int
- auto-@ operator

merge former into @.

Add "auto-coerce" feature,
off for modules,
on for scripts.

Could be toggleable:

```datalove
feature auto_coerce off
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

