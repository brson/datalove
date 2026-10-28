# Datalove Bot Docs

Internal documentation maintained by AI assistants working on the Datalove compiler.
Specifications, research, design documents, implementation plans, and reports.
Content here tends to drift, but is often quite useful.
The [botspec](botspec.md) in particular is usually close to correct and current.

---

### Issues

- [Known issues](issues.md)

---

### Specifications

- [Language Specification](botspec.md)
- [Compiler Guide](compiler-guide.md)
- [The Datafun IR](ir.md)
- [Generics: how it works](generics.md)
- [REPL Architecture](repl-architecture.md)
- [The Responsive Scripting Environment](script-reactivity-architecture.md)
- [Script Reactivity, Case by Case](script-reactivity-cases.md)
- [Script Glossary](script-glossary.md)
- [Datalit Grammar (EBNF)](datalit-ebnf.md)
- [Datafun Grammar (EBNF)](datafun-ebnf.md)
- [Datalit Typing Rules](datalit-typing-rules.md)
- [Datafun Typing Rules](datafun-typing-rules.md)
- [Sigil Assignments](sigil-assignments.md)
- [Salsa: idiomatic and effective use](salsa-patterns.md)
- [The compiler's salsa architecture](salsa-architecture.md)
- [The Native Rider ABI](native-abi.md)
- [Where Compile Time Goes](compile-time.md)
- [Testing Under Miri](miri.md)

---

### Design

- [Tables: What the Type System Is Missing](design-table-rows.md)
- [Generics and Specialization](plan-generics.md)
- [Specializing Generics](plan-monomorphization.md)
- [Script Reactivity](plan-script-reactivity.md)
- [A Register Bytecode for the Interpreter](plan-bytecode.md)
- [A Frame Stack for the Interpreter](plan-frame-stack.md)
- [Testing the Engines Against One Reference](plan-engine-tests.md)
- [Tuning the JIT](plan-jit-tuning.md)
- [Consts Built Once](plan-static-consts.md)
- [A Runtime Datalit Loader](plan-datalit-loader.md)
- [One Rule for Literals That Share a Type](plan-literal-typing.md)
- [`for` Loops over the Collections](plan-for-loops.md)
- [Generics by Reference](plan-fat-generics.md)
- [Organizing the IR Passes](plan-ir-passes.md)
- [A Parser That Reads Tokens in Place](plan-parser.md)
- [A Plain-Data AST](plan-plain-ast.md)
- [Parsing Large Collection Literals in Parallel](plan-parallel-collections.md)
- [Lexical Scope at the Top Level](plan-lexical-scope.md)
- [Carry/Bring](carry-bring.md)

---

### Research

- [Datalog-Style Programming with Atoms/Terms/Enums](research/research-datalog.md)
- [Temporal Types](research/research-temporal-types.md)
- [Lowering to WebAssembly Components](research/research-webcomponents.md)
- [Are We Fast Yet for benchvs](research/research-awfy-benchmarks.md)

---

### Process

- [Tasks](tasks.md)
- [Releasing](release.md)
- [Writing Style](writing-style.md)
