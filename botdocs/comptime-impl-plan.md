# Comptime Arguments Implementation Plan

## Union-Branch Approach for `const` Parameters

This document provides a detailed implementation plan for adding Zig-style comptime arguments
to datalove using the **union-branch specialization** strategy.

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Architecture Overview](#architecture-overview)
3. [Phase 1: AST & Parsing](#phase-1-ast--parsing)
4. [Phase 2: Type System](#phase-2-type-system)
5. [Phase 3: Comptime Call Site Recording](#phase-3-comptime-call-site-recording)
6. [Phase 4: IR-Level Specialization](#phase-4-ir-level-specialization)
7. [Phase 5: Const Folding Within Branches](#phase-5-const-folding-within-branches)
8. [Phase 6: Codegen Optimization](#phase-6-codegen-optimization)
9. [Testing Strategy](#testing-strategy)
10. [Risk Mitigation](#risk-mitigation)
11. [Future Extensions](#future-extensions)
12. [Implementation Order](#implementation-order)
13. [Appendix: Key Files to Modify](#appendix-key-files-to-modify)

---

## Executive Summary

### Goal

Add `const` parameter modifier enabling compile-time known arguments:

```
fun repeat(const n: i32, s: string) -> string
    // n is known at compile time, enabling optimization
end fun

const N = 3
let x = repeat(N, "ab")  // N is a const binding, value looked up
```

### Initial Restriction: Const-Binding-Only Arguments

To avoid complexity with CTFE ordering, comptime arguments must be **const binding names**:

```
const N = 5
const MODE = 2

fun foo(const n: i32, x: string) -> string ...

let a = foo(N, "hello")     // ✓ N is a const binding
let b = foo(MODE, "world")  // ✓ MODE is a const binding
let c = foo(3, "x")         // ✗ ERROR: literal not allowed (for now)
let d = foo(1 + 2, "y")     // ✗ ERROR: expression not allowed
```

This restriction means **no additional CTFE is needed during specialization**—we just
look up already-evaluated const values from `ResolvedConsts`.

### Strategy: Union-Branch Specialization

Instead of generating N separate functions (full monomorphization), generate **one function
with N branches**, dispatching on an enum tag:

```
// Generated internal representation
enum Comptime_repeat_n { V0, V1, V2 }  // variants for N=3, N=5, N=10

fun repeat_unified(n_tag: Comptime_repeat_n, s: string) -> string
    if discriminant(n_tag) == 0
        const n = 3
        // body with n=3 const-folded
    else if discriminant(n_tag) == 1
        const n = 5
        // body with n=5 const-folded
    else
        const n = 10
        // body with n=10 const-folded
    end if
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

### Key Insight: Specialization Within Lowering Phase

Specialization happens **inside the lowering phase**, after const evaluation:

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Parse → Resolve → Typecheck → Ownership → Lower (Phase 5)              │
│                         │                      │                        │
│              [record comptime           ┌──────┴──────┐                 │
│               call sites]               │ 5a: Lower   │                 │
│                                         │ 5b: Eval    │ ← consts evaluated
│                                         │ 5c: SPECIAL │ ← NEW: transform IR
│                                         │ 5d: Assemble│                 │
│                                         └─────────────┘                 │
└─────────────────────────────────────────────────────────────────────────┘
```

### Why This Ordering Works

1. **Typecheck** records which call sites have comptime args (just names, no values yet)
2. **Phase 5a** lowers all functions to IR (including comptime-param functions)
3. **Phase 5b** evaluates all const bindings → `ResolvedConsts`
4. **Phase 5c (NEW)** specializes:
   - Look up comptime arg values from `ResolvedConsts` (no CTFE needed!)
   - Transform IR functions to union-branch form
   - Rewrite call instructions
5. **Phase 5d** assembles and inlines consts

**No duplicate lowering or CTFE** — const values are already computed in 5b.

### New Module: `datalove-datafun-specialize`

Can be a new crate or a module within `datalove-datafun-lower`:

```
datalove-datafun-specialize/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API
│   ├── transform.rs        # IR transformation to union-branch
│   ├── rewrite.rs          # Call instruction rewriting
│   └── types.rs            # Data structures
```

### Key Data Structures

```rust
/// Recorded during typecheck: a call site with comptime args
#[derive(Clone, Debug)]
pub struct ComptimeCallSite<'db> {
    /// The call expression (for locating in IR later)
    pub call_expr_id: salsa::Id,
    /// Name of the called function
    pub func_name: InternedText<'db>,
    /// Indices of comptime parameters in the callee
    pub comptime_param_indices: Vec<usize>,
    /// Names of const bindings used as comptime args (NOT values yet)
    pub comptime_arg_names: Vec<InternedText<'db>>,
}

/// Collected during typecheck for a module
#[derive(Clone, Debug, Default)]
pub struct ComptimeCallSiteRegistry<'db> {
    /// All call sites with comptime args
    pub call_sites: Vec<ComptimeCallSite<'db>>,
    /// Functions that have comptime parameters
    pub comptime_funcs: HashMap<InternedText<'db>, Vec<usize>>,  // name → param indices
}

/// Built during specialization (phase 5c) after const eval
#[derive(Clone, Debug)]
pub struct ResolvedComptimeCall {
    /// The call site
    pub call_site_id: salsa::Id,
    /// Resolved values (looked up from ResolvedConsts)
    pub comptime_values: Vec<ConstValue>,
}

/// Specialization info for one comptime-param function
#[derive(Clone, Debug)]
pub struct FuncSpecialization {
    /// Original function's FuncId
    pub original_func_id: FuncId,
    /// The enum type for dispatch
    pub enum_type: IrType,
    /// Map from comptime values to variant index
    pub value_to_variant: HashMap<Vec<ConstValue>, u32>,
    /// All unique instantiations
    pub instantiations: Vec<Vec<ConstValue>>,
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

### 2.4 Const Evaluability Check (Simplified)

With the const-binding-only restriction, this check is simple:

```rust
impl<'db> TypeContext<'db> {
    /// Check if expression is a const binding name (our initial restriction)
    fn is_const_binding_arg(&self, expr: ExprFun<'db>) -> Option<InternedText<'db>> {
        match expr.expr(self.db) {
            ExprFunKind::Name(name) if self.is_const_binding(name) => Some(name),
            _ => None,
        }
    }
}
```

**Note**: This intentionally rejects literals like `foo(3, x)` even though `3` is
obviously compile-time known. The restriction simplifies the implementation by
ensuring all comptime values are already in `ResolvedConsts`. Future extensions
can relax this to allow literals and expressions.

---

## Phase 3: Comptime Call Site Recording

Recording happens **during typecheck**, not as a separate pass. This is lightweight—
we just record const binding names, not values.

### 3.1 Extend Typecheck Context

**File**: `datalove-datafun-tycheck/src/context.rs`

```rust
impl<'db> TypeContext<'db> {
    /// Registry of comptime call sites discovered during typecheck
    pub comptime_registry: ComptimeCallSiteRegistry<'db>,
}
```

### 3.2 Record During Call Synthesis

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

    // Check for comptime parameters
    let comptime_indices: Vec<usize> = param_comptime.iter()
        .enumerate()
        .filter_map(|(i, &is_ct)| if is_ct { Some(i) } else { None })
        .collect();

    if !comptime_indices.is_empty() {
        // Record this function has comptime params
        ctx.comptime_registry.comptime_funcs
            .entry(call.name(ctx.db))
            .or_insert_with(|| comptime_indices.clone());

        // Validate and record comptime arguments
        let mut arg_names = Vec::new();
        for &i in &comptime_indices {
            let arg = call.args(ctx.db)[i];
            let name = validate_comptime_arg(ctx, arg, i)?;
            arg_names.push(name);
        }

        // Record call site (names only, not values)
        ctx.comptime_registry.call_sites.push(ComptimeCallSite {
            call_expr_id: call.as_id(),
            func_name: call.name(ctx.db),
            comptime_param_indices: comptime_indices,
            comptime_arg_names: arg_names,
        });
    }

    // ... rest of type checking (unchanged)
}
```

### 3.3 Validate Const-Binding-Only

```rust
/// Validate that a comptime argument is a const binding name
fn validate_comptime_arg<'db>(
    ctx: &TypeContext<'db>,
    arg: ExprFun<'db>,
    param_idx: usize,
) -> Result<InternedText<'db>, TypeError> {
    match arg.expr(ctx.db) {
        ExprFunKind::Name(name) => {
            // Must be a const binding, not a let/var
            if ctx.is_const_binding(name) {
                Ok(name)
            } else {
                Err(TypeError::ComptimeArgNotConst {
                    param_idx,
                    reason: format!("'{}' is not a const binding", name.text(ctx.db)),
                })
            }
        }
        _ => Err(TypeError::ComptimeArgNotConst {
            param_idx,
            reason: "comptime argument must be a const binding name".to_string(),
        }),
    }
}
```

### 3.4 Propagate Registry Through Pipeline

The `ComptimeCallSiteRegistry` is attached to `SingleModuleTypecheckResult` and
flows through to the lowering phase:

```rust
// In typecheck_module result
pub struct SingleModuleTypecheckResult<'db> {
    // ... existing fields ...
    pub comptime_registry: ComptimeCallSiteRegistry<'db>,  // NEW
}
```

---

## Phase 4: IR-Level Specialization

This phase runs within lowering, specifically as **phase 5c** after const evaluation.
All work happens on IR, not AST.

### 4.1 Integration into Lowering Pipeline

**File**: `datalove-datafun-compiler/src/tracked_lower.rs`

```rust
pub fn lower_module_graph_with_evaluator<'db>(
    // ... existing params ...
) -> ModuleGraphLoweringResult<'db> {
    // Phase 5a: Lower all functions (existing)
    let lowered_functions = lower_all_module_functions(db, ...);

    // Phase 5b: Evaluate consts (existing)
    let resolved_consts = evaluate_all_module_consts(db, &lowered_functions, evaluator);

    // Phase 5c: Specialize comptime functions (NEW)
    let (specialized_functions, call_rewrites) = specialize_comptime_functions(
        db,
        &lowered_functions,
        &resolved_consts,
        &typecheck_result.comptime_registry,
    );

    // Phase 5d: Assemble (existing, uses specialized functions)
    assemble_modules(db, &specialized_functions, &resolved_consts, &call_rewrites)
}
```

### 4.2 Resolve Comptime Values

**File**: `datalove-datafun-specialize/src/lib.rs`

```rust
/// Resolve comptime arg names to values using already-evaluated consts
fn resolve_comptime_calls(
    registry: &ComptimeCallSiteRegistry,
    resolved_consts: &ResolvedConsts,
) -> Vec<ResolvedComptimeCall> {
    registry.call_sites.iter().map(|site| {
        let values: Vec<ConstValue> = site.comptime_arg_names.iter()
            .map(|name| {
                resolved_consts.get_by_name(name.text())
                    .expect("const binding should exist")
                    .clone()
            })
            .collect();

        ResolvedComptimeCall {
            call_site_id: site.call_expr_id,
            comptime_values: values,
        }
    }).collect()
}
```

**Key point**: No CTFE here—just HashMap lookups into `ResolvedConsts`.

### 4.3 Build Specialization Plan

```rust
/// Group call sites by function and collect unique instantiations
fn build_specialization_plan(
    resolved_calls: &[ResolvedComptimeCall],
    registry: &ComptimeCallSiteRegistry,
) -> HashMap<String, FuncSpecialization> {
    let mut plan: HashMap<String, FuncSpecialization> = HashMap::new();

    for call in resolved_calls {
        let func_name = registry.get_func_name(call.call_site_id);

        let spec = plan.entry(func_name.clone()).or_insert_with(|| {
            FuncSpecialization {
                original_func_id: registry.get_func_id(&func_name),
                enum_type: IrType::Unit,  // built later
                value_to_variant: HashMap::new(),
                instantiations: Vec::new(),
            }
        });

        // Add unique instantiation
        if !spec.instantiations.contains(&call.comptime_values) {
            let variant_idx = spec.instantiations.len() as u32;
            spec.value_to_variant.insert(call.comptime_values.clone(), variant_idx);
            spec.instantiations.push(call.comptime_values.clone());
        }
    }

    // Build enum types
    for spec in plan.values_mut() {
        spec.enum_type = build_comptime_enum(spec.instantiations.len());
    }

    plan
}

fn build_comptime_enum(num_variants: usize) -> IrType {
    let variants: Vec<(String, Option<IrType>)> = (0..num_variants)
        .map(|i| (format!("V{}", i), None))
        .collect();
    IrType::Enum(variants)
}
```

### 4.4 Transform IR Function to Union-Branch

```rust
/// Transform an IrFunction to union-branch form
fn transform_ir_function(
    func: &IrFunction,
    spec: &FuncSpecialization,
    comptime_param_indices: &[usize],
) -> IrFunction {
    // Original: params = [comptime_p0, comptime_p1, regular_p0, ...]
    // New:      params = [tag, regular_p0, ...]

    let mut new_params = Vec::new();
    let mut new_param_types = Vec::new();
    let mut new_param_modes = Vec::new();

    // Add tag parameter
    new_params.push(ParamId(0));
    new_param_types.push(spec.enum_type.clone());
    new_param_modes.push(ParamMode::In);

    // Add non-comptime params (renumbered)
    for (i, (param, (ty, mode))) in func.params.iter()
        .zip(func.param_types.iter().zip(func.param_modes.iter()))
        .enumerate()
    {
        if !comptime_param_indices.contains(&i) {
            new_params.push(ParamId(new_params.len() as u32));
            new_param_types.push(ty.clone());
            new_param_modes.push(*mode);
        }
    }

    // Build dispatch blocks + specialized body copies
    let new_blocks = build_dispatch_ir(
        &func.blocks,
        &spec.instantiations,
        comptime_param_indices,
        &func.param_types,
    );

    IrFunction {
        id: func.id,
        name: func.name.clone(),
        params: new_params,
        param_modes: new_param_modes,
        param_types: new_param_types,
        return_type: func.return_type.clone(),
        blocks: new_blocks,
        // ... update value_count, slot_count, etc.
    }
}
```

### 4.5 Build IR Dispatch Blocks

```rust
fn build_dispatch_ir(
    original_blocks: &[IrBlock],
    instantiations: &[Vec<ConstValue>],
    comptime_param_indices: &[usize],
    param_types: &[IrType],
) -> Vec<IrBlock> {
    let mut blocks = Vec::new();
    let num_variants = instantiations.len();

    // Entry block: get discriminant and start dispatch chain
    let entry = IrBlock {
        id: BlockId(0),
        params: vec![],
        instructions: vec![
            // v0 = param0 (the tag)
            // v1 = discriminant(v0) -- or just use v0 if enum is repr(int)
        ],
        terminator: Terminator::Branch {
            cond: /* v1 == 0 */,
            then_block: BlockId(num_variants as u32),  // first variant body
            then_args: vec![],
            else_block: BlockId(1),  // next check
            else_args: vec![],
        },
    };
    blocks.push(entry);

    // Dispatch chain: check each variant
    for i in 1..num_variants {
        let check_block = IrBlock {
            id: BlockId(i as u32),
            params: vec![],
            instructions: vec![],
            terminator: Terminator::Branch {
                cond: /* discriminant == i */,
                then_block: BlockId((num_variants + i) as u32),
                then_args: vec![],
                else_block: BlockId((i + 1) as u32),
                else_args: vec![],
            },
        };
        blocks.push(check_block);
    }

    // Last check falls through to unreachable/panic
    // (or last variant with no else)

    // Variant bodies: clone original blocks with const substitution
    for (variant_idx, values) in instantiations.iter().enumerate() {
        let variant_blocks = clone_blocks_with_const_substitution(
            original_blocks,
            comptime_param_indices,
            values,
            param_types,
            BlockId((num_variants + variant_idx) as u32),  // base block id
        );
        blocks.extend(variant_blocks);
    }

    blocks
}
```

### 4.6 Const Substitution in Cloned Blocks

```rust
fn clone_blocks_with_const_substitution(
    original_blocks: &[IrBlock],
    comptime_param_indices: &[usize],
    values: &[ConstValue],
    param_types: &[IrType],
    base_block_id: BlockId,
) -> Vec<IrBlock> {
    let mut cloned = Vec::new();

    for (i, block) in original_blocks.iter().enumerate() {
        let mut new_block = block.clone();
        new_block.id = BlockId(base_block_id.0 + i as u32);

        // Prepend const instructions for comptime params
        if i == 0 {
            let mut const_instrs: Vec<Instruction> = comptime_param_indices.iter()
                .zip(values.iter())
                .enumerate()
                .map(|(i, (&param_idx, value))| {
                    Instruction::Const {
                        dest: ValueId(/* fresh id for this param */),
                        value: value.clone(),
                    }
                })
                .collect();
            const_instrs.extend(new_block.instructions.drain(..));
            new_block.instructions = const_instrs;
        }

        // Rewrite any references to comptime params → the const values
        rewrite_param_references(&mut new_block, comptime_param_indices);

        // Adjust block references in terminators
        adjust_block_references(&mut new_block.terminator, base_block_id);

        cloned.push(new_block);
    }

    cloned
}
```

### 4.7 Rewrite Call Instructions

```rust
fn rewrite_call_instructions(
    functions: &mut [IrFunction],
    spec_plan: &HashMap<String, FuncSpecialization>,
    resolved_calls: &[ResolvedComptimeCall],
    registry: &ComptimeCallSiteRegistry,
) {
    // Build lookup: call_site_id → (func_name, variant_idx)
    let call_lookup: HashMap<_, _> = resolved_calls.iter()
        .map(|call| {
            let func_name = registry.get_func_name(call.call_site_id);
            let spec = &spec_plan[&func_name];
            let variant_idx = spec.value_to_variant[&call.comptime_values];
            (call.call_site_id, (func_name, variant_idx))
        })
        .collect();

    for func in functions {
        for block in &mut func.blocks {
            for instr in &mut block.instructions {
                if let Instruction::Call { dest, func: func_ref, args } = instr {
                    // Check if this call needs rewriting
                    // (need to map IR call back to original call site somehow)
                    if let Some((func_name, variant_idx)) = lookup_call(instr, &call_lookup) {
                        let spec = &spec_plan[&func_name];

                        // Build new args: [enum_variant, non-comptime args...]
                        let mut new_args = Vec::new();

                        // Add enum variant construction
                        // (emit EnumVariant instruction before call, use result)
                        let variant_val = /* value from EnumVariant instr */;
                        new_args.push(Operand::Value(variant_val));

                        // Add non-comptime args
                        let comptime_indices = &registry.comptime_funcs[&func_name];
                        for (i, arg) in args.iter().enumerate() {
                            if !comptime_indices.contains(&i) {
                                new_args.push(arg.clone());
                            }
                        }

                        *args = new_args;
                    }
                }
            }
        }
    }
}
```

---

## Phase 5: Const Folding Within Branches

After union-branch transformation, each branch contains `const` bindings for the
comptime parameter values. The **existing CTFE infrastructure** handles this automatically.

### 5.1 How Existing Const Folding Works

From `compiler-guide.md`, the lowering phase already:

1. Collects const bindings
2. Evaluates them via CTFE
3. Inlines the values at use sites

The union-branch transformation produces code like:

```
// Before CTFE (conceptual IR):
if discriminant(n_tag) == 0
    const n = 3          // <- normal const binding
    let result = s * n   // <- uses const n
    ...
```

The existing const eval pass sees `const n = 3` as a normal const binding and
inlines `3` wherever `n` is used within that branch.

### 5.2 No Additional Work Needed

Because we:
1. Transform at IR level (Phase 4)
2. Insert normal `Instruction::Const` for comptime param values
3. Let the existing phase 5b/5d handle evaluation and inlining

The const folding is **free** — we just emit the right IR structure.

### 5.3 Salsa Integration

The specialization step is not a separate tracked function — it's part of the
lowering pipeline:

```rust
// In tracked_lower.rs, within lower_module_graph_with_evaluator:

pub fn lower_module_graph_with_evaluator<'db>(...) -> ModuleGraphLoweringResult<'db> {
    // 5a: Lower all functions
    let mut lowered = lower_all_functions(db, ...);

    // 5b: Evaluate top-level consts
    let resolved_consts = evaluate_consts(db, &lowered, evaluator);

    // 5c: Specialize comptime functions (NEW)
    if !typecheck_result.comptime_registry.is_empty() {
        specialize_in_place(&mut lowered, &resolved_consts, &typecheck_result.comptime_registry);
    }

    // 5d: Assemble and inline
    assemble(db, lowered, resolved_consts)
}
```

This keeps specialization as a simple in-place transformation rather than a
separate Salsa query, avoiding cache invalidation complexity.

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
- Specialization is IR-to-IR transformation within lowering (phase 5c)
- No AST modification needed — all changes happen after lowering
- Transformed IR uses existing instruction types (Const, Branch, etc.)

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

### Sprint 1: Foundation
- [ ] Phase 1: AST & Parsing (`is_comptime` field, `const` modifier parsing)
- [ ] Phase 2: Type System (`param_comptime` in TypeFunction)
- [ ] Basic parser and typecheck tests

### Sprint 2: Call Site Recording
- [ ] Phase 3: Record comptime call sites during typecheck
- [ ] Validate const-binding-only restriction
- [ ] Propagate `ComptimeCallSiteRegistry` through pipeline
- [ ] Unit tests for recording

### Sprint 3: IR Transformation
- [ ] Phase 4: Union-branch IR transformation
- [ ] Build dispatch blocks and cloned body blocks
- [ ] Const substitution in cloned blocks
- [ ] Call instruction rewriting
- [ ] Integration tests

### Sprint 4: Pipeline Integration
- [ ] Insert specialization into lowering (phase 5c)
- [ ] Resolve comptime values from `ResolvedConsts`
- [ ] End-to-end tests (interpreter + AOT)

### Sprint 5: Polish
- [ ] Phase 6: Codegen hints (jump table for many variants)
- [ ] Error messages for invalid comptime args
- [ ] Performance benchmarks

---

## Appendix: Key Files to Modify

| File | Changes |
|------|---------|
| `datafun-ast/src/ast.rs` | Add `is_comptime` to FunParam |
| `datafun-parser/src/statement.rs` | Parse `const` modifier |
| `datafun-common/src/lib.rs` | Add `param_comptime` to TypeFunction |
| `datafun-tycheck/src/synthesize.rs` | Validate const-binding-only args, record call sites |
| `datafun-tycheck/src/context.rs` | Add `ComptimeCallSiteRegistry` |
| `datafun-compiler/src/tracked_lower.rs` | Insert specialization step in phase 5c |
| `datafun-ir/src/lib.rs` | (Maybe) enum discriminant helpers |

| New File/Module | Purpose |
|-----------------|---------|
| `datafun-compiler/src/specialize.rs` | IR transformation module |
| `datafun-compiler/src/specialize/transform.rs` | Union-branch IR generation |
| `datafun-compiler/src/specialize/rewrite.rs` | Call instruction rewriting |
| `datafun-compiler/src/specialize/types.rs` | Data structures (FuncSpecialization, etc.) |

**Note**: Can also be a separate `datafun-specialize` crate if preferred for modularity.
