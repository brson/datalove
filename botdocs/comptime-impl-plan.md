# Comptime Arguments Implementation Plan

## Union-Branch Approach for `const` Parameters

This document provides a detailed implementation plan for adding Zig-style comptime arguments
to datalove using the **union-branch specialization** strategy.

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Architecture Overview](#architecture-overview)
3. [Phase 1: AST & Parsing](#phase-1-ast--parsing)
4. [Phase 2: Type System](#phase-2-type-system)
5. [Phase 3: Comptime Value Collection](#phase-3-comptime-value-collection)
6. [Phase 4: Union-Branch Transformation](#phase-4-union-branch-transformation)
7. [Phase 5: IR & Lowering Integration](#phase-5-ir--lowering-integration)
8. [Phase 6: Codegen Optimization](#phase-6-codegen-optimization)
9. [Testing Strategy](#testing-strategy)
10. [Risk Mitigation](#risk-mitigation)
11. [Future Extensions](#future-extensions)

---

## Executive Summary

### Goal

Add `const` parameter modifier enabling compile-time known arguments:

```
fun repeat(const n: i32, s: string) -> string
    // n is known at compile time, enabling optimization
end fun

let x = repeat(3, "ab")  // n=3 is evaluated at compile time
```

### Strategy: Union-Branch Specialization

Instead of generating N separate functions (full monomorphization), generate **one function
with N branches**, dispatching on an enum tag:

```
// Generated internal representation
enum Comptime_repeat_n { N_3, N_5, N_10 }

fun repeat_unified(n_tag: Comptime_repeat_n, s: string) -> string
    match n_tag
        N_3 =>
            const n = 3
            // body with n=3 const-folded
        N_5 =>
            const n = 5
            // body with n=5 const-folded
        N_10 =>
            const n = 10
            // body with n=10 const-folded
    end match
end fun
```

### Why Union-Branch

| Aspect | Full Mono | Union-Branch |
|--------|-----------|--------------|
| Code size | O(N × func) | O(func + N × branch) |
| Compile time | O(N × func) | O(func + N) |
| Icache | Poor (N copies) | Good (1 function) |
| Branch cost | None | ~2-5 cycles |
| Const folding | Full | Full (per branch) |

For a scripting language prioritizing compile time, union-branch is the better tradeoff.

---

## Architecture Overview

### Pipeline Integration Point

```
┌─────────────────────────────────────────────────────────────────────────┐
│                     Current Pipeline                                     │
├─────────────────────────────────────────────────────────────────────────┤
│  Parse → Resolve → Typecheck → Ownership → Lower → [CTFE] → Assemble    │
└─────────────────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                     New Pipeline                                         │
├─────────────────────────────────────────────────────────────────────────┤
│  Parse → Resolve → Typecheck → [SPECIALIZE] → Ownership → Lower → ...   │
│                                     │                                    │
│                          ┌──────────┴──────────┐                        │
│                          │ Comptime Collection │                        │
│                          │ Value Evaluation    │                        │
│                          │ Union-Branch Gen    │                        │
│                          │ Call Rewriting      │                        │
│                          └─────────────────────┘                        │
└─────────────────────────────────────────────────────────────────────────┘
```

The specialization pass runs **after typecheck, before ownership analysis**:
- Has full type information (can verify const-ness)
- Modifies AST before ownership sees it
- Lowering remains unchanged (just sees more functions)

### New Crate: `datalove-datafun-specialize`

```
datalove-datafun-specialize/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API, Salsa integration
│   ├── collect.rs          # Phase 1: Collect comptime call sites
│   ├── evaluate.rs         # Phase 2: Evaluate comptime arguments
│   ├── transform.rs        # Phase 3: Generate union-branch functions
│   ├── rewrite.rs          # Phase 4: Rewrite call sites
│   ├── types.rs            # Data structures
│   └── tests.rs            # Unit tests
```

### Key Data Structures

```rust
/// Identifies a comptime-param function
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct ComptimeFuncId {
    pub module_id: Option<ModuleId>,
    pub func_name: String,
}

/// All comptime values seen for a parameter across call sites
#[derive(Clone, Debug)]
pub struct ComptimeValueSet {
    pub param_index: usize,
    pub param_name: String,
    pub param_type: IrType,
    pub values: Vec<ConstValue>,  // deduplicated, sorted
}

/// Complete specialization info for one function
#[derive(Clone, Debug)]
pub struct ComptimeFuncSpec {
    pub func_id: ComptimeFuncId,
    pub original_func: StmtFun,
    pub comptime_params: Vec<usize>,  // indices of const params
    pub value_sets: Vec<ComptimeValueSet>,
}

/// Mapping from (func, comptime_args) to enum variant
#[derive(Clone, Debug)]
pub struct SpecializationMap {
    /// For each comptime function, the generated enum type and variant mapping
    pub funcs: HashMap<ComptimeFuncId, FuncSpecialization>,
}

#[derive(Clone, Debug)]
pub struct FuncSpecialization {
    /// The enum type for this function's comptime params
    pub enum_type: IrType,
    /// Map from comptime arg values to enum variant index
    pub value_to_variant: HashMap<Vec<ConstValue>, u32>,
    /// The transformed function (with match dispatch)
    pub transformed_func: StmtFun,
}

/// Result of specialization pass
pub struct SpecializationResult<'db> {
    /// New/modified functions to add to module
    pub new_functions: Vec<StmtFun<'db>>,
    /// Functions to remove (replaced by specialized versions)
    pub removed_functions: Vec<StmtFun<'db>>,
    /// Call site rewrites: (call_site_id, new_args)
    pub call_rewrites: HashMap<salsa::Id, CallRewrite>,
}
```

---

## Phase 1: AST & Parsing

### 1.1 Extend FunParam

**File**: `datalove-datafun-ast/src/ast.rs`

```rust
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct FunParam<'db> {
    pub name: InternedText<'db>,
    pub mode: ParamMode,
    pub is_comptime: bool,  // NEW: true if `const` modifier present
    pub type_hint: datalit::ast::TypeHint<'db>,
}
```

### 1.2 Add `const` Keyword

**File**: `datalove-datafun-parser/src/lexer.rs`

The `const` keyword already exists for const bindings. Verify it's in the keyword list.

### 1.3 Parse `const` Parameter Modifier

**File**: `datalove-datafun-parser/src/statement.rs`

Modify `parse_fun_param()`:

```rust
fn parse_fun_param(&mut self) -> Result<FunParam<'db>, ParseError> {
    // NEW: Check for `const` modifier first
    let is_comptime = if self.check(TokenKind::Keyword)
        && self.current_text() == "const"
    {
        self.advance();
        true
    } else {
        false
    };

    // Existing mode parsing (in, out, ref, mut)
    let mode = self.parse_param_mode();

    // Parameter name
    let name = self.expect_ident()?;

    // Colon and type
    self.expect(TokenKind::Colon)?;
    let type_hint = self.parse_type_hint()?;

    Ok(FunParam {
        name,
        mode,
        is_comptime,  // NEW
        type_hint,
    })
}
```

**Syntax examples**:
```
fun foo(const n: i32)              // const by-value
fun bar(const ref data: [i32])     // const reference (less common)
fun baz(n: i32, const mode: i32)   // mixed params
```

### 1.4 Validation Rules

Add validation in parser or early typecheck:

1. **Const params must have supported types**: primitives, strings, simple aggregates
2. **Const params cannot be `out` or `mut`**: `const out x` is nonsensical
3. **Const params should come first** (convention, not required)

**File**: `datalove-datafun-tycheck/src/lib.rs` (add validation)

```rust
fn validate_comptime_param(param: &FunParam) -> Result<(), TypeError> {
    if param.is_comptime {
        // Cannot combine const with out or mut
        if matches!(param.mode, ParamMode::Out | ParamMode::Mut) {
            return Err(TypeError::InvalidComptimeMode {
                param_name: param.name.clone(),
            });
        }
        // Type must be evaluable at compile time
        if !is_ctfe_supported_type(&param.type_hint) {
            return Err(TypeError::UnsupportedComptimeType {
                param_name: param.name.clone(),
            });
        }
    }
    Ok(())
}
```

---

## Phase 2: Type System

### 2.1 Extend TypeFunction

**File**: `datalove-datafun-common/src/lib.rs`

```rust
#[salsa::tracked]
pub struct TypeFunction<'db> {
    #[returns(ref)]
    pub param_types: Vec<Type<'db>>,
    #[returns(ref)]
    pub param_modes: Vec<ParamMode>,
    #[returns(ref)]
    pub param_comptime: Vec<bool>,  // NEW: which params are comptime
    pub return_type: Type<'db>,
}
```

### 2.2 Update Function Type Construction

**File**: `datalove-datafun-tycheck/src/context.rs`

When building `TypeFunction` from `StmtFun`:

```rust
fn build_function_type(&self, func: StmtFun<'db>) -> TypeFunction<'db> {
    let params = func.params(self.db);
    TypeFunction::new(
        self.db,
        params.iter().map(|p| self.resolve_type(&p.type_hint)).collect(),
        params.iter().map(|p| p.mode).collect(),
        params.iter().map(|p| p.is_comptime).collect(),  // NEW
        self.resolve_return_type(func.return_type(self.db)),
    )
}
```

### 2.3 Comptime Argument Checking

**File**: `datalove-datafun-tycheck/src/synthesize.rs`

Extend `synthesize_function_call`:

```rust
fn synthesize_function_call<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    call: ExprFunctionCall<'db>,
) -> Result<Type<'db>, TypeError> {
    let func_type = ctx.lookup_function(call.name(ctx.db))?;
    let param_comptime = func_type.param_comptime(ctx.db);

    // Existing argument checking...

    // NEW: Validate comptime arguments
    for (i, (arg, &is_comptime)) in call.args(ctx.db).iter()
        .zip(param_comptime.iter())
        .enumerate()
    {
        if is_comptime {
            // Argument must be evaluable at compile time
            if !ctx.is_const_evaluable(*arg) {
                return Err(TypeError::ComptimeArgNotConst {
                    func_name: call.name(ctx.db).text(ctx.db).to_string(),
                    param_index: i,
                    arg_expr: *arg,
                });
            }

            // Store evaluated value for specialization pass
            let const_val = ctx.evaluate_comptime_arg(*arg)?;
            ctx.record_comptime_call(call, i, const_val);
        }
    }

    // ... rest of type checking
}
```

### 2.4 Const Evaluability Check

```rust
impl<'db> TypeContext<'db> {
    /// Check if expression can be evaluated at compile time
    fn is_const_evaluable(&self, expr: ExprFun<'db>) -> bool {
        match expr.expr(self.db) {
            // Literals are always const
            ExprFunKind::Literal(_) => true,

            // Names are const if they refer to const bindings
            ExprFunKind::Name(name) => {
                self.is_const_binding(name)
            }

            // Operators on const operands are const
            ExprFunKind::BinOp(op) => {
                self.is_const_evaluable(op.lhs(self.db)) &&
                self.is_const_evaluable(op.rhs(self.db))
            }

            // Function calls to pure functions with const args
            ExprFunKind::FunctionCall(call) => {
                self.is_pure_function(call.name(self.db)) &&
                call.args(self.db).iter().all(|a| self.is_const_evaluable(*a))
            }

            // Tuples/structs with const fields
            ExprFunKind::Tuple(t) => {
                t.elements(self.db).iter().all(|e| self.is_const_evaluable(*e))
            }

            _ => false,
        }
    }
}
```

---

## Phase 3: Comptime Value Collection

### 3.1 Collection Pass

**File**: `datalove-datafun-specialize/src/collect.rs`

```rust
/// Collect all comptime call sites in a module graph
pub fn collect_comptime_calls<'db>(
    db: &'db dyn Database,
    modules: &[Module<'db>],
    typecheck_result: &ModuleGraphTypecheckResult<'db>,
) -> ComptimeCallCollection<'db> {
    let mut collection = ComptimeCallCollection::new();

    for module in modules {
        let parsed = parse_module_ast(db, *module);
        collect_from_statements(db, &parsed.statements, &mut collection);
    }

    collection
}

fn collect_from_statements<'db>(
    db: &'db dyn Database,
    stmts: &[Statement<'db>],
    collection: &mut ComptimeCallCollection<'db>,
) {
    for stmt in stmts {
        match stmt {
            Statement::Fun(func) => {
                // Check if this function HAS comptime params
                let has_comptime = func.params(db).iter().any(|p| p.is_comptime);
                if has_comptime {
                    collection.register_comptime_func(*func);
                }
                // Recurse into body
                collect_from_statements(db, func.body(db), collection);
            }
            Statement::Let(let_stmt) => {
                collect_from_expr(db, let_stmt.value(db), collection);
            }
            Statement::If(if_stmt) => {
                collect_from_expr(db, if_stmt.condition(db), collection);
                collect_from_statements(db, if_stmt.then_body(db), collection);
                if let Some(else_body) = if_stmt.else_body(db) {
                    collect_from_statements(db, else_body, collection);
                }
            }
            // ... other statement types
        }
    }
}

fn collect_from_expr<'db>(
    db: &'db dyn Database,
    expr: ExprFun<'db>,
    collection: &mut ComptimeCallCollection<'db>,
) {
    match expr.expr(db) {
        ExprFunKind::FunctionCall(call) => {
            // Check if callee has comptime params
            if let Some(func_spec) = collection.get_comptime_func(call.name(db)) {
                // Evaluate comptime arguments
                let comptime_args = evaluate_comptime_args(db, call, func_spec);
                collection.record_call_site(call, comptime_args);
            }
            // Recurse into arguments
            for arg in call.args(db) {
                collect_from_expr(db, *arg, collection);
            }
        }
        ExprFunKind::BinOp(op) => {
            collect_from_expr(db, op.lhs(db), collection);
            collect_from_expr(db, op.rhs(db), collection);
        }
        // ... other expression types
    }
}
```

### 3.2 ComptimeCallCollection Structure

```rust
pub struct ComptimeCallCollection<'db> {
    /// Functions that have comptime parameters
    comptime_funcs: HashMap<InternedText<'db>, ComptimeFuncInfo<'db>>,

    /// All call sites to comptime functions, grouped by callee
    call_sites: HashMap<InternedText<'db>, Vec<ComptimeCallSite<'db>>>,

    /// Unique (func, comptime_values) combinations seen
    instantiations: HashMap<InternedText<'db>, HashSet<Vec<ConstValue>>>,
}

pub struct ComptimeFuncInfo<'db> {
    pub func: StmtFun<'db>,
    pub comptime_param_indices: Vec<usize>,
}

pub struct ComptimeCallSite<'db> {
    pub call_expr: ExprFunctionCall<'db>,
    pub comptime_arg_values: Vec<ConstValue>,
    pub containing_func: Option<StmtFun<'db>>,
}
```

### 3.3 Comptime Argument Evaluation

**File**: `datalove-datafun-specialize/src/evaluate.rs`

Reuse existing CTFE infrastructure:

```rust
pub fn evaluate_comptime_args<'db>(
    db: &'db dyn Database,
    call: ExprFunctionCall<'db>,
    func_spec: &ComptimeFuncInfo<'db>,
    evaluator: &mut dyn CtfeEvaluator,
) -> Vec<ConstValue> {
    let args = call.args(db);
    let mut comptime_values = Vec::new();

    for &param_idx in &func_spec.comptime_param_indices {
        let arg_expr = args[param_idx];

        // Try fast path: literal extraction
        if let Some(val) = try_extract_literal(db, arg_expr) {
            comptime_values.push(val);
            continue;
        }

        // Slow path: full CTFE evaluation
        let ir_unit = lower_const_expr_to_unit(db, arg_expr);
        let param = &func_spec.func.params(db)[param_idx];
        let result_type = IrType::from_type_hint(db, &param.type_hint);

        let value = evaluator.evaluate(&ir_unit, &result_type)
            .expect("comptime arg evaluation failed");
        comptime_values.push(value);
    }

    comptime_values
}
```

---

## Phase 4: Union-Branch Transformation

### 4.1 Enum Type Generation

**File**: `datalove-datafun-specialize/src/transform.rs`

```rust
/// Generate the enum type for a function's comptime parameters
fn generate_comptime_enum<'db>(
    db: &'db dyn Database,
    func_name: &str,
    instantiations: &HashSet<Vec<ConstValue>>,
) -> (IrType, HashMap<Vec<ConstValue>, u32>) {
    // Sort instantiations for deterministic ordering
    let mut sorted: Vec<_> = instantiations.iter().collect();
    sorted.sort_by(|a, b| compare_const_value_vecs(a, b));

    // Generate variant names
    let mut variants = Vec::new();
    let mut value_to_variant = HashMap::new();

    for (idx, values) in sorted.iter().enumerate() {
        let variant_name = format!("V{}", idx);  // V0, V1, V2, ...
        variants.push((variant_name, None));  // No payload - values are const-folded
        value_to_variant.insert((*values).clone(), idx as u32);
    }

    let enum_type = IrType::Enum(variants);
    (enum_type, value_to_variant)
}
```

### 4.2 Function Body Transformation

```rust
/// Transform a comptime-param function into union-branch form
pub fn transform_to_union_branch<'db>(
    db: &'db dyn Database,
    func: StmtFun<'db>,
    comptime_params: &[usize],
    instantiations: &HashSet<Vec<ConstValue>>,
) -> StmtFun<'db> {
    let (enum_type, value_to_variant) = generate_comptime_enum(
        db,
        func.name(db).text(db),
        instantiations,
    );

    // Build new parameter list: replace comptime params with single tag
    let old_params = func.params(db);
    let mut new_params = Vec::new();

    // Add enum tag parameter
    let tag_param = FunParam {
        name: intern_text(db, "__comptime_tag"),
        mode: ParamMode::In,
        is_comptime: false,  // Tag is a regular runtime value
        type_hint: enum_type_to_hint(&enum_type),
    };
    new_params.push(tag_param);

    // Add non-comptime params unchanged
    for (i, param) in old_params.iter().enumerate() {
        if !comptime_params.contains(&i) {
            new_params.push(param.clone());
        }
    }

    // Build match body
    let match_body = build_union_branch_body(
        db,
        func.body(db),
        comptime_params,
        &old_params,
        instantiations,
        &value_to_variant,
    );

    // Create new function
    StmtFun::new(
        db,
        func.module_id(db),
        func.name(db),  // Keep same name for simplicity
        new_params,
        func.return_type(db),
        match_body,
        func.local_index(db),
    )
}
```

### 4.3 Match Body Generation

Since `match` is unimplemented, we generate nested if-else chains:

```rust
fn build_union_branch_body<'db>(
    db: &'db dyn Database,
    original_body: &[Statement<'db>],
    comptime_params: &[usize],
    old_params: &[FunParam<'db>],
    instantiations: &HashSet<Vec<ConstValue>>,
    value_to_variant: &HashMap<Vec<ConstValue>, u32>,
) -> Vec<Statement<'db>> {
    let mut sorted: Vec<_> = instantiations.iter().collect();
    sorted.sort_by(|a, b| compare_const_value_vecs(a, b));

    // Generate nested if-else for each variant
    // if __comptime_tag == V0 then
    //     const p1 = v0_1; const p2 = v0_2; ...
    //     <original body>
    // else if __comptime_tag == V1 then
    //     const p1 = v1_1; const p2 = v1_2; ...
    //     <original body>
    // ...
    // end if

    build_variant_chain(db, &sorted, comptime_params, old_params, original_body, 0)
}

fn build_variant_chain<'db>(
    db: &'db dyn Database,
    variants: &[&Vec<ConstValue>],
    comptime_params: &[usize],
    old_params: &[FunParam<'db>],
    original_body: &[Statement<'db>],
    current_idx: usize,
) -> Vec<Statement<'db>> {
    if current_idx >= variants.len() {
        // Unreachable case - panic or return error
        return vec![build_panic_stmt(db, "invalid comptime variant")];
    }

    let values = variants[current_idx];

    // Build condition: __comptime_tag == V{current_idx}
    let condition = build_enum_check(db, current_idx);

    // Build then-body: const bindings + original body
    let mut then_body = Vec::new();
    for (i, &param_idx) in comptime_params.iter().enumerate() {
        let param = &old_params[param_idx];
        let const_stmt = Statement::Const(StmtConst::new(
            db,
            param.name,
            Some(param.type_hint.clone()),
            const_value_to_expr(db, &values[i]),
        ));
        then_body.push(const_stmt);
    }
    then_body.extend(original_body.iter().cloned());

    // Build else-body: next variant or unreachable
    let else_body = if current_idx + 1 < variants.len() {
        Some(build_variant_chain(db, variants, comptime_params, old_params,
                                  original_body, current_idx + 1))
    } else {
        None
    };

    vec![Statement::If(StmtIf::new(
        db,
        condition,
        None,  // no binding
        then_body,
        else_body,
        0,
    ))]
}
```

### 4.4 Enum Comparison Without Match

Since match is unimplemented, we use intrinsics or comparison:

```rust
fn build_enum_check<'db>(db: &'db dyn Database, variant_idx: usize) -> ExprFun<'db> {
    // Build: __comptime_tag == enum V{variant_idx}
    //
    // Implementation options:
    // 1. If enums support == comparison, use that
    // 2. Use intrinsic to get discriminant and compare
    // 3. Use pattern: if __comptime_tag |V{idx}| then ... (if-binding on enum)

    // Option 3 is cleanest if enum if-binding works:
    // Actually, we'd generate:
    //   if enum_discriminant(__comptime_tag) == {variant_idx}

    ExprFun::new(db, ExprFunKind::BinOp(ExprBinOp::new(
        db,
        BinOp::Eq,
        build_discriminant_call(db, "__comptime_tag"),
        build_literal(db, variant_idx as i64),
    )))
}
```

**Note**: May need to add `enum_discriminant` intrinsic or use existing comparison.

---

## Phase 5: IR & Lowering Integration

### 5.1 Call Site Rewriting

**File**: `datalove-datafun-specialize/src/rewrite.rs`

```rust
/// Rewrite all call sites to comptime functions
pub fn rewrite_call_sites<'db>(
    db: &'db dyn Database,
    modules: &mut [ParsedModule<'db>],
    collection: &ComptimeCallCollection<'db>,
    spec_map: &SpecializationMap,
) {
    for module in modules {
        rewrite_module_calls(db, module, collection, spec_map);
    }
}

fn rewrite_call<'db>(
    db: &'db dyn Database,
    call: ExprFunctionCall<'db>,
    collection: &ComptimeCallCollection<'db>,
    spec_map: &SpecializationMap,
) -> ExprFunctionCall<'db> {
    let func_name = call.name(db);

    // Get specialization info
    let func_spec = match spec_map.funcs.get(&func_name.text(db).to_string()) {
        Some(spec) => spec,
        None => return call,  // Not a comptime function
    };

    // Get comptime arg values for this call site
    let call_site = collection.get_call_site(call)
        .expect("call site should be recorded");

    // Look up variant index
    let variant_idx = func_spec.value_to_variant
        .get(&call_site.comptime_arg_values)
        .expect("instantiation should exist");

    // Build new argument list: [enum_variant, non-comptime args...]
    let old_args = call.args(db);
    let mut new_args = Vec::new();

    // Add enum variant constructor
    let variant_expr = build_enum_variant(db, &func_spec.enum_type, *variant_idx);
    new_args.push(variant_expr);

    // Add non-comptime args
    let comptime_indices: HashSet<_> = collection.get_comptime_func(func_name)
        .unwrap()
        .comptime_param_indices
        .iter()
        .cloned()
        .collect();

    for (i, arg) in old_args.iter().enumerate() {
        if !comptime_indices.contains(&i) {
            new_args.push(*arg);
        }
    }

    // Create new call expression
    ExprFunctionCall::new(db, func_name, new_args)
}
```

### 5.2 Integration with Lowering

The transformed functions and rewritten calls go through normal lowering:

1. **Enum construction** (`EnumVariant` instruction) for call site args
2. **Branch dispatch** in function body via if-else lowering
3. **Const bindings** within each branch are CTFE-evaluated and inlined

The existing CTFE pipeline handles the per-branch const folding automatically.

### 5.3 Salsa Integration

**File**: `datalove-datafun-compiler/src/tracked_specialize.rs`

```rust
/// Tracked function for specialization
#[salsa::tracked]
pub fn specialize_module_graph<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
) -> SpecializationResult<'db> {
    // Collection phase
    let collection = collect_comptime_calls(db, parsed_graph.modules(), typecheck_result);

    // Skip if no comptime functions
    if collection.is_empty() {
        return SpecializationResult::empty();
    }

    // Evaluation phase
    let evaluator = create_ctfe_evaluator();
    let evaluated = evaluate_all_comptime_args(db, &collection, &mut evaluator);

    // Transformation phase
    let spec_map = transform_comptime_funcs(db, &collection, &evaluated);

    // Rewriting phase
    let rewrites = compute_call_rewrites(db, &collection, &spec_map);

    SpecializationResult {
        transformed_funcs: spec_map.into_transformed_funcs(),
        call_rewrites: rewrites,
    }
}
```

---

## Phase 6: Codegen Optimization

### 6.1 Branch Optimization Opportunities

The Cranelift backend can optimize the generated code:

1. **Constant propagation**: Within each branch, comptime values are constants
2. **Dead code elimination**: Branches not taken based on const conditions
3. **Switch lowering**: Convert if-else chain to jump table if many variants

### 6.2 Consider Adding Jump Table Hint

For many variants (>4), hint to backend to use jump table:

```rust
// In codegen, when we see enum dispatch pattern:
if is_comptime_dispatch(terminator) && num_variants > 4 {
    emit_jump_table(variants);
} else {
    emit_if_else_chain(variants);
}
```

### 6.3 Inlining Considerations

After transformation:
- The unified function is larger but single
- Inlining at call sites benefits from const args in tag
- Consider marking small comptime functions for inlining

---

## Testing Strategy

### Unit Tests

**Location**: `datalove-datafun-specialize/src/tests.rs`

```rust
#[test]
fn test_collect_comptime_calls() {
    let source = r#"
        fun repeat(const n: i32, s: string) -> string
            // body
        end fun

        let a = repeat(3, "x")
        let b = repeat(5, "y")
    "#;

    let collection = collect_from_source(source);
    assert_eq!(collection.instantiations("repeat").len(), 2);
    assert!(collection.has_values("repeat", &[ConstValue::I32(3)]));
    assert!(collection.has_values("repeat", &[ConstValue::I32(5)]));
}

#[test]
fn test_union_branch_transform() {
    // Verify transformation produces valid AST
}

#[test]
fn test_call_rewriting() {
    // Verify call sites are correctly rewritten
}
```

### Integration Tests

**Location**: `datalove-datafun/tests/fixtures/specialize/`

```
001_simple_const_param.dfs
001_simple_const_param.out.expected

002_multiple_instantiations.dfs
003_nested_comptime_calls.dfs
004_comptime_in_module.world
005_mixed_const_nonconst.dfs
```

### Dual-Mode Tests

Extend existing dual tests to compare:
- Interpreter output
- AOT output
- Verify identical results with comptime args

---

## Risk Mitigation

### Risk 1: Compile Time Regression

**Mitigation**:
- Lazy evaluation: only process files with comptime functions
- Memoization via Salsa
- Benchmark suite with comptime-heavy code

### Risk 2: Code Size Explosion

**Mitigation**:
- Instance limit (configurable, default 64)
- Warning when approaching limit
- Option to fall back to error rather than degrade

### Risk 3: Complex Interaction with Existing Passes

**Mitigation**:
- Insert specialization cleanly between typecheck and ownership
- Transformed AST should type-check cleanly
- Add validation pass to verify transformation correctness

### Risk 4: Enum Discriminant Access

**Mitigation**:
- Add `enum_discriminant` intrinsic if needed
- Or use existing comparison operators on enums
- Or generate if-binding pattern

### Risk 5: Debugging Complexity

**Mitigation**:
- Preserve source locations in transformed code
- Add `#[comptime_specialized]` attribute to generated functions
- Debug mode that shows original function + instantiations

---

## Future Extensions

### Phase B: Type as ConstValue

Add `ConstValue::Type(IrType)` for Zig-style type parameters:

```
fun identity(const T: type, x: T) -> T
    x
end fun
```

### Phase C: Type Computation

Enable functions returning types:

```
fun Pair(const A: type, const B: type) -> type
    {first: A, second: B}
end fun
```

### Phase D: Comptime Blocks

Arbitrary compile-time execution:

```
fun foo()
    comptime
        // Arbitrary code here, executed at compile time
    end comptime
end fun
```

---

## Implementation Order

### Sprint 1: Foundation (3-5 days)
- [ ] Phase 1: AST & Parsing
- [ ] Phase 2: Type System basics
- [ ] Basic validation tests

### Sprint 2: Collection & Evaluation (3-4 days)
- [ ] Phase 3: Collection pass
- [ ] CTFE integration for arg evaluation
- [ ] Unit tests

### Sprint 3: Transformation (5-7 days)
- [ ] Phase 4: Union-branch generation
- [ ] Enum type construction
- [ ] If-else chain generation
- [ ] Integration tests

### Sprint 4: Integration (4-5 days)
- [ ] Phase 5: Call site rewriting
- [ ] Salsa integration
- [ ] Pipeline integration
- [ ] End-to-end tests

### Sprint 5: Polish (3-4 days)
- [ ] Phase 6: Codegen optimization
- [ ] Error messages
- [ ] Documentation
- [ ] Performance benchmarks

**Total Estimate**: 18-25 days

---

## Appendix: Key Files to Modify

| File | Changes |
|------|---------|
| `datafun-ast/src/ast.rs` | Add `is_comptime` to FunParam |
| `datafun-parser/src/statement.rs` | Parse `const` modifier |
| `datafun-common/src/lib.rs` | Add `param_comptime` to TypeFunction |
| `datafun-tycheck/src/synthesize.rs` | Validate comptime args |
| `datafun-tycheck/src/context.rs` | Track comptime call info |
| `datafun-compiler/src/compile.rs` | Insert specialization phase |
| `datafun-compiler/src/lib.rs` | Export new tracked functions |
| `datafun-ir/src/lib.rs` | (Maybe) enum discriminant helpers |

| New File | Purpose |
|----------|---------|
| `datafun-specialize/src/lib.rs` | Crate root, public API |
| `datafun-specialize/src/collect.rs` | Collection pass |
| `datafun-specialize/src/evaluate.rs` | CTFE integration |
| `datafun-specialize/src/transform.rs` | Union-branch generation |
| `datafun-specialize/src/rewrite.rs` | Call site rewriting |
| `datafun-specialize/src/types.rs` | Data structures |
