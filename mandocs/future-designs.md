# Future designs


## 2026/09/30 - Locating and loading the runtime, sys packages, and sys riders

Executing Datalove requires the runtime, the sys/ Datalove source library,
and the sys native riders. Today sys just contains sys/std and the corresponding
native rider written in Rust.

There three scenarios that impact finding the rt, std and std rider:

- in-tree
- installed from a git checkout
- installed from cargo

some mechanism at build time embeds the info about the kind of build above,
git checkout builds include the git revision and absolute path to source.




sourcing:
in-tree all from the source tree;
from git checkout - from the absolute source path (today), from github by rev once the repo is public;
from cargo - rt crates from crates.io using same version number, std rider from crates.io

todo

we name the rider crate "datalove-rider-sys-std" to make sure there is namespace for other sys packages.
i don't envision the std datalove code always coming from the rider package, but ite

  



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







