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
but it can also be loaded from a single _worldfile_ (`.dlworld`)
that holds all source text and module structure;
and it can be defined in a _workspace_ file (`.dlws`)
that describes the on-disk structure.



## On-disk world structure




## Worldfiles

