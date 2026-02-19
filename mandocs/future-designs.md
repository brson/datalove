# Future designs



## 2026/02/18 - Generics

```datalove

fun push(self: mut [T], v: T) where {
  T is move,
}
  rtcall dlrt_list_push(self, v)
end fun


```




## 2026/02/10 - Indexing and querying

All datalit aggregate and collection types have some way of
indexing or querying,
such that querying a node of any document can be done in one line.

```datalove
// Tuples
let a = (0, 1)
let x = a.0
let y = a.1

// Structs
let a = { b: 0, c: 1 }
let x = a.b
let y = a.c

// List
let a = [0, 1]
let x = a[0]?
let y = a[1]?


// Map
let a = %{ 100 = 1 }
let x = a[100]?


// Set
let a = #{ 100 }
let x = a[100]?


// Table
let a = {| b, c ; 0, 1 ; 2, 3 }
let x = a.b@   // column projection then clone
let y = a.b[0]? // column projection then index

// Tensor
let a = [| 0 1, 2 3 |]
let x = a[0]@ // row projection then clone
let y = a[0]?[]? // row projection, row projection

// Term
let a = term Foo 0
let x = a.0

// Option
let a = some 0
let b = a?
let a = some (1, 2)
let c = grab { a?.0 }?  // grab intercepts early returns?

// Result

// Enum
let a = enum { term A 1, term B 2 }
let b = a.extract { term A }?


```

Indexes are passed by reference.
Results are returned by reference,
mutability determined by context.

Indexing operations return an option.




## 2026/02/10 - Workspaces, the package world, and native riders




## 2026/02/10 - Metadata syntax

It'll be plain old data using
datalit types and either a restricted expression syntax
or more likely const-evaluated expressions;
all statements and expressions annotatable.

Sketches:

```datalove
# "data"
fun foo()
  let a = : int / 2

  let b = # "data" / 2
  let b = # "data" : int / 2
end fun

# term Linkage #{
  atom NoMangle,
  term SymbolName "foobar",
}
fun foo()
  let a = : int / 2

  let b = # term DebugInfo #{
    term Span (0, 0)
  } / 2
  let b = # term DebugInfo #{
    term Span (0, 0)
  } : int / 2
end fun

// Multiple attributes.
# "a"
# "b"
fun foo()
  let foo = # "a" # "b" # "c" / 2
end fun
```

Could try to enforce "inner" function attributes.

```datalove
fun foo()
  #^ term Linkage #{
    atom NoMangle,
    term SymbolName "foobar",
  }

  ret
end fun
```

Parameters and return values:

```datalove
fun foo(
  # "foo" ref a: int,

  // A single argument with multiple attributes.
  # "foo" # "bar" ;
  # "baz" ;
  b: int
) # "a" : int

  ret 2
end fun
```




## 2026/02/10 - Ergonomic switches for repl / scripts

Definitely need more ergonomics and less suprises in the repl.
Scripts may or may not need ergonomic support.
Onboarding repl -> script -> modules,
can get stricter with prhogress.

Needed:

- auto-coerce (auto-@ insertion).
  exists but not surfaced.
- auto clone, at least for script unit exports,
  bigints, strings, maybe not all types

Could be toggleable:

```datalove
feature auto_coerce off
feature auto_export_clone
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

