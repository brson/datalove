# Datafun REPL & Interpreter Architecture Research

**Date**: 2025-10-12
**Topic**: Implementing datafun interpreter for datalove REPL with incremental computation, memoization, and advanced runtime features

## Executive Summary

This document synthesizes research on implementing a datafun interpreter for the datalove REPL, focusing on:
- Salsa-based incremental computation for REPL state
- Modern REPL design patterns (reactive, dataflow-aware)
- Interpreter architectures with clear paths to JIT/AOT compilation
- Leveraging datalit's unique runtime features (memoization, undo/redo, choice points)

**Key Recommendations**:
1. Use Salsa for incremental computation of REPL state and evaluation
2. Start with tree-walking interpreter, design for future bytecode VM
3. Implement reactive dataflow tracking like Observable/IPyflow
4. Leverage pure-data semantics for perfect memoization
5. Build undo/redo on command pattern + dataflow tracking
6. Implement choice points as generators (Phase 1) before full backtracking

## 1. Current Datalove Infrastructure

### 1.1 What We Have

**Runtime ABI (rtdt)**:
- Complete C-compatible type system
- Scalars: Bool, U32, F32, Int (bigint)
- Collections: List, String, Map (B+tree), Set (B+tree)
- Structured: Tuples, Structs, Enums
- Special: Option, Result, Data, Error
- Full type descriptors (TyDesc) for runtime introspection

**Datalit Language**:
- Salsa-based database already set up (`datalove_datalit::Database`)
- AST with expressions for all rtdt types
- Type hints with heap annotations (Local, Global, Omitted)
- Parser, resolver, type checker, instantiate, pretty printer

**REPL Framework**:
- Basic command parsing (ReplCommand vs ScriptStatement)
- Engine with eval (stub implementations)
- Multiple frontends: term, rat (ratatui), egui

**Existing Research**:
- Extensive logic programming research (`notes/research-logic-programming.md`)
- Generator-based choice points, multi-determinism
- Argument modes for bidirectional functions

### 1.2 What We're Building

**Datafun**: Simple functional language built on datalit types
- Pure functions operating on pure data
- Eventually: argument modes (in, out, di, uo)
- Eventually: multi-determinism (generators, choice points)
- REPL-first with advanced features

## 2. Salsa for REPL Incremental Computation

### 2.1 Salsa Fundamentals

**What Salsa Provides**:
- On-demand, incremental computation
- Automatic dependency tracking
- Memoization with invalidation
- Used by rust-analyzer and rustc query system

**Core Mechanism**:
1. Define queries (tracked functions)
2. Queries automatically track dependencies
3. Results are memoized
4. When inputs change, only affected queries recompute
5. Early cutoff: if result unchanged, dependents reuse cached values

**Key Optimization**: "Durable Incrementality"
- Persist memoization across sessions
- Track file mtimes and hashes
- Invalidate only when actual content changes

### 2.2 Representing REPL State in Salsa

**Challenge**: REPL is inherently stateful and session-based
- Each input builds on previous state
- Need to track history for undo/redo
- Want incremental re-evaluation when definitions change

**Solution**: Session-based Input with Revision Tracking

```rust
use salsa::Database;

#[salsa::input]
pub struct ReplSession {
    /// Unique session ID
    #[returns(ref)]
    pub session_id: SessionId,

    /// History of all statements executed
    #[returns(ref)]
    pub statements: Vec<Statement>,
}

#[salsa::input]
pub struct Statement {
    /// Statement ID (monotonic)
    pub stmt_id: StmtId,

    /// Session this belongs to
    pub session: ReplSession,

    /// Source code
    #[returns(ref)]
    pub source: String,

    /// Parsed AST (memoized)
    // Implemented as tracked query below
}

#[salsa::tracked]
pub fn parse_statement(db: &dyn ReplDb, stmt: Statement) -> ParsedStatement {
    // Parse statement.source(db)
    // Automatically memoized by Salsa
}

#[salsa::tracked]
pub fn resolve_statement(
    db: &dyn ReplDb,
    stmt: Statement,
    env: Environment,
) -> ResolvedStatement {
    // Resolve names in context of environment
    // Automatically tracks dependencies on environment
}

#[salsa::tracked]
pub fn typecheck_statement(
    db: &dyn ReplDb,
    resolved: ResolvedStatement,
    type_env: TypeEnvironment,
) -> TypedStatement {
    // Type check with type environment
}

#[salsa::tracked]
pub fn eval_statement(
    db: &dyn ReplDb,
    typed: TypedStatement,
    runtime_env: RuntimeEnvironment,
) -> EvalResult {
    // Evaluate statement
    // Returns value + updated environment
}
```

**Environment Tracking**:

```rust
#[salsa::tracked]
pub struct Environment {
    /// Parent environment (for scoping)
    pub parent: Option<Environment>,

    /// Bindings introduced by this environment
    #[returns(ref)]
    pub bindings: HashMap<InternedText, Value>,
}

#[salsa::tracked]
pub fn lookup_binding(
    db: &dyn ReplDb,
    env: Environment,
    name: InternedText,
) -> Option<Value> {
    // Lookup with automatic dependency tracking
    if let Some(val) = env.bindings(db).get(&name) {
        Some(val.clone())
    } else if let Some(parent) = env.parent(db) {
        lookup_binding(db, parent, name)
    } else {
        None
    }
}
```

**Key Insight**: Each REPL statement becomes a Salsa input, and all derived computations (parsing, type checking, evaluation) are tracked queries. When a statement is redefined, only affected downstream computations re-execute.

### 2.3 Incremental Re-evaluation

**Scenario**: User redefines a function

```rust
// Session:
> fun add(x, y) = x + y
> let result = add(2, 3)
result: 5

// User redefines add:
> fun add(x, y) = x + y + 1

// Salsa automatically:
1. Invalidates add definition
2. Marks result binding as dirty
3. Re-evaluates result on next access
result: 6  // Updated automatically
```

**Implementation**:

```rust
#[salsa::tracked]
pub fn get_current_environment(db: &dyn ReplDb, session: ReplSession) -> Environment {
    // Build environment from all statements
    let mut env = Environment::empty(db);
    for stmt in session.statements(db) {
        let result = eval_statement(db, stmt, env);
        env = result.updated_env(db);
    }
    env
}

// When statement redefined:
pub fn redefine_statement(db: &mut dyn ReplDb, stmt_id: StmtId, new_source: String) {
    // Update statement source (Salsa input)
    let session = current_session(db);
    let stmt = find_statement(db, session, stmt_id);
    stmt.set_source(db).to(new_source);

    // Salsa automatically invalidates all dependent queries:
    // - parse_statement
    // - resolve_statement
    // - typecheck_statement
    // - eval_statement
    // - get_current_environment
    // - any queries that depend on bindings from this statement
}
```

### 2.4 Salsa + REPL Best Practices

**From rust-analyzer experience**:

1. **Intern strings aggressively**
   - Use `InternedText` for all identifiers
   - Reduces memory, speeds up equality checks

2. **Structure as many small queries, not few large ones**
   - Fine-grained invalidation
   - Better incremental performance

3. **Use `#[returns(ref)]` for collections**
   - Avoids cloning large data structures
   - Salsa tracks by identity

4. **Design for durability**
   - Persist Salsa database across REPL sessions
   - Fast restart by loading cached state

5. **Track file dependencies**
   - If REPL can load files, track them as inputs
   - Automatic reload when files change

**Example**: Persistent REPL Session

```rust
pub struct ReplPersistence {
    db_path: PathBuf,
}

impl ReplPersistence {
    pub fn save_session(&self, db: &dyn ReplDb, session: ReplSession) -> AnyResult<()> {
        // Serialize Salsa database
        // Save to disk
    }

    pub fn load_session(&self) -> AnyResult<(Box<dyn ReplDb>, ReplSession)> {
        // Deserialize Salsa database
        // Restore session state
        // All memoized results preserved!
    }
}
```

## 3. State-of-the-Art REPL Design

### 3.1 Observable Notebooks: Reactive Dataflow

**Key Innovation**: Spreadsheet-like reactivity for code

**How It Works**:
1. Analyze code to extract variable dependencies
2. Build dependency graph (DAG)
3. Compute topological order for execution
4. When variable changes, re-run only affected cells
5. Instant feedback, automatic updates

**Observable Runtime** (from GitHub observablehq/runtime):
```javascript
// Observable's reactive model:
// Cell declares what it defines and what it uses

cell1 = {
  // Defines: x
  // Uses: nothing
  const x = 42;
  return x;
}

cell2 = {
  // Defines: y
  // Uses: x
  const y = x * 2;
  return y;
}

// Runtime automatically:
// - Detects x is used by cell2
// - When cell1 re-runs, cell2 re-runs automatically
// - Maintains topological order
```

**Lesson for Datalove**:
- Track which bindings each statement uses and defines
- Build dependency graph
- Incremental re-execution on change
- Salsa gives us this for free!

### 3.2 IPyflow & Dataflow Notebooks

**Problem with Traditional Jupyter**:
- Cells can be run out of order
- Hidden state, stale outputs
- Hard to reason about notebook state

**IPyflow Solution**:
- Track dataflow between cells automatically
- Detect stale outputs
- Reactively re-execute on dependency changes
- Warn about out-of-order execution

**Key Features**:
1. **Automatic dependency tracking**: Analyze which variables each cell reads/writes
2. **Reactive execution**: When cell re-runs, downstream cells marked stale
3. **Smart re-execution**: Option to auto-re-run stale cells
4. **Dataflow visualization**: Show dependency graph

**Implementation Approach**:
```python
# IPyflow tracks:
- Per-cell: defined_symbols, used_symbols
- Global: symbol_to_defining_cell mapping
- Dependency graph: cell -> [dependent_cells]

# On cell execution:
1. Record which symbols are read/written
2. Update dependency graph
3. Mark downstream cells as stale
4. Optionally auto-execute stale cells
```

**Lesson for Datalove**:
- Similar to Salsa's automatic dependency tracking
- Need to track symbol definitions at statement level
- Mark dependent statements as needing re-eval
- REPL UI should show stale bindings

### 3.3 Marimo: Python Notebooks with Reactivity

**Design Philosophy**:
- Notebooks as Python scripts (not JSON)
- Reactive by default
- Reproducible (no hidden state)

**Key Rule**:
> When a cell is run, all other cells that reference its definitions
> (its descendants) are also run.

**Implementation**:
```python
# Static analysis extracts dependencies:
import marimo as mo

@mo.cell
def cell1():
    x = 42
    return x

@mo.cell
def cell2(x):  # Explicit dependency
    y = x * 2
    return y

# Runtime ensures topological execution order
# Change x -> y automatically updates
```

**Lesson for Datalove**:
- Make dependencies explicit in function signatures
- Salsa already does this via query parameters
- Consider making REPL statements more like cells

### 3.4 Replit History++: Operational Transformations

**Innovation**: Track edits as operational transformations (OTs)
- Store "intended changes" not just state snapshots
- Enables collaborative editing
- Efficient storage (log of diffs vs full snapshots)

**Approach**:
```typescript
// Log of operations:
type Operation =
  | { type: 'insert', pos: number, text: string }
  | { type: 'delete', pos: number, length: number }
  | { type: 'replace', pos: number, oldText: string, newText: string }

// Replay history:
function replayTo(targetVersion: number) {
  let state = initialState;
  for (let op of operations.slice(0, targetVersion)) {
    state = applyOperation(state, op);
  }
  return state;
}
```

**Lesson for Datalove**:
- Track REPL history as edits/commands
- Enables undo/redo
- Enables session replay
- Efficient history navigation

## 4. Interpreter Architecture: Tree-Walk to JIT

### 4.1 The Progression

**Standard Path**: Tree-Walk → Bytecode VM → JIT → AOT

1. **Tree-Walking Interpreter**
   - Directly execute AST
   - Simple to implement
   - Slow (pointer chasing, no optimization)
   - Good for prototyping

2. **Bytecode VM**
   - Compile AST to bytecode
   - Stack-based or register-based
   - More efficient than tree-walking
   - Platform-independent

3. **JIT Compiler**
   - Compile hot bytecode to machine code
   - Profile-guided optimization
   - Fast execution
   - Complex implementation

4. **AOT Compiler**
   - Compile everything ahead of time
   - Maximum performance
   - No runtime overhead
   - Less flexible

### 4.2 Phase 1: Tree-Walking Interpreter

**Recommendation**: Start here for datafun

**Why**:
- Datafun is simple (pure functional, no complex control flow)
- Datalit AST already exists
- Focus on semantics first, performance later
- Easy to debug and prototype

**Basic Structure**:

```rust
pub struct Interpreter {
    db: Box<dyn ReplDb>,
}

pub enum Value {
    Bool(bool),
    U32(u32),
    F32(f32),
    Int(BigInt),
    String(String),
    Tuple(Vec<Value>),
    Struct(HashMap<String, Value>),
    Enum { variant: String, payload: Option<Box<Value>> },
    List(Vec<Value>),
    Map(BTreeMap<Value, Value>),
    Set(BTreeSet<Value>),
    Function(Function),
    // ... other types
}

impl Interpreter {
    pub fn eval_expr(&mut self, expr: &Expr, env: &Environment) -> Result<Value, Error> {
        match expr {
            Expr::True => Ok(Value::Bool(true)),
            Expr::False => Ok(Value::Bool(false)),
            Expr::Int(i) => Ok(Value::Int(i.parse()?)),
            Expr::String(s) => Ok(Value::String(s.clone())),

            Expr::Var(name) => {
                env.lookup(name)
                    .ok_or_else(|| Error::UndefinedVariable(name.clone()))
            }

            Expr::Let { name, value, body } => {
                let val = self.eval_expr(value, env)?;
                let new_env = env.extend(name.clone(), val);
                self.eval_expr(body, &new_env)
            }

            Expr::FunCall { func, args } => {
                let func_val = self.eval_expr(func, env)?;
                let arg_vals: Vec<Value> = args.iter()
                    .map(|arg| self.eval_expr(arg, env))
                    .collect::<Result<_, _>>()?;
                self.apply_function(func_val, arg_vals)
            }

            // ... other cases
        }
    }
}
```

**Integration with Salsa**:

```rust
#[salsa::tracked]
pub fn eval_expr_salsa(
    db: &dyn ReplDb,
    expr: ExprFull,
    env: Environment,
) -> EvalResult {
    let interpreter = Interpreter::new(db);
    match interpreter.eval_expr(expr.expr(db), &env) {
        Ok(value) => EvalResult::Value(value),
        Err(e) => EvalResult::Error(e),
    }
}
```

### 4.3 Phase 2: Bytecode VM

**When to Transition**: After core semantics are stable

**Why Bytecode**:
- 10-100x faster than tree-walking
- Easier optimization opportunities
- Foundation for JIT
- More compact representation

**VM Design Choices**:

**Stack-Based** (like JVM, CPython):
```
PUSH 2
PUSH 3
ADD
RETURN
```
- Pros: Simple, compact code
- Cons: More instructions, stack manipulation overhead

**Register-Based** (like Lua, many modern VMs):
```
LOAD R1, 2
LOAD R2, 3
ADD R3, R1, R2
RETURN R3
```
- Pros: Fewer instructions, more direct
- Cons: Larger instruction size

**Recommendation**: Register-based
- Better for functional language
- Easier to optimize
- Natural fit for SSA form

**Example Bytecode Design**:

```rust
pub enum Bytecode {
    // Constants
    LoadInt { dst: Reg, value: i64 },
    LoadBool { dst: Reg, value: bool },
    LoadString { dst: Reg, idx: u32 }, // Index into constant pool

    // Arithmetic
    Add { dst: Reg, lhs: Reg, rhs: Reg },
    Sub { dst: Reg, lhs: Reg, rhs: Reg },
    Mul { dst: Reg, lhs: Reg, rhs: Reg },

    // Variables
    LoadLocal { dst: Reg, idx: u32 },
    StoreLocal { src: Reg, idx: u32 },
    LoadGlobal { dst: Reg, name_idx: u32 },

    // Functions
    Call { dst: Reg, func: Reg, args: Vec<Reg> },
    Return { src: Reg },

    // Collections
    MakeList { dst: Reg, elems: Vec<Reg> },
    MakeTuple { dst: Reg, elems: Vec<Reg> },
    MakeStruct { dst: Reg, fields: Vec<(u32, Reg)> },

    // Control flow (later)
    Jump { target: Label },
    JumpIf { cond: Reg, target: Label },

    // ... more instructions
}

pub struct BytecodeChunk {
    pub instructions: Vec<Bytecode>,
    pub constants: Vec<Constant>,
    pub num_registers: usize,
}
```

**VM Execution**:

```rust
pub struct VM {
    registers: Vec<Value>,
    call_stack: Vec<CallFrame>,
    globals: HashMap<String, Value>,
}

impl VM {
    pub fn execute(&mut self, chunk: &BytecodeChunk) -> Result<Value, Error> {
        let mut pc = 0; // Program counter

        loop {
            let instr = &chunk.instructions[pc];
            match instr {
                Bytecode::LoadInt { dst, value } => {
                    self.registers[dst.0] = Value::Int((*value).into());
                    pc += 1;
                }

                Bytecode::Add { dst, lhs, rhs } => {
                    let l = &self.registers[lhs.0];
                    let r = &self.registers[rhs.0];
                    self.registers[dst.0] = l.add(r)?;
                    pc += 1;
                }

                Bytecode::Call { dst, func, args } => {
                    let func_val = &self.registers[func.0];
                    let arg_vals: Vec<_> = args.iter()
                        .map(|r| self.registers[r.0].clone())
                        .collect();

                    let result = self.call_function(func_val, arg_vals)?;
                    self.registers[dst.0] = result;
                    pc += 1;
                }

                Bytecode::Return { src } => {
                    return Ok(self.registers[src.0].clone());
                }

                // ... other instructions
            }
        }
    }
}
```

**Compilation Pass**:

```rust
pub struct Compiler {
    chunk: BytecodeChunk,
    next_register: usize,
    locals: HashMap<String, usize>,
}

impl Compiler {
    pub fn compile_expr(&mut self, expr: &Expr) -> Result<Reg, Error> {
        match expr {
            Expr::Int(i) => {
                let dst = self.alloc_register();
                self.emit(Bytecode::LoadInt { dst, value: i.parse()? });
                Ok(dst)
            }

            Expr::Add { lhs, rhs } => {
                let lhs_reg = self.compile_expr(lhs)?;
                let rhs_reg = self.compile_expr(rhs)?;
                let dst = self.alloc_register();
                self.emit(Bytecode::Add { dst, lhs: lhs_reg, rhs: rhs_reg });
                self.free_register(lhs_reg);
                self.free_register(rhs_reg);
                Ok(dst)
            }

            Expr::FunCall { func, args } => {
                let func_reg = self.compile_expr(func)?;
                let arg_regs: Vec<_> = args.iter()
                    .map(|arg| self.compile_expr(arg))
                    .collect::<Result<_, _>>()?;

                let dst = self.alloc_register();
                self.emit(Bytecode::Call {
                    dst,
                    func: func_reg,
                    args: arg_regs.clone(),
                });

                self.free_register(func_reg);
                for reg in arg_regs {
                    self.free_register(reg);
                }

                Ok(dst)
            }

            // ... other cases
        }
    }
}
```

### 4.4 Phase 3: JIT Compilation with Cranelift

**When to Add JIT**: After bytecode VM is stable and profiling shows hot paths

**Why Cranelift**:
- Fast compilation (order of magnitude faster than LLVM)
- Safe (written in Rust)
- Designed for JIT use cases
- Used by Wasmtime for WASM JIT
- No need for fallback to interpreter (unlike many JITs)

**Architecture**:

```rust
use cranelift::prelude::*;
use cranelift_module::{Module, Linkage};
use cranelift_jit::{JITModule, JITBuilder};

pub struct JitCompiler {
    builder_context: FunctionBuilderContext,
    ctx: codegen::Context,
    module: JITModule,
}

impl JitCompiler {
    pub fn compile_function(&mut self, bytecode: &BytecodeChunk) -> Result<*const u8, Error> {
        // Translate bytecode to Cranelift IR
        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);

        // Map registers to Cranelift values
        let mut registers: Vec<Value> = vec![];

        for instr in &bytecode.instructions {
            match instr {
                Bytecode::LoadInt { dst, value } => {
                    let val = builder.ins().iconst(types::I64, *value);
                    registers[dst.0] = val;
                }

                Bytecode::Add { dst, lhs, rhs } => {
                    let l = registers[lhs.0];
                    let r = registers[rhs.0];
                    let result = builder.ins().iadd(l, r);
                    registers[dst.0] = result;
                }

                Bytecode::Return { src } => {
                    let val = registers[src.0];
                    builder.ins().return_(&[val]);
                }

                // ... other instructions
            }
        }

        builder.seal_all_blocks();
        builder.finalize();

        // Compile to machine code
        let id = self.module.declare_function(
            "jit_func",
            Linkage::Local,
            &self.ctx.func.signature,
        )?;

        self.module.define_function(id, &mut self.ctx)?;
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions()?;

        let code_ptr = self.module.get_finalized_function(id);
        Ok(code_ptr)
    }
}
```

**Tiered Compilation Strategy**:

```rust
pub struct TieredExecutor {
    vm: VM,
    jit: JitCompiler,
    hot_functions: HashMap<FunctionId, HotFunctionInfo>,
}

struct HotFunctionInfo {
    call_count: usize,
    bytecode: BytecodeChunk,
    jit_code: Option<*const u8>,
}

impl TieredExecutor {
    pub fn execute_function(&mut self, func_id: FunctionId, args: Vec<Value>) -> Result<Value, Error> {
        let info = self.hot_functions.get_mut(&func_id).unwrap();
        info.call_count += 1;

        // Threshold for JIT compilation
        const JIT_THRESHOLD: usize = 100;

        if info.call_count >= JIT_THRESHOLD && info.jit_code.is_none() {
            // Compile to native code
            info.jit_code = Some(self.jit.compile_function(&info.bytecode)?);
        }

        if let Some(jit_code) = info.jit_code {
            // Execute native code
            unsafe {
                let func: extern "C" fn(*const Value, usize) -> Value
                    = std::mem::transmute(jit_code);
                Ok(func(args.as_ptr(), args.len()))
            }
        } else {
            // Execute in VM
            self.vm.execute_function(&info.bytecode, args)
        }
    }
}
```

### 4.5 Phase 4: AOT Compilation

**When to Add AOT**: For deployment, ahead-of-time compilation of modules

**Two Approaches**:

**1. Cranelift AOT**:
```rust
use cranelift_object::{ObjectModule, ObjectBuilder};

pub fn compile_module_aot(module: &DatafunModule) -> Result<Vec<u8>, Error> {
    let builder = ObjectBuilder::new(
        isa::lookup_by_name("x86_64").unwrap(),
        "module",
        cranelift_module::default_libcall_names(),
    )?;

    let mut obj_module = ObjectModule::new(builder);

    // Compile all functions
    for func in &module.functions {
        compile_function_aot(&mut obj_module, func)?;
    }

    // Emit object file
    let obj_product = obj_module.finish();
    Ok(obj_product.emit()?)
}
```

**2. WASM Target**:
```rust
// Compile datafun -> WASM
pub fn compile_to_wasm(module: &DatafunModule) -> Result<Vec<u8>, Error> {
    // Use wasm-encoder to emit WASM bytecode
    // Advantages:
    // - Universal target
    // - Can run in browser
    // - Wasmtime can JIT WASM using Cranelift
}
```

**Hybrid REPL Strategy**:
- Interpreter for interactive development
- JIT for hot functions during REPL session
- AOT for loaded modules
- WASM for web deployment

## 5. Memoization Strategies

### 5.1 Perfect Memoization with Pure Data

**Key Advantage**: Datalove's pure-data semantics enable perfect memoization

**Why Pure Data is Perfect for Memoization**:
1. **No side effects**: Function always returns same output for same input
2. **Value semantics**: Can use value as cache key
3. **No aliasing**: No hidden dependencies
4. **Deterministic**: No non-determinism (except explicit choice points)

**Contrast with Impure Languages**:
```python
# Impure - can't memoize safely
def get_config():
    return read_config_file()  # Reads from disk!

# Memoizing this would be wrong:
# Config file might change, but cache would return stale value
```

```rust
// Pure - safe to memoize
fun load_config(path: @string ^in) -> Config ^out {
    // If same path, always same config
    // Safe to cache
}
```

### 5.2 Salsa is Automatic Memoization

**Key Insight**: Salsa queries are automatically memoized

```rust
#[salsa::tracked]
pub fn expensive_computation(db: &dyn Db, input: Input) -> Output {
    // This is automatically memoized by Salsa
    // - First call: compute and cache
    // - Subsequent calls: return cached value
    // - If input changes: recompute
}
```

**Example**: Parsing and Type Checking

```rust
#[salsa::tracked]
pub fn parse_source(db: &dyn Db, source: InternedText) -> Ast {
    // Parse source to AST
    // Memoized automatically
}

#[salsa::tracked]
pub fn typecheck(db: &dyn Db, ast: Ast) -> TypedAst {
    // Type check AST
    // Memoized automatically
    // Depends on parse_source, so invalidated when source changes
}

// Usage in REPL:
let ast = parse_source(db, source);  // Computed
let typed = typecheck(db, ast);      // Computed

// Later, same source:
let ast2 = parse_source(db, source); // Cached!
let typed2 = typecheck(db, ast2);    // Cached!

// Modified source:
let new_source = modify(source);
let ast3 = parse_source(db, new_source); // Recomputed
let typed3 = typecheck(db, ast3);        // Recomputed
```

### 5.3 User-Level Memoization

**Beyond Salsa**: Allow users to memoize functions

**Syntax Proposal**:

```rust
#[memoized]
fun fibonacci(n: @u32 ^in) -> @u32 ^out {
    if n <= @1 {
        n
    } else {
        fibonacci(n - @1) + fibonacci(n - @2)
    }
}

// Without memoization: O(2^n)
// With memoization: O(n)
```

**Implementation**:

```rust
pub struct MemoizedFunction {
    func: Function,
    cache: HashMap<Vec<Value>, Value>,
}

impl MemoizedFunction {
    pub fn call(&mut self, args: Vec<Value>) -> Result<Value, Error> {
        // Check cache
        if let Some(cached) = self.cache.get(&args) {
            return Ok(cached.clone());
        }

        // Compute
        let result = self.func.call(args.clone())?;

        // Store in cache
        self.cache.insert(args, result.clone());

        Ok(result)
    }
}
```

**Cache Key Requirements**:
- Values must be hashable
- Need total ordering for Map/Set (already have via rtdt)
- Need Eq (have via value semantics)

**Cache Implementation**:

```rust
impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Value::Bool(b) => {
                0u8.hash(state);
                b.hash(state);
            }
            Value::U32(n) => {
                1u8.hash(state);
                n.hash(state);
            }
            Value::Tuple(elems) => {
                2u8.hash(state);
                elems.hash(state);
            }
            Value::List(elems) => {
                3u8.hash(state);
                elems.hash(state);
            }
            // ... other cases
        }
    }
}
```

### 5.4 Memoization for Multi Functions

**Challenge**: Multi functions yield multiple solutions

**Solution**: Cache entire solution sequence

```rust
#[memoized]
#[multi]
fun splits(list: [@T] ^in) yields ([@T], [@T]) {
    for i in @0..=list.len() {
        yield list.split_at(i)
    }
}

// Implementation:
pub struct MemoizedMultiFunction {
    func: MultiFunction,
    cache: HashMap<Vec<Value>, Vec<SolutionSet>>,
}

type SolutionSet = Vec<Value>;

impl MemoizedMultiFunction {
    pub fn call(&mut self, args: Vec<Value>) -> impl Iterator<Item = Value> {
        // Check cache
        if let Some(solutions) = self.cache.get(&args) {
            return solutions.clone().into_iter();
        }

        // Compute all solutions
        let solutions: Vec<_> = self.func.call(args.clone()).collect();

        // Cache solution set
        self.cache.insert(args, solutions.clone());

        solutions.into_iter()
    }
}
```

**Trade-off**: Space vs time
- Must compute all solutions before caching
- Space cost for large solution sets
- Time saved on repeated calls

**Optimization**: Lazy memoization
```rust
// Stream solutions, cache as we go
pub struct LazyMemoizedMulti {
    cache: HashMap<Vec<Value>, Vec<Value>>,
    max_cached: usize,
}

impl LazyMemoizedMulti {
    pub fn call(&mut self, args: Vec<Value>) -> impl Iterator<Item = Value> {
        let cached_count = self.cache.get(&args).map_or(0, |v| v.len());

        if cached_count > 0 {
            // Return cached solutions first
            let cached = self.cache[&args].clone();
            // Then continue computing
            let iter = self.func.call(args.clone()).skip(cached_count);

            cached.into_iter().chain(iter)
        } else {
            // Compute from scratch, cache as we go
            self.func.call(args.clone())
                .inspect(|solution| {
                    self.cache.entry(args.clone())
                        .or_insert_with(Vec::new)
                        .push(solution.clone());
                })
        }
    }
}
```

## 6. Undo/Redo in Functional Languages

### 6.1 Command Pattern + Pure Data

**Key Insight**: Pure data + command pattern = perfect undo/redo

**Why It Works**:
1. **Immutability**: Old states naturally preserved
2. **No side effects**: Undo is just restoring state
3. **Deterministic**: Replay is identical
4. **Value semantics**: Easy to snapshot

**Basic Command Pattern**:

```rust
pub trait Command {
    type State;

    fn execute(&self, state: &Self::State) -> Self::State;
    fn undo(&self, state: &Self::State) -> Self::State;
}

pub struct History<S> {
    states: Vec<S>,
    current: usize,
}

impl<S: Clone> History<S> {
    pub fn push(&mut self, new_state: S) {
        // Discard any states after current
        self.states.truncate(self.current + 1);
        self.states.push(new_state);
        self.current += 1;
    }

    pub fn undo(&mut self) -> Option<&S> {
        if self.current > 0 {
            self.current -= 1;
            Some(&self.states[self.current])
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<&S> {
        if self.current + 1 < self.states.len() {
            self.current += 1;
            Some(&self.states[self.current])
        } else {
            None
        }
    }
}
```

### 6.2 REPL History with Undo/Redo

**Implementation**:

```rust
pub struct ReplHistory {
    // Statement history
    statements: Vec<Statement>,

    // Environment snapshots
    env_snapshots: Vec<Environment>,

    // Current position
    current: usize,
}

impl ReplHistory {
    pub fn execute_statement(&mut self, db: &mut dyn ReplDb, source: String) -> Result<Value, Error> {
        // Parse and evaluate
        let stmt = Statement::new(db, self.statements.len(), source);
        let current_env = self.current_environment();
        let result = eval_statement(db, stmt, current_env)?;

        // Save state
        self.statements.truncate(self.current + 1);
        self.statements.push(stmt);

        self.env_snapshots.truncate(self.current + 1);
        self.env_snapshots.push(result.env);

        self.current += 1;

        Ok(result.value)
    }

    pub fn undo(&mut self) -> Option<Environment> {
        if self.current > 0 {
            self.current -= 1;
            Some(self.env_snapshots[self.current].clone())
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<Environment> {
        if self.current + 1 < self.env_snapshots.len() {
            self.current += 1;
            Some(self.env_snapshots[self.current].clone())
        } else {
            None
        }
    }
}
```

### 6.3 Dataflow-Aware Undo

**Problem**: Simple undo/redo doesn't account for dependencies

**Example**:
```
> let x = 5
> let y = x * 2
> y
10
> undo  // What should happen?
// Option 1: Remove y binding
// Option 2: Undo x, invalidate y
```

**Solution**: Track dataflow dependencies

```rust
pub struct DataflowHistory {
    statements: Vec<Statement>,
    dependencies: HashMap<StmtId, HashSet<StmtId>>,
    current: usize,
}

impl DataflowHistory {
    pub fn undo(&mut self, db: &mut dyn ReplDb) -> Result<(), Error> {
        if self.current == 0 {
            return Ok(());
        }

        let stmt_to_undo = self.statements[self.current - 1];

        // Find all statements that depend on this one
        let dependents = self.find_dependents(stmt_to_undo);

        // Mark dependents as stale
        for dep in dependents {
            invalidate_statement(db, dep);
        }

        self.current -= 1;
        Ok(())
    }

    fn find_dependents(&self, stmt: StmtId) -> HashSet<StmtId> {
        let mut result = HashSet::new();
        let mut queue = vec![stmt];

        while let Some(current) = queue.pop() {
            if let Some(deps) = self.dependencies.get(&current) {
                for dep in deps {
                    if result.insert(*dep) {
                        queue.push(*dep);
                    }
                }
            }
        }

        result
    }
}
```

### 6.4 Replit-Style History Navigation

**Inspiration**: Replit History++ with operational transformations

**Approach**: Store commands/edits, not just states

```rust
pub enum ReplEdit {
    Execute { stmt_id: StmtId, source: String },
    Modify { stmt_id: StmtId, old_source: String, new_source: String },
    Delete { stmt_id: StmtId, source: String },
}

pub struct EditHistory {
    edits: Vec<ReplEdit>,
    current: usize,
}

impl EditHistory {
    pub fn apply_edit(&mut self, db: &mut dyn ReplDb, edit: ReplEdit) -> Result<(), Error> {
        // Apply edit
        match &edit {
            ReplEdit::Execute { stmt_id, source } => {
                execute_statement(db, *stmt_id, source)?;
            }
            ReplEdit::Modify { stmt_id, new_source, .. } => {
                modify_statement(db, *stmt_id, new_source)?;
            }
            ReplEdit::Delete { stmt_id, .. } => {
                delete_statement(db, *stmt_id)?;
            }
        }

        // Save edit
        self.edits.truncate(self.current + 1);
        self.edits.push(edit);
        self.current += 1;

        Ok(())
    }

    pub fn undo(&mut self, db: &mut dyn ReplDb) -> Result<(), Error> {
        if self.current == 0 {
            return Ok(());
        }

        let edit = &self.edits[self.current - 1];

        // Reverse the edit
        match edit {
            ReplEdit::Execute { stmt_id, .. } => {
                delete_statement(db, *stmt_id)?;
            }
            ReplEdit::Modify { stmt_id, old_source, .. } => {
                modify_statement(db, *stmt_id, old_source)?;
            }
            ReplEdit::Delete { stmt_id, source } => {
                execute_statement(db, *stmt_id, source)?;
            }
        }

        self.current -= 1;
        Ok(())
    }

    pub fn redo(&mut self, db: &mut dyn ReplDb) -> Result<(), Error> {
        if self.current >= self.edits.len() {
            return Ok(());
        }

        let edit = &self.edits[self.current].clone();
        self.apply_edit(db, edit)?;

        Ok(())
    }
}
```

**Advantage**: Can replay session deterministically

## 7. Choice Points and Backtracking

### 7.1 WAM Choice Points

**Warren Abstract Machine**: The foundation of Prolog implementation

**Key Data Structures**:

1. **Heap**: Stores terms (data structures)
2. **Stack**: Stores environments (local variables)
3. **Trail**: Records variable bindings (for undo)
4. **Choice Point Stack**: Saves alternative execution paths

**Choice Point Structure**:
```
struct ChoicePoint {
    continuation: CodePointer,       // Next clause to try
    environment: EnvironmentPointer, // Saved environment
    next_clause: ClausePointer,      // Alternative clauses
    trail_pointer: usize,            // Trail state for backtracking
    heap_pointer: usize,             // Heap state
}
```

**Backtracking Process**:
1. **Choice point created**: Multiple clauses match
2. **Try first clause**: If succeeds, continue
3. **On failure**: Pop to last choice point
4. **Restore state**: Reset heap, trail, environment
5. **Try next clause**: Continue with alternative
6. **Exhausted choices**: Backtrack further

**Key Instructions**:
- `try_me_else L`: Create choice point, try this clause first
- `retry_me_else L`: Restore state, try next clause
- `trust_me`: Last clause, remove choice point

### 7.2 Choice Points as Generators (Phase 1)

**Simpler Approach**: Use generators/iterators before full backtracking

**Key Insight**: Multi-deterministic functions are generators

```rust
// Multi function
#[multi]
fun parent(p: Person ^in) yields Person ^out {
    for child in db.children_of(p) {
        yield child
    }
}

// Compiles to:
fn parent_impl(p: &Person, db: &Database) -> impl Iterator<Item = Person> {
    db.children_of(p).clone().into_iter()
}
```

**Choice Point = Iterator State**:

```rust
pub enum GeneratorState<Y, R> {
    Yielded(Y),
    Complete(R),
}

pub trait Generator {
    type Yield;
    type Return;

    fn resume(&mut self) -> GeneratorState<Self::Yield, Self::Return>;
}

// Example: Split generator
struct SplitGenerator<T> {
    list: Vec<T>,
    index: usize,
}

impl<T: Clone> Generator for SplitGenerator<T> {
    type Yield = (Vec<T>, Vec<T>);
    type Return = ();

    fn resume(&mut self) -> GeneratorState<Self::Yield, Self::Return> {
        if self.index <= self.list.len() {
            let left = self.list[..self.index].to_vec();
            let right = self.list[self.index..].to_vec();
            self.index += 1;
            GeneratorState::Yielded((left, right))
        } else {
            GeneratorState::Complete(())
        }
    }
}
```

**REPL Integration**:

```
> splits([1, 2, 3])
=> ([], [1, 2, 3])

> next
=> ([1], [2, 3])

> next
=> ([1, 2], [3])

> next
=> ([1, 2, 3], [])

> all
=> [
    ([], [1, 2, 3]),
    ([1], [2, 3]),
    ([1, 2], [3]),
    ([1, 2, 3], [])
]
```

### 7.3 Full Backtracking (Phase 2)

**When Needed**: Complex search, constraint solving

**Implementation**: Explicit continuation stack

```rust
pub struct Continuation<'a> {
    // Function being executed
    func: &'a Function,

    // Current instruction pointer
    ip: usize,

    // Local environment
    locals: Vec<Value>,

    // Remaining choices at this point
    alternatives: Vec<Alternative>,
}

pub struct Alternative {
    // Next instruction to try
    ip: usize,

    // State to restore
    saved_locals: Vec<Value>,
}

pub struct BacktrackingVM {
    // Stack of continuations
    stack: Vec<Continuation>,

    // Trail for undoing variable bindings
    trail: Vec<TrailEntry>,
}

enum TrailEntry {
    Binding { var_id: VarId, old_value: Option<Value> },
}

impl BacktrackingVM {
    pub fn execute_with_backtracking(&mut self, func: &Function) -> Vec<Value> {
        let mut solutions = vec![];

        loop {
            match self.step() {
                StepResult::Yielded(value) => {
                    solutions.push(value);
                    // Try to find more solutions
                    self.backtrack();
                }
                StepResult::Failed => {
                    // Backtrack to last choice point
                    if !self.backtrack() {
                        // No more choices
                        break;
                    }
                }
                StepResult::Complete => break,
            }
        }

        solutions
    }

    fn backtrack(&mut self) -> bool {
        // Find continuation with alternatives
        while let Some(cont) = self.stack.last_mut() {
            if let Some(alt) = cont.alternatives.pop() {
                // Restore state
                cont.ip = alt.ip;
                cont.locals = alt.saved_locals;

                // Undo variable bindings
                self.restore_trail();

                return true;
            } else {
                // No more alternatives at this level
                self.stack.pop();
            }
        }

        false // No more choice points
    }

    fn restore_trail(&mut self) {
        while let Some(entry) = self.trail.pop() {
            match entry {
                TrailEntry::Binding { var_id, old_value } => {
                    // Restore old binding
                    self.set_variable(var_id, old_value);
                }
            }
        }
    }
}
```

**Example: N-Queens with Backtracking**:

```rust
#[nondet]
fun queens(n: @u32 ^in) yields [@u32] ^out {
    let board = make_board(n);

    // Choice point: try different positions
    for col in @0..n {
        if is_safe(board, @0, col) {
            let new_board = place_queen(board, @0, col);

            // Recursive search (creates nested choice points)
            if n == @1 {
                yield new_board
            } else {
                for solution in queens_from(new_board, @1, n) {
                    yield solution
                }
            }
        }
        // If not safe or no solution found, backtrack automatically
    }
}
```

### 7.4 Choice Point Memoization

**Problem**: Backtracking recomputes same branches

**Solution**: Memoize choice point results

```rust
pub struct MemoizedChoicePoint {
    // Cache solutions for each choice point
    cache: HashMap<(FunctionId, Vec<Value>), Vec<Vec<Value>>>,
}

impl MemoizedChoicePoint {
    pub fn execute_with_memo(
        &mut self,
        func_id: FunctionId,
        args: Vec<Value>,
        vm: &mut BacktrackingVM,
    ) -> Vec<Value> {
        // Check cache
        let key = (func_id, args.clone());
        if let Some(solutions) = self.cache.get(&key) {
            return solutions.concat();
        }

        // Compute all solutions
        let solutions = vm.execute_with_backtracking(func_id, args);

        // Group by choice point
        let grouped = self.group_by_choice_point(solutions);

        // Cache
        self.cache.insert(key, grouped.clone());

        grouped.concat()
    }
}
```

**Benefit**: Pure data + memoization = tabling (a la XSB Prolog)

## 8. Implementation Roadmap

### Phase 1: Tree-Walking Interpreter (Weeks 1-4)

**Goals**:
- Execute datalit expressions
- Basic datafun: let bindings, functions, function calls
- Salsa integration for REPL state

**Deliverables**:
1. `datalove-datafun-interp` crate
2. Tree-walking interpreter for Expr
3. Salsa queries for parsing, typechecking, eval
4. Basic REPL integration

**Success Criteria**:
- Can evaluate datalit expressions
- Can define and call simple functions
- REPL maintains state across statements

### Phase 2: Generators & Multi Functions (Weeks 5-8)

**Goals**:
- Implement multi-deterministic functions as generators
- REPL commands for exploring solutions (next, all)
- Memoization for multi functions

**Deliverables**:
1. Generator-based multi functions
2. REPL solution exploration
3. Basic memoization

**Success Criteria**:
- Can define functions that yield multiple solutions
- REPL can step through solutions
- Solutions are cached

### Phase 3: Enhanced REPL Features (Weeks 9-12)

**Goals**:
- Dataflow tracking
- Undo/redo
- Session persistence
- History navigation

**Deliverables**:
1. Dataflow dependency tracking
2. Command-based undo/redo
3. Session save/load
4. Rich history UI

**Success Criteria**:
- Undo invalidates dependent bindings
- Can save/load REPL sessions
- History navigation works smoothly

### Phase 4: Bytecode VM (Months 4-6)

**Goals**:
- Design bytecode format
- Implement compiler (AST → bytecode)
- Implement VM
- Performance testing

**Deliverables**:
1. Bytecode format
2. Compiler
3. Register-based VM
4. Benchmarks

**Success Criteria**:
- 10-100x faster than tree-walking
- All tests pass
- REPL feels snappy

### Phase 5: JIT with Cranelift (Months 7-9)

**Goals**:
- Cranelift integration
- Tiered compilation
- Profile-guided optimization

**Deliverables**:
1. Cranelift JIT backend
2. Tiered executor (VM + JIT)
3. Hot function profiling

**Success Criteria**:
- Hot functions run at native speed
- Compilation overhead is acceptable
- Stable, no crashes

### Phase 6: Advanced Features (Months 10-12)

**Goals**:
- Full backtracking
- AOT compilation
- WASM target

**Deliverables**:
1. Backtracking VM
2. AOT compiler
3. WASM backend

**Success Criteria**:
- Can solve constraint problems with backtracking
- Can compile modules ahead of time
- Can run datafun in browser

## 9. Recommendations

### 9.1 Start Simple

**Priority 1**: Get tree-walking interpreter working
- Focus on correctness first
- Leverage existing datalit infrastructure
- Use Salsa for incremental computation

**Priority 2**: Multi functions as generators
- Simpler than full backtracking
- Covers many use cases
- Foundation for choice points

**Priority 3**: REPL features (undo, dataflow, persistence)
- Makes REPL truly advanced
- Differentiates from other REPLs

### 9.2 Design for Evolution

**Architecture Principles**:
1. **Separation of concerns**: Parser, resolver, typechecker, interpreter
2. **Abstract execution**: Define interface that tree-walker, VM, JIT all implement
3. **Extensibility**: Easy to add new expression types, operations
4. **Testability**: Small, composable functions

**Example Abstract Executor**:

```rust
pub trait Executor {
    fn eval_expr(&mut self, expr: &Expr, env: &Environment) -> Result<Value, Error>;
    fn eval_statement(&mut self, stmt: &Statement) -> Result<Value, Error>;
    fn call_function(&mut self, func: &Function, args: Vec<Value>) -> Result<Value, Error>;
}

// Implementations:
struct TreeWalkingExecutor { ... }
struct BytecodeVMExecutor { ... }
struct JitExecutor { ... }
```

### 9.3 Leverage Salsa Everywhere

**Make everything a query**:
- Parsing
- Name resolution
- Type checking
- Even evaluation (with caveats)

**Benefit**: Automatic incremental recomputation

**Caveat**: Eval might be too dynamic
- Consider hybrid: Salsa for static analysis, interpreter for runtime

### 9.4 Pure Data is Your Superpower

**Exploit pure-data semantics**:
- Perfect memoization
- Deterministic replay
- Undo/redo for free
- Session persistence

**Don't Compromise**:
- Resist adding side effects
- Keep I/O in separate "proc" layer (future)
- Maintain value semantics

### 9.5 REPL-First Philosophy

**Design for interactive exploration**:
- Fast feedback (incremental recomputation)
- Explorable solutions (multi functions)
- Time travel (undo/redo)
- Introspection (show types, show deps)

**UI Considerations**:
- Show dataflow dependencies visually
- Highlight stale bindings
- Solution explorer for multi functions
- Timeline scrubber for history

## 10. Conclusion

Datalove has a unique opportunity to build a state-of-the-art REPL with:

1. **Salsa-powered incremental computation**: Automatic memoization and invalidation
2. **Dataflow-aware execution**: Like Observable/IPyflow
3. **Pure-data semantics**: Perfect memoization, deterministic replay
4. **Logic programming features**: Generators, multi functions, eventual backtracking
5. **Clear evolution path**: Tree-walk → Bytecode → JIT → AOT
6. **Advanced REPL features**: Undo/redo, session persistence, time travel

The existing infrastructure (rtdt ABI, datalit language, Salsa integration) provides a solid foundation.

**Recommended Strategy**:
- Start with tree-walking interpreter (simple, fast to implement)
- Focus on REPL UX (dataflow, undo/redo, exploration)
- Add generators for multi functions (powerful, simpler than full backtracking)
- Design for future optimization (bytecode, JIT) but don't implement yet
- Leverage pure data for perfect memoization and replay

This approach balances **simplicity** (start small), **elegance** (pure functional, dataflow-aware), and **performance path** (clear road to JIT/AOT), while delivering unique value through advanced REPL features.

## References

**Salsa**:
- https://github.com/salsa-rs/salsa
- rust-analyzer blog: "Durable Incrementality"

**Modern REPLs**:
- Observable: https://observablehq.com/
- IPyflow: https://github.com/ipyflow/ipyflow
- Marimo: https://marimo.io/

**JIT/AOT**:
- Cranelift: https://cranelift.dev/
- Wasmtime: https://wasmtime.dev/

**Logic Programming**:
- WAM: "Warren's Abstract Machine: A Tutorial Reconstruction"
- Mercury: https://mercurylang.org/
- Your own: `notes/research-logic-programming.md`

**General Interpreter Design**:
- "Crafting Interpreters" by Bob Nystrom
- "Modern Compiler Implementation in ML" by Andrew Appel
