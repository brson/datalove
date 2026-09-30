# Future designs


## 2026/09/30 - Locating and loading the runtime, sys packages, and sys riders

Executing Datalove requires the runtime, the sys/ Datalove source library,
and the sys native riders. Today sys just contains sys/std and the corresponding
native rider written in Rust.

The runtime is composed of three crates:

- datalove-rtdt - data definitions
- datalove-rti - the abstract runtime interface
- datalove-rt - the runtime

The compiler manages linkage to the runtime itself - it is linked
into the interpreter for use by dynamically loaded riders,
and it is statically linked into AOT compiles.

Riders link to rtdt and rti through standard Rust static linkage.

The compiler needs direct access to datalove-rt to build it for AOT compiles.
The compiler needs to be able to find the riders that are declared by sys packages.
Riders need to find the correct versions of datalove-rtdt and datalove-rti.

There three scenarios that impact finding the rt, rtdt/rti, std and the std rider:

- in-tree or installed from a git checkout
- installed from crates.io

Some mechanism at build time embeds the info about the kind of build above,
git checkout builds include the git revision and absolute path to source.

Vaguely:

``rust
enum BuildInfo {
  Prod { version: SemVer },
  Local {
    git_sha: String,
    abs_path: PathBuf,
  }
}
```

Every workspace has a "work dir" where it can build to.
All riders are compiled into a "native component" in the work dir.

Datalove source packages that contain riders look like:

```
sys/std
  manifest.toml
  rider/
    rider.rdi
    Cargo.toml
    etc.
  mod.dfm etc
```

manifest.toml contains

```
[rider]
name = "datalove-rider-sys-std"
version = "0.1.0"
```

The std rider is named datalove-rider-sys-std, reserving the -sys- namespace for future packages.
When a datalove package is "packaged" the contents of the rider/ directory, except for rider.dri
is stripped. The remainder is the datalove package - the rider is sourced from crates.io.

In a production build:

- the sys packages are sourced from the datalove-sys-packages crate, embedded, excluding the rust source.
- the riders named in the sys packages are encoded into the synthetic native component crate by name and version, built from crates.io
- the native component crate is built into a target directory in the work dir
- for aot builds the datalove-rt crate is downloaded directly from crates.io into the workdir and built independently

In a local build:

- the sys packgages are sourced from the absolute path to the checkout
- the riders named in the sys packages are sourced from their local paths;
  not because it is a local build, but because the rider source is present next to the datalove packge,
  a general rule, not sys-specific
- the ative component crate is built into a target directory in the workdir
- for aot builds the datalove-rt crate is sourced from the absolute checkout path, built into a target directory in the workdir
- in all builds the path to datalove-rtdt and datalove-rti is overridden to the local path so it doesn't try to pull from crates.io

Obvious risk here is that the production path cannot be tested until the crates are published.




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







