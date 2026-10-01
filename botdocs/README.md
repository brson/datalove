# Datalove Bot Docs

Internal documentation maintained by AI assistants working on the Datalove compiler.
Specifications, research, design documents, implementation plans, and reports.

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
- [Datalit Grammar (EBNF)](datalit-ebnf.md)
- [Datalit Typing Rules](datalit-typing-rules.md)
- [Datafun Typing Rules](datafun-typing-rules.md)
- [Operator Precedence](op-precedence.md)
- [Sigil Assignments](sigil-assignments.md)
- [Salsa: idiomatic and effective use](salsa-patterns.md)
- [The compiler's salsa architecture](salsa-architecture.md)
- [The Native Rider ABI](native-abi.md)
- [index-64 Feature](index-64.md)

---

### Design

- [Place Expressions and Ephemeral References](places.md)
- [Clone and Coerce Operators](design-clone-and-coerce.md)
- [Token Gluing and Operator Fixity](design-token-gluing.md)
- [Auto-Adapt Mode](design-auto-adapt.md)
- [Tables: What the Type System Is Missing](design-table-rows.md)
- [Const Parameter Specialization](const-param-specialization.md)

---

### Plans

- [Generics and Specialization](plan-generics.md)
- [Fat References for Borrowed Generic Values](plan-fat-refs.md)
- [Reusing Compilation](plan-compile-reuse.md)
- [Const Parameter Implementation](const-param-impl-plan.md)
- [JIT + Inliner Integration](plan-fancy-jit.md)
- [Unified Code Unit Migration](plan-unified-code-unit.md)
- [Compiling What Is Reachable](plan-reachability.md)
- [Script Reactivity](plan-script-reactivity.md)
- [Sourcing the Runtime, Sys Packages and Riders](plan-rider-sourcing.md)

---

### Reports

- [The State of Worldgen](reports/report-worldgen-state.md)

- [`.fui` on bcts](report-fairlightui-on-bcts.md)
- [Adapt Operator Error Cases](report-adapt-cases.md)
- [Advanced PLT Directions](report-advanced-ideas.md)
- [GADTs Without Dependent Types](report-gadts.md)
- [Pattern Matching and Destructuring](report-match-downcast.md)
- [Coercion Implementation](reports/report-coercion-impl.md)
- [Stdlib Blockers](reports/report-stdlib-blockers.md)
- [Const Parameters and Generics](reports/report-const-params-and-generics.md)
- [The JIT and the Inliner](reports/report-jit-and-inliner.md)
- [Profiling the Front End](reports/report-frontend-profile.md)
- [Primary vs Full Expression Parsing](report-expr-primary.md)
- [Datalog-Style Programming with Atoms/Terms/Enums](report-datalog.md)
- [Overloading or Traits](report-overloading-vs-traits.md)

---

### Research

- [Atom / Term / Enum PL Landscape](research/research-atom-tag-enum.md)
- [Linear Types](research/research-linear-types.md)
- [Const Evaluation](research/research-const-eval.md)
- [Logic Programming](research/research-logic-programming.md)
- [REPL and Interpreter Architecture](research/research-repl-interp.md)
- [Termination and Refinement Types](research/research-termination-refinement.md)
- [Type Annotation Syntax](research/research-type-annotations.md)
- [WASM Component Model](research/research-wasm-components.md)
- [Arrays and Tensors](research/arrays.md)
- [WebComponents Lowering](research-webcomponents.md)
- [Brace Styles](research-braces.md)

---

### Tasks

- [Tasks](tasks.md)
- [Carry/Bring](carry-bring.md)

---

### Process

- [Releasing](release.md)
