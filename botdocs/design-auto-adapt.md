# Design: Auto-Adapt Mode and Diagnostics

This document describes the design for:
1. Enhanced diagnostics that suggest `@` insertion for recoverable errors
2. An `auto_adapt` mode that automatically recovers from these errors
3. A test suite validating both modes

## Background

See `botdocs/report-adapt-cases.md` for the catalog of all `@`-recoverable errors.

## Part 1: Enhanced Diagnostics

### Goal

When a recoverable error occurs, emit a diagnostic with a "help" note suggesting
the `@` fix:

```datalove
error[F016]: mismatched types
  --> example.dfs:5:20
   |
 4 |     let a = consume(msg)
   |                     --- value moved here
 5 |     let b = consume(msg)
   |                     ^^^ expected `string`, found moved value
   |
   = help: use `msg@` to clone the value
```

### Recoverable Error Codes

Mark these errors as `@`-recoverable in the codebase:

**Type Checker (F-codes)**:
- `F016` TypeMismatch - when `can_clone_coerce_to(actual, expected)` is true

**Ownership (D-codes)**:
- `D001` UseAfterMove
- `D002` DoubleMove
- `D007` MoveInLoop

### Implementation

#### 1. Add RecoveryHint to PendingDiagnostic

```rust
// crates/datalove-datafun-tycheck/src/lib.rs

/// Hint for how to recover from an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryHint {
    /// Insert @ after the expression to clone/coerce.
    InsertAdapt {
        /// The expression that should be adapted.
        expr_id: ExprFun<'db>,
        /// Human-readable description of what @ does here.
        description: String,
    },
    /// No automatic recovery available.
    None,
}

pub enum PendingDiagnostic<'db> {
    TypeMismatch {
        expr: ExprFun<'db>,
        expected: String,
        actual: String,
        module_id: Option<ModuleId>,
        recovery_hint: RecoveryHint,  // NEW
    },
    // ... other variants get recovery_hint where applicable
}
```

#### 2. Compute RecoveryHint at Error Creation

```rust
// crates/datalove-datafun-tycheck/src/context.rs

impl<'db> TypeContext<'db> {
    pub fn error_type_mismatch(
        &mut self,
        expr: ExprFun<'db>,
        expected: &Type<'db>,
        actual: &Type<'db>,
    ) {
        let recovery_hint = if can_clone_coerce_to(actual, expected, self.db) {
            RecoveryHint::InsertAdapt {
                expr_id: expr,
                description: format_adapt_description(actual, expected),
            }
        } else {
            RecoveryHint::None
        };

        self.pending_diagnostics.push(PendingDiagnostic::TypeMismatch {
            expr,
            expected: type_to_string(self.db, expected),
            actual: type_to_string(self.db, actual),
            module_id: self.module_id,
            recovery_hint,
        });
    }
}

fn format_adapt_description(from: &Type, to: &Type) -> String {
    if types_equivalent(from, to) {
        "clone the value".to_string()
    } else {
        format!("convert from `{}` to `{}`", from, to)
    }
}
```

#### 3. Emit Help Note in Diagnostic

```rust
// crates/datalove-datafun-tycheck/src/emit.rs

fn emit_type_mismatch<'db>(
    db: &'db dyn Db,
    diag: &PendingDiagnostic<'db>,
    span_lookup: &dyn SpanLookup<'db>,
) {
    let PendingDiagnostic::TypeMismatch {
        expr, expected, actual, module_id, recovery_hint
    } = diag else { return };

    let span = span_lookup.lookup_expr(*expr);

    let mut builder = DiagnosticBuilder::error(db, format!(
        "mismatched types: expected `{}`, found `{}`", expected, actual
    ))
    .code("F016")
    .primary_label(span, format!("expected `{}`", expected));

    // Add recovery hint if available
    if let RecoveryHint::InsertAdapt { description, .. } = recovery_hint {
        builder = builder.note(format!("help: use `@` to {}", description));
    }

    builder.emit_type();
}
```

#### 4. Similar Changes for Ownership Errors

```rust
// crates/datalove-datafun-sema/src/lib.rs (or ownership module)

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalysisError {
    UseAfterMove {
        local_index: u32,
        name: String,
        first_move_index: Option<u32>,  // NEW: where it was first moved
    },
    DoubleMove {
        local_index: u32,
        name: String,
        first_move_index: u32,  // NEW
    },
    MoveInLoop {
        local_index: u32,
        name: String
    },
    // ... rest unchanged
}
```

The ownership error formatter adds help notes:

```rust
fn format_use_after_move(error: &AnalysisError, spans: &DatafunSpans) -> Diagnostic {
    // ... primary label at use site ...
    // ... secondary label at first move site ...

    builder.note(format!(
        "help: use `{}@` to clone the value before the first use",
        error.name
    ))
}
```

---

## Part 2: Auto-Adapt Mode

### Goal

When `auto_adapt` is enabled, the compiler:
1. Detects recoverable errors
2. Implicitly inserts `@` at the appropriate locations
3. Continues compilation as if the user had written `@`
4. Optionally reports which adaptations were made

### Configuration

```rust
// crates/datalove-datafun-common/src/lib.rs

/// Controls automatic adaptation of type mismatches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutoAdaptMode {
    /// Disabled - emit errors for mismatches (default).
    #[default]
    Disabled,
    /// Enabled - automatically insert @ for recoverable errors.
    Enabled,
    /// Enabled with reporting - insert @ and emit info diagnostics.
    EnabledWithReport,
}

pub fn auto_adapt_mode_from_env() -> AutoAdaptMode {
    match std::env::var("DATALOVE_AUTO_ADAPT") {
        Ok(val) if val ≡ "report" => AutoAdaptMode::EnabledWithReport,
        Ok(_) => AutoAdaptMode::Enabled,
        Err(_) => AutoAdaptMode::Disabled,
    }
}
```

### Pipeline Integration

#### 1. Thread Through Compilation

```rust
// crates/datalove-datafun-compiler/src/compile.rs

pub struct CompileOptions {
    pub parallel_mode: ParallelMode,
    pub auto_adapt_mode: AutoAdaptMode,  // NEW
}

pub fn compile_modules<'db>(
    db: &'db dyn Db,
    input: ModuleCompilationInput<'db>,
    options: CompileOptions,
) -> ModuleCompilationOutput<'db> {
    // ... parse phase ...

    let typecheck_result = typecheck_module_graph_with_mode(
        db,
        parsed,
        name_resolutions,
        options.parallel_mode,
        options.auto_adapt_mode,  // NEW
    );

    // ... ownership phase ...
}
```

#### 2. TypeContext with Auto-Adapt

```rust
// crates/datalove-datafun-tycheck/src/context.rs

pub struct TypeContext<'db> {
    // ... existing fields ...
    auto_adapt_mode: AutoAdaptMode,
    /// Adaptations that were automatically applied.
    auto_adaptations: Vec<AutoAdaptation<'db>>,
}

#[derive(Clone, Debug)]
pub struct AutoAdaptation<'db> {
    pub expr: ExprFun<'db>,
    pub from_type: Type<'db>,
    pub to_type: Type<'db>,
    pub kind: AdaptKind,
}

#[derive(Clone, Copy, Debug)]
pub enum AdaptKind {
    Clone,
    Widen,
    CloneAndWiden,
}
```

#### 3. Auto-Adapt in Type Checking

```rust
// crates/datalove-datafun-tycheck/src/check.rs

impl<'db> TypeContext<'db> {
    /// Check expression against expected type, with auto-adapt support.
    pub fn check_expr(
        &mut self,
        expr: ExprFun<'db>,
        expected: &Type<'db>,
    ) -> Result<(), ()> {
        let actual = self.synthesize_expr(expr)?;

        if types_equivalent(self.db, &actual, expected) {
            return Ok(());
        }

        // Try auto-adapt if enabled
        if self.auto_adapt_mode ≢ AutoAdaptMode::Disabled {
            if can_clone_coerce_to(&actual, expected, self.db) {
                self.record_auto_adaptation(expr, &actual, expected);
                // Store adapted type for this expression
                self.store_expr_type(expr, expected.clone());
                return Ok(());
            }
        }

        // Emit error (with recovery hint)
        self.error_type_mismatch(expr, expected, &actual);
        Err(())
    }

    fn record_auto_adaptation(
        &mut self,
        expr: ExprFun<'db>,
        from: &Type<'db>,
        to: &Type<'db>,
    ) {
        let kind = determine_adapt_kind(from, to);
        self.auto_adaptations.push(AutoAdaptation {
            expr,
            from_type: from.clone(),
            to_type: to.clone(),
            kind,
        });

        // Emit info diagnostic if reporting enabled
        if self.auto_adapt_mode ≡ AutoAdaptMode::EnabledWithReport {
            self.pending_diagnostics.push(PendingDiagnostic::AutoAdapted {
                expr,
                from_type: type_to_string(self.db, from),
                to_type: type_to_string(self.db, to),
                kind,
            });
        }
    }
}
```

#### 4. Auto-Adapt in Ownership Analysis

```rust
// crates/datalove-datafun-ownership/src/lib.rs

pub struct OwnershipOptions {
    pub auto_adapt_mode: AutoAdaptMode,
}

impl<'db> AnalysisCtx<'db> {
    fn analyze_expr_moves(&mut self, expr: ExprFun<'db>, is_consumed: bool) -> Option<BindingId> {
        // ... existing logic ...

        // When we would emit UseAfterMove/DoubleMove/MoveInLoop:
        if self.options.auto_adapt_mode ≢ AutoAdaptMode::Disabled {
            // Instead of error, record that @ should be inserted
            self.auto_clone_insertions.push(expr);
            // Don't mark as moved - the clone keeps original alive
            return None;
        }

        // Normal error path
        self.errors.push(AnalysisError::UseAfterMove { ... });
    }
}
```

#### 5. IR Lowering with Auto-Adaptations

The auto-adaptations need to be communicated to IR lowering:

```rust
// crates/datalove-datafun-lower/src/lib.rs

pub struct LoweringContext<'db> {
    // ... existing fields ...
    /// Expressions that should be wrapped with CloneCoerce.
    auto_adaptations: HashSet<ExprId>,
}

impl<'db> LoweringContext<'db> {
    fn lower_expr(&mut self, expr: ExprFun<'db>) -> IrExpr {
        let lowered = self.lower_expr_inner(expr);

        // Wrap with CloneCoerce if auto-adapted
        if self.auto_adaptations.contains(&expr.as_id()) {
            return self.emit_clone_coerce(lowered, ...);
        }

        lowered
    }
}
```

---

## Part 3: Example Test Suite

### Directory Structure

```
crates/datalove-datafun-compiler/tests/
├── fixtures/
│   └── auto-adapt/
│       ├── 001_widen_u8_to_int.dfs
│       ├── 001_widen_u8_to_int.out.expected      # non-auto-adapt (error)
│       ├── 001_widen_u8_to_int.adapt.expected    # auto-adapt (success + debuglog)
│       ├── 002_clone_string_use_twice.dfs
│       ├── 002_clone_string_use_twice.out.expected
│       ├── 002_clone_string_use_twice.adapt.expected
│       └── ...
└── auto_adapt_tests.rs
```

### Test File Format

Each `.dfs` file is a complete example demonstrating one adapt-recoverable case:

```datalove
// 001_widen_u8_to_int.dfs
// Tests: F016 type mismatch recoverable via widening

fn process(x: int) -> int
    ret x * 2

fn main() -> int
    let n: u8 = 42
    ret process(n)  // Without @: error. With auto-adapt: works.
```

### Expected Output Format

**Non-auto-adapt mode** (`.out.expected`):
```json
{
  "mode": "normal",
  "success": false,
  "diagnostics": [
    {
      "code": "F016",
      "severity": "error",
      "message": "mismatched types: expected `int`, found `u8`",
      "labels": [
        {"span": [120, 121], "text": "n", "style": "primary"}
      ],
      "notes": ["help: use `n@` to convert from `u8` to `int`"]
    }
  ]
}
```

**Auto-adapt mode** (`.adapt.expected`):
```json
{
  "mode": "auto-adapt",
  "success": true,
  "adaptations": [
    {
      "location": [120, 121],
      "from": "u8",
      "to": "int",
      "kind": "widen"
    }
  ],
  "execution": {
    "debuglog": ["main() returned: 84"]
  }
}
```

### Test Runner

```rust
// crates/datalove-datafun-compiler/tests/auto_adapt_tests.rs

use datalove_exampletest::{ExampleTest, TestResult};

#[test]
fn auto_adapt_examples() {
    let tests = ExampleTest::new("fixtures/auto-adapt")
        .extension("dfs")
        .allow_errors(true);  // We expect errors in non-adapt mode

    // Run each test in both modes
    let results: Vec<TestResult> = tests.run_all(|path| {
        let source = std::fs::read_to_string(path)?;

        // Test 1: Non-auto-adapt mode (should fail with diagnostic)
        let normal_result = run_compilation(&source, AutoAdaptMode::Disabled);
        let normal_output = format_test_output(&normal_result, "normal");

        // Test 2: Auto-adapt mode (should succeed)
        let adapt_result = run_compilation(&source, AutoAdaptMode::EnabledWithReport);
        let adapt_output = if adapt_result.is_success() {
            // Also run interpreter and capture debuglog
            let exec_result = run_interpreter(&adapt_result);
            format_test_output_with_execution(&adapt_result, &exec_result, "auto-adapt")
        } else {
            format_test_output(&adapt_result, "auto-adapt")
        };

        Ok(TestOutputPair {
            normal: normal_output,
            adapt: adapt_output,
        })
    });

    // Compare against expected files
    for result in results {
        match result {
            TestResult::Passed => {},
            TestResult::Failed { expected, actual } => {
                panic!("Test failed:\nExpected:\n{}\nActual:\n{}", expected, actual);
            }
            _ => {}
        }
    }
}
```

### Test Cases

Based on `report-adapt-cases.md`, create these test files:

#### Widening Cases
```datalove
001_widen_u8_to_u16.dfs      - u8 -> u16
002_widen_u8_to_int.dfs      - u8 -> int
003_widen_i8_to_i32.dfs      - i8 -> i32
004_widen_u8_to_i16.dfs      - cross-sign: u8 -> i16
005_widen_u16_to_i32.dfs     - cross-sign: u16 -> i32
006_widen_u32_to_i64.dfs     - cross-sign: u32 -> i64
007_widen_in_list.dfs        - list element widening
008_widen_in_tuple.dfs       - tuple field widening
009_widen_in_struct.dfs      - struct field widening
010_widen_in_map_key.dfs     - map key widening
011_widen_in_map_value.dfs   - map value widening
```

#### Clone Cases
```
020_clone_string_arg.dfs         - string to consuming function
021_clone_string_twice.dfs       - same string used twice
022_clone_list_arg.dfs           - list to consuming function
023_clone_in_tuple.dfs           - linear value in tuple twice
024_clone_int_bigint.dfs         - int (bigint) cloning
```

#### Ownership Cases
```datalove
030_use_after_move.dfs           - D001: use after move
031_double_move.dfs              - D002: double move in expression
032_move_in_loop.dfs             - D007: move in loop body
033_use_after_move_in_if.dfs     - D001 in conditional
034_double_move_tuple.dfs        - D002 in tuple construction
```

#### Edge Cases
```
040_already_adapted.dfs          - explicit @ present (no change)
041_copy_type_no_adapt.dfs       - copy type doesn't need @
042_incompatible_types.dfs       - non-recoverable mismatch (still error)
043_nested_adapt.dfs             - adaptation in nested expression
044_adapt_in_function_call.dfs   - adaptation at call site
```

### Debuglog Capture

For successful auto-adapt runs, capture interpreter output:

```rust
fn run_interpreter(compilation: &CompilationResult) -> ExecutionResult {
    let mut executor = ScriptExecutor::new(compilation.ir.clone());
    executor.set_debug_output(DebugOutputMode::Capture);

    match executor.run() {
        Ok(value) => ExecutionResult {
            success: true,
            return_value: Some(format_value(&value)),
            debuglog: executor.captured_debuglog(),
        },
        Err(e) => ExecutionResult {
            success: false,
            error: Some(e.to_string()),
            debuglog: executor.captured_debuglog(),
        },
    }
}
```

---

## Part 4: CLI Integration

### Flags

```rust
// crates/datalove-cli/src/main.rs

#[derive(Parser)]
struct ScriptCommand {
    /// Enable auto-adapt mode (automatically insert @ for recoverable errors)
    #[arg(long)]
    auto_adapt: bool,

    /// Enable auto-adapt with reporting (show what was adapted)
    #[arg(long)]
    auto_adapt_report: bool,

    // ... existing fields ...
}

impl ScriptCommand {
    fn auto_adapt_mode(&self) -> AutoAdaptMode {
        if self.auto_adapt_report {
            AutoAdaptMode::EnabledWithReport
        } else if self.auto_adapt {
            AutoAdaptMode::Enabled
        } else {
            auto_adapt_mode_from_env()
        }
    }
}
```

### Usage Examples

```bash
# Normal mode - errors on type mismatches
datalove run example.dfs

# Auto-adapt mode - silently fix recoverable errors
datalove run --auto-adapt example.dfs

# Auto-adapt with reporting - show what was fixed
datalove run --auto-adapt-report example.dfs

# Via environment variable
DATALOVE_AUTO_ADAPT=1 datalove run example.dfs
DATALOVE_AUTO_ADAPT=report datalove run example.dfs
```

---

## Implementation Order

### Phase 1: Diagnostics Enhancement
1. Add `RecoveryHint` to `PendingDiagnostic`
2. Compute hints in `TypeContext::error_type_mismatch()`
3. Emit help notes in `emit_pending_diagnostics()`
4. Add first-move tracking to ownership errors
5. Emit help notes for ownership errors

### Phase 2: Auto-Adapt Mode
1. Add `AutoAdaptMode` enum and env reading
2. Thread through `CompileOptions`
3. Add to `TypeContext`
4. Implement type-checking auto-adapt
5. Add `AutoAdapted` diagnostic variant
6. Implement ownership auto-adapt
7. Communicate adaptations to IR lowering

### Phase 3: Test Suite
1. Create `fixtures/auto-adapt/` directory
2. Create test runner in `auto_adapt_tests.rs`
3. Add widening test cases
4. Add clone test cases
5. Add ownership test cases
6. Add edge case tests

### Phase 4: CLI Integration
1. Add flags to CLI commands
2. Wire up to pipeline
3. Document in help text

---

## Open Questions

1. **AST Rewriting vs IR-level**: Should auto-adapt rewrite the AST (inserting
   actual `@` nodes) or handle at IR level? IR-level is simpler but AST rewriting
   would allow "show me the fixed code" tooling.

2. **Multiple Adaptations**: If the same expression needs both clone and widen
   (e.g., `string` variable used twice where `int` expected), how to report?
   Currently impossible since strings don't widen to int.

3. **Partial Success**: If some errors are recoverable and some aren't, should
   auto-adapt proceed with the recoverable ones? Current design: yes, but still
   fail overall due to unrecoverable errors.

4. **IDE Integration**: Should there be a machine-readable output format for
   IDEs to offer quick-fixes? JSON diagnostics already support this structure.
