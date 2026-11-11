# Datalove Bot Docs

Internal documentation maintained by AI assistants working on the Datalove compiler.
The reference docs describe the language and the compiler as they are and are
kept current; the [botspec](botspec.md) and the [compiler guide](compiler-guide.md)
are the place to start. Open plans describe work not yet done. History is
finished plans, kept for what was weighed and rejected rather than as a
description of the code. `botdocs/learn.dfs` is a tour of Datafun written as a
script, so it stays true by running.

---

### Issues

- [Known issues](issues.md)

---

### Reference: the language

- [Language Specification](botspec.md)
- [Datalit Grammar (EBNF)](datalit-ebnf.md)
- [Datafun Grammar (EBNF)](datafun-ebnf.md)
- [Datalit Typing Rules](datalit-typing-rules.md)
- [Datafun Typing Rules](datafun-typing-rules.md)
- [Proving the Type System](formal.md)
- [Sigil Assignments](sigil-assignments.md)

---

### Reference: the compiler

- [Compiler Guide](compiler-guide.md)
- [The Datafun IR](ir.md)
- [Generics: how it works](generics.md)
- [The Native Rider ABI](native-abi.md)
- [The compiler's salsa architecture](salsa-architecture.md)
- [Salsa: idiomatic and effective use](salsa-patterns.md)
- [Where Compile Time Goes](compile-time.md)
- [Testing Under Miri](miri.md)
- [Rust Style: the Prelude and Powerletters](rust-style.md)

---

### Reference: scripts and the REPL

- [REPL Architecture](repl-architecture.md)
- [The Responsive Scripting Environment](script-reactivity-architecture.md)
- [Script Reactivity, Case by Case](script-reactivity-cases.md)
- [Script Glossary](script-glossary.md)
- [Documentation Fixtures and Session Replays](doc-fixtures.md)

---

### Open plans and designs

- [A Virtual I/O Layer](plan-vio.md)
- [Script Sessions in Parallel](plan-multi-session.md)
- [Specializing Generics](plan-monomorphization.md)
- [Consts Built Once](plan-static-consts.md)
- [Recursive Structural Types](plan-recursive-types.md)
- [Organizing the IR Passes](plan-ir-passes.md)
- [A Runtime Datalit Loader](plan-datalit-loader.md)
- [Generated Programs on Every Engine](plan-worldgen-engines.md)
- [Tables: What the Type System Is Missing](design-table-rows.md)

---

### History: finished plans, kept for the reasoning

- [Script Reactivity](plan-script-reactivity.md)
- [Generics by Reference](plan-fat-generics.md)
- [Generics and Specialization](plan-generics.md)
- [A Register Bytecode for the Interpreter](plan-bytecode.md)
- [Tuning the JIT](plan-jit-tuning.md)
- [Values in Registers for the Cranelift Backend](plan-values-in-registers.md)
- [`for` Loops over the Collections](plan-for-loops.md)
- [A Parser That Reads Tokens in Place](plan-parser.md)
- [A Plain-Data AST](plan-plain-ast.md)
- [Parsing Large Collection Literals in Parallel](plan-parallel-collections.md)
- [One Rule for Literals That Share a Type](plan-literal-typing.md)
- [Carry/Bring](carry-bring.md)

---

### Research

- [Reactive Scripts and the Literature](research/reactive-scripts-and-frp.md)
- [Datalog-Style Programming with Atoms/Terms/Enums](research/research-datalog.md)
- [Temporal Types](research/research-temporal-types.md)
- [Lowering to WebAssembly Components](research/research-webcomponents.md)
- [Are We Fast Yet for benchvs](research/research-awfy-benchmarks.md)

---

### Process

- [Tasks](tasks.md)
- [Releasing](release.md)
- [Writing Style](writing-style.md)
