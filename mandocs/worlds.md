# Datalove Worlds

Datalove is a whole-program compiler.
This means that it sees all input upfront
and is able to perform analysis globally.

The total compiler input we call the _world_.
It consists of source text,
organized into modules,
organized into packages,
organized into package libraries,
and scripts,
all of which may or may not be on disk;
it also consists of native _riders_ &mdash;
Rust code that must be built and loaded
with the Datalove code.

The Datalove world is usually loaded from files
on disk, its structure inferred;
but it can also be loaded from a single _worldfile_
that holds all source text and module structure.



## On-disk world structure

A world has a system library, `sys`,
and user libraries, such as `local`.
A library is a directory of packages;
a package is a directory of modules;
a module is a `.dfm` file,
named by its path, `library/package/module`:

```
sys/
  std/
    manifest.toml
    list.dfm          // sys/std/list
    string.dfm        // sys/std/string
    rider/
      rider.dli       // native function declarations
      Cargo.toml      // the Rust crate implementing them
      src/lib.rs
```

Modules are found by convention, never listed:
any directory in a library holding `.dfm` files,
or a `manifest.toml`, is a package.

A package has at most one rider, in `rider/`.
A package with a rider must have a `manifest.toml`
naming the rider's crate and its exact version.



## Worldfiles

A worldfile is a sequence of sections,
each a header between two `----------` separator lines,
followed by the section's text.
They are used mostly for testing the compiler,
and run with `datalove script-world`.
There is no fixed extension;
the test suites use `.world` and `.wf`.

```
----------
module local/geo/point
----------

fun manhattan(p: (int, int)): int
    let (x, y) = p
    ret x + y
end fun

----------
scriptunit-fragment
----------

require module local/geo/point
debuglog point.manhattan((3, 4))
```

Section headers:

- `module lib/pkg/mod` - a module's source
- `scriptunit-fragment` - a script unit
- `scriptunit-expr` - a script that is a single expression,
  whose value is printed
- `rider <package>` - a rider interface, attached to the package of that name;
  it declares native functions but cannot implement them,
  so they must already be linked into the binary
- `module-add`, `module-remove`, `module-change-ws`,
  `module-change-ast`, `module-change-ty` - edits to a module,
  for testing incremental recompilation
- `inline-directives` - for testing the inliner
