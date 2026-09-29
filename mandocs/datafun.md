# Datalove Functions Primer

_Datalove Functions_ is the side-effect-free sublanguage of Datalove.
We often refer to it as _datafun_.
On top of the data types defined by [Datalove Literals](datalit.md) it adds:

- Pure and (nearly) total functions.
- A simple, acyclic module system.
- Constants with full compile-time function evaluation.
- Reactive script units that may be chained together, and that incrementally
  recompile and reevaluate as dependent units and modules are updated.

Datafun scripts may be interpreted directly via lowered IR, optionally with per-function JIT,
or may be compiled to statically-linked binaries, either via Cranelift or C.
The Datalove Functions implementation is self-contained and independent from full Datalove
and suitable for use as a constrained embedded application scripting language.

This document is a broad overview of the language.
For detail see additional documentation.


## Contents



## Functions

## Control flow

## Data types and destructuring

## Option and result

## Data types and their operations

## The `adapt` operator

## Constants and compile-time evaluation

## Modules and their organization

modules, packages, libraries

## The standard library

## Scripts

## Interactive script units

## Workspaces
