# A spec for bots

I'm falling behind on keeping canonical docs up to date:

- README.md
- demo-datalit.dlt
- demo-datafun-script.dfs
- demo-datafun-module.dfm
- notes/anytype.md
- notes/arrays.md
- notes/bitmanip.md
- notes/module-system.md
- notes/panicking.md
- notes/repl-ui.md
- notes/script-semantics.md
- notes/sigil-assignments.md
- notes/typing-rules.md
- notes/zipper-heaps.md

We should keep a bot-owned spec that actually is up to date
for bot purposes.

Put it in notes/botspec.md

Read my existing partial specs linked above,
but be aware that some may be out of date or represent future designs.

Make a plan to incrementally implement this document,
starting with datalit syntax, typechecking etc.,
datafun etc.
Use an organization that is similar to what my own docs suggest.

At each step compare the spec to the implementation and make note.
Only include features that have some implementation progress.
Use an appendix to note documented features that don't exist at all
or are completely wrong compared to the implementation.

---

## Execution Plan

### Document Structure

```
notes/botspec.md
├── Header (purpose, last-verified date)
├── 1. Datalit Layer
│   ├── 1.1 Primitive Types
│   ├── 1.2 Collection Types
│   ├── 1.3 Aggregate Types (tuples, structs, enums)
│   ├── 1.4 Special Types (option, result, error, data)
│   ├── 1.5 Heap Annotations
│   └── 1.6 Literal Syntax
├── 2. Datafun Layer
│   ├── 2.1 Statements (let, fun, ret, if, require, import)
│   ├── 2.2 Expressions
│   ├── 2.3 Operators (arithmetic, checked, optional)
│   ├── 2.4 Function Definitions
│   ├── 2.5 Parameter Modes (in, out, ref, mut)
│   └── 2.6 Module System
├── 3. Type System
│   ├── 3.1 Bidirectional Typing
│   ├── 3.2 Numeric Widening
│   ├── 3.3 Coercions
│   └── 3.4 Copy vs Linear Types
├── 4. Runtime/REPL
│   ├── 4.1 CLI Commands
│   └── 4.2 REPL Capabilities
└── Appendix A: Documented But Unimplemented Features
```

### Implementation Steps

**Step 1: Create Document Skeleton**
Create `notes/botspec.md` with header and section structure.

**Step 2: Datalit Section**
Verify against:
- `crates/datalove-datalit/src/ast.rs` - AST types
- `crates/datalove-datalit/src/tycheck.rs` - type checking
- `demo-datalit.dlt` - syntax examples

Document: primitives, collections, aggregates, special types, heap annotations.

**Step 3: Datafun Section**
Verify against:
- `crates/datalove-datafun/src/ast.rs` - statement/expression AST
- `crates/datalove-datafun/src/tycheck.rs` - typechecker
- `crates/datalove-datafun/src/interp/mod.rs` - interpreter
- `demo-datafun-script.dfs` - syntax examples

Document: statements, expressions, operators, functions, parameter modes, modules.
Mark unimplemented: try operators, comparisons, if-in-function-body, pattern matching.

**Step 4: Type System Section**
Verify against:
- `notes/typing-rules.md` - formal rules
- `crates/datalove-datalit/src/tycheck.rs` - implementation

Document: bidirectional typing, widening, coercions, copy vs linear.

**Step 5: Runtime/REPL Section**
Verify against:
- `crates/datalove-cli/src/main.rs` - CLI commands
- `crates/datalove-repl/src/` - REPL implementation

Document: CLI commands, REPL capabilities.

**Step 6: Appendix**
List features from existing docs that have no implementation:
- Tensor/array operations (notes/arrays.md)
- Zipper heaps (notes/zipper-heaps.md)
- Full panic handling
- Comparison operators in expressions
- Try operators (? !)

### Key Files to Verify

| Section | Verify Against |
|---------|---------------|
| Datalit types | `crates/datalove-datalit/src/ast.rs`, `ty.rs` |
| Datalit syntax | `crates/datalove-datalit/src/parser.rs` |
| Datafun AST | `crates/datalove-datafun/src/ast.rs` |
| Datafun interp | `crates/datalove-datafun/src/interp/mod.rs` |
| Module system | `crates/datalove-datafun/src/package_*.rs` |
| CLI | `crates/datalove-cli/src/main.rs` |

### Format Guidelines

- Concise descriptions, not tutorial prose
- Code examples where they clarify syntax
- Mark unimplemented features inline with `[NOT IMPLEMENTED]`
- Mark partial features with `[PARTIAL: description]`
- Use tables for operator/type lists
