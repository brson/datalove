# Future designs




## 2026/02/19 - Pretty symbols

In a future where AI is writing most code and I'm just reviewing,
we can think about having the code use non-ascii symbols
where it makes it more readable.

```
// structs
{
  foo: int,
}

// tuples
(int, int)

// lists
[1, 2, 3]

// maps
%{ int = int }
map { int -> int }
⟪ int → int ⟫

// sets
#{ int }
set { int }
⟨ int ⟩

// tables
[| a, b ; 1, 2 |]
⟦ a, b ; 1, 2 ⟧

// tensors
(| int, 2 |)
⟬ int, 2 ⟭
```

NB 2026/09/29: I previously did this conversion on a branch but didn't
merge it. May still be there for reference.



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







