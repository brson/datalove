# IR Serialization for Module Worlds

## Overview

This document describes how to serialize all IR needed for executing a module world, then load and run it. This enables:
- **Compilation artifacts**: Save compiled IR to disk for later execution
- **Distribution**: Share compiled modules without source code
- **Caching**: Avoid re-compilation in development/testing
- **AOT compilation targets**: Prepare IR for LLVM/Cranelift backends

## Current State

All core IR types already derive `Serialize` and `Deserialize` via serde:
- `ValueId`, `SlotId`, `ParamId`, `BlockId`, `FuncId`, `IrModuleId`
- `Operand`, `SlotDest`, `ConstValue`, `BinOp`, `UnaryOp`, `ParamMode`
- `Instruction`, `Terminator`, `IrBlock`, `IrFunction`, `IrModule`
- `IrType`, `TypeRef`, `FuncRef`, `ExportBinding`, `IrScriptUnit`
- `SymbolTable`, `FuncDef`

The `FunctionRegistry` exists but doesn't derive Serialize (stores HashMap, needs custom serialization).

## Serialization Design

### Top-Level Structure

```rust
/// Complete IR compilation unit for a module world.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IrCompilationUnit {
    /// Version of the IR format (for future compatibility).
    pub version: u32,

    /// All compiled modules with their functions.
    pub modules: Vec<IrModuleData>,

    /// Entry point specification.
    pub entry_point: EntryPoint,

    /// Metadata about the compilation.
    pub metadata: CompilationMetadata,
}

/// Data for a single compiled module.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IrModuleData {
    /// Unique module ID within this compilation unit.
    pub module_id: IrModuleId,

    /// Module path (e.g., "local/test/main").
    pub path: String,

    /// All functions defined in this module.
    pub functions: Vec<IrFunction>,
}

/// Entry point for execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EntryPoint {
    /// Execute a specific function by module path and name.
    Function {
        module_path: String,
        function_name: String,
    },

    /// Execute the "main" function from "local/test/main" (testing convention).
    MainFunction,

    /// Script units (for REPL/interactive execution).
    ScriptUnits {
        units: Vec<IrScriptUnit>,
    },
}

/// Metadata about the compilation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompilationMetadata {
    /// When this was compiled (ISO 8601 timestamp).
    pub compiled_at: String,

    /// Compiler version.
    pub compiler_version: String,

    /// Module dependency graph (for validation).
    pub dependencies: Vec<(String, Vec<String>)>,
}
```

### Serialization Format

Use **bincode** for compact binary format:

```rust
use bincode;

// Serialize
let compilation_unit = IrCompilationUnit { ... };
let bytes = bincode::serialize(&compilation_unit)?;
std::fs::write("output.dflir", bytes)?;

// Deserialize
let bytes = std::fs::read("output.dflir")?;
let compilation_unit: IrCompilationUnit = bincode::deserialize(&bytes)?;
```

Alternative formats:
- **JSON** - Human-readable, debugging-friendly, larger size
- **MessagePack** - Compact binary, more portable than bincode
- **Custom format** - Optimized for specific use cases

## Loading and Execution

### Phase 1: Deserialize

```rust
pub fn load_compilation_unit(path: &Path) -> AnyResult<IrCompilationUnit> {
    let bytes = std::fs::read(path)?;
    let unit: IrCompilationUnit = bincode::deserialize(&bytes)?;

    // Validate version.
    if unit.version != CURRENT_IR_VERSION {
        bail!("IR version mismatch: expected {}, got {}",
              CURRENT_IR_VERSION, unit.version);
    }

    Ok(unit)
}
```

### Phase 2: Build Execution Environment

```rust
pub fn create_execution_environment(
    unit: &IrCompilationUnit
) -> AnyResult<ScriptEnvironment> {
    let mut env = ScriptEnvironment::new();

    // Register all module functions.
    for module_data in &unit.modules {
        for function in &module_data.functions {
            env.add_module_function(
                module_data.module_id,
                function.id,
                function.clone(),
            );
        }
    }

    Ok(env)
}
```

### Phase 3: Resolve Entry Point

```rust
pub fn resolve_entry_point(
    unit: &IrCompilationUnit,
    env: &ScriptEnvironment,
) -> AnyResult<(IrModuleId, FuncId)> {
    match &unit.entry_point {
        EntryPoint::Function { module_path, function_name } => {
            // Find module by path.
            let module_data = unit.modules.iter()
                .find(|m| m.path == *module_path)
                .ok_or_else(|| anyhow!("Module not found: {}", module_path))?;

            // Find function by name.
            let function = module_data.functions.iter()
                .find(|f| f.name == *function_name)
                .ok_or_else(|| anyhow!("Function not found: {}", function_name))?;

            Ok((module_data.module_id, function.id))
        }

        EntryPoint::MainFunction => {
            // Convention: main function in local/test/main.
            let module_data = unit.modules.iter()
                .find(|m| m.path == "local/test/main")
                .ok_or_else(|| anyhow!("Module local/test/main not found"))?;

            let function = module_data.functions.iter()
                .find(|f| f.name == "main")
                .ok_or_else(|| anyhow!("main function not found"))?;

            Ok((module_data.module_id, function.id))
        }

        EntryPoint::ScriptUnits { .. } => {
            bail!("Script unit entry points not supported for module-only execution")
        }
    }
}
```

### Phase 4: Execute

```rust
pub fn execute_module_world(path: &Path) -> AnyResult<String> {
    // Load.
    let unit = load_compilation_unit(path)?;

    // Build environment.
    let env = create_execution_environment(&unit)?;

    // Resolve entry point.
    let (module_id, func_id) = resolve_entry_point(&unit, &env)?;

    let function = env.registry.get_module_function(module_id, func_id)
        .ok_or_else(|| anyhow!("Entry function not in registry"))?;

    // Verify entry point is nullary.
    if !function.params.is_empty() {
        bail!("Entry point must have no parameters");
    }

    // Create interpreter.
    let mut interp = datalove_datafun_interp::IrInterpreter::new();

    // Allocate return value buffer.
    let ret_tydesc = interp.tydesc_table_mut()
        .get_or_create(&function.return_type);
    let (ret_size, ret_align) = unsafe {
        ((*ret_tydesc).size, (*ret_tydesc).align)
    };
    let mut ret_buffer = AlignedBuffer::with_align(
        ret_size as usize,
        ret_align as usize
    );

    // Execute function.
    interp.execute_function_with_env(
        function,
        &[],  // No args
        &env,
        datalove_datafun_interp::Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        },
    )?;

    // Pretty-print result.
    let value = datalove_datafun_interp::Value {
        ptr: ret_buffer.as_mut_ptr(),
        tydesc: ret_tydesc,
    };
    let output = interp.pretty_print_value(&value)?;

    // Cleanup.
    interp.destroy_value(&value)?;

    Ok(output)
}
```

## Compilation Pipeline Integration

### Extend `CompiledModules`

```rust
impl<'db> CompiledModules<'db> {
    /// Serialize compiled modules to an IR compilation unit.
    pub fn to_compilation_unit(&self) -> IrCompilationUnit {
        // Collect module data.
        let mut modules = Vec::new();

        for (ir_module_idx, module) in self.module_graph.iter_modules(self.db).enumerate() {
            let ir_module_id = IrModuleId(ir_module_idx as u32);
            let module_path = module.id(self.db).path(self.db).clone();

            // Collect all functions for this module.
            let functions: Vec<IrFunction> = self.env.registry
                .iter_module_functions_with_ids()
                .filter(|((mid, _), _)| *mid == ir_module_id)
                .map(|(_, func)| func.clone())
                .collect();

            modules.push(IrModuleData {
                module_id: ir_module_id,
                path: module_path,
                functions,
            });
        }

        // Create entry point (convention: main from local/test/main).
        let entry_point = EntryPoint::MainFunction;

        // Create metadata.
        let metadata = CompilationMetadata {
            compiled_at: chrono::Utc::now().to_rfc3339(),
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            dependencies: self.collect_dependencies(),
        };

        IrCompilationUnit {
            version: CURRENT_IR_VERSION,
            modules,
            entry_point,
            metadata,
        }
    }

    /// Save compiled modules to a file.
    pub fn save_to_file(&self, path: &Path) -> AnyResult<()> {
        let unit = self.to_compilation_unit();
        let bytes = bincode::serialize(&unit)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }
}
```

### CLI Integration

```bash
# Compile a worldfile to IR
datalove compile input.world -o output.dflir

# Execute compiled IR
datalove run output.dflir

# Compile and run (for testing)
datalove test input.world
```

## File Format Specification

### Binary Format (`.dflir`)

```
Magic: 0x44464C49 ("DFLI" in ASCII)
Version: u32 (4 bytes)
Length: u64 (8 bytes) - total size of serialized data
Data: bincode-encoded IrCompilationUnit
Checksum: CRC32 (4 bytes) - for corruption detection
```

### JSON Format (`.dflir.json`)

Human-readable version for debugging:

```json
{
  "version": 1,
  "modules": [
    {
      "module_id": 0,
      "path": "local/test/main",
      "functions": [
        {
          "id": 0,
          "name": "main",
          "params": [],
          "param_modes": [],
          "param_types": [],
          "return_type": "u32",
          "blocks": [...]
        }
      ]
    }
  ],
  "entry_point": {
    "type": "MainFunction"
  },
  "metadata": {
    "compiled_at": "2026-01-07T12:00:00Z",
    "compiler_version": "0.1.0",
    "dependencies": []
  }
}
```

## Cross-Unit References (Script Units)

For worldfiles with script units, additional serialization needed:

```rust
/// Extended compilation unit with script units.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IrScriptCompilationUnit {
    /// Base module data.
    pub base: IrCompilationUnit,

    /// Sequential script units.
    pub script_units: Vec<IrScriptUnit>,

    /// Cross-unit binding metadata.
    pub script_context: SerializedScriptContext,
}

/// Serializable version of ScriptLowerContext.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerializedScriptContext {
    /// Let bindings: name -> (unit, ValueId, IrType).
    pub values: Vec<(String, u32, ValueId, IrType)>,

    /// Var bindings: name -> (unit, SlotId, IrType).
    pub slots: Vec<(String, u32, SlotId, IrType)>,

    /// Function bindings: name -> (unit, FuncId).
    pub functions: Vec<(String, u32, FuncId)>,
}
```

## Execution Model Comparison

| Aspect | Interpreter (Current) | Serialized IR | AOT (Future) |
|--------|----------------------|---------------|--------------|
| Input | Worldfile source | `.dflir` file | Native binary |
| Parse | Every run | Once (at compile) | Once |
| Typecheck | Every run | Once (at compile) | Once |
| Lower | Every run | Once (at compile) | Once |
| Execute | Bytecode interp | Bytecode interp | Native code |
| Startup | Slow (full pipeline) | Fast (deserialize) | Fastest (no IR) |
| Distribution | Source + compiler | IR file + runtime | Binary only |

## Implementation Plan

### Phase 1: Core Serialization (Module-Only)
- [ ] Define `IrCompilationUnit` and related types
- [ ] Implement `to_compilation_unit()` for `CompiledModules`
- [ ] Add `save_to_file()` method
- [ ] Test serialization round-trip

### Phase 2: Loading and Execution
- [ ] Implement `load_compilation_unit()`
- [ ] Implement `create_execution_environment()`
- [ ] Implement `resolve_entry_point()`
- [ ] Implement `execute_module_world()`
- [ ] Test with existing module worldfile tests

### Phase 3: CLI Integration
- [ ] Add `compile` subcommand
- [ ] Add `run` subcommand for `.dflir` files
- [ ] Update `test` subcommand to optionally use cached IR

### Phase 4: Script Unit Support
- [ ] Define `IrScriptCompilationUnit`
- [ ] Implement serialization of `ScriptLowerContext`
- [ ] Handle cross-unit references during deserialization
- [ ] Test with script unit worldfiles

### Phase 5: Optimization
- [ ] Add compression (zstd/lz4)
- [ ] Implement incremental compilation (cache per module)
- [ ] Add signature/checksums for validation
- [ ] Measure performance improvements

## Security Considerations

### Untrusted IR Files

When loading IR from untrusted sources:
1. **Version validation** - Reject unknown IR versions
2. **Size limits** - Prevent resource exhaustion
3. **ID validation** - Ensure ValueId/SlotId/BlockId are in bounds
4. **Type safety** - Validate all IrType structures
5. **CFG validation** - Ensure blocks are well-formed, no infinite loops in structure

### Safe Deserialization

```rust
pub fn load_compilation_unit_safe(path: &Path) -> AnyResult<IrCompilationUnit> {
    // Check file size.
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_IR_FILE_SIZE {
        bail!("IR file too large: {} bytes", metadata.len());
    }

    // Read and deserialize.
    let bytes = std::fs::read(path)?;
    let unit: IrCompilationUnit = bincode::deserialize(&bytes)
        .context("Failed to deserialize IR")?;

    // Validate version.
    if unit.version != CURRENT_IR_VERSION {
        bail!("IR version mismatch");
    }

    // Validate all modules.
    for module_data in &unit.modules {
        validate_ir_module(module_data)?;
    }

    Ok(unit)
}

fn validate_ir_module(module: &IrModuleData) -> AnyResult<()> {
    for function in &module.functions {
        // Validate value/slot IDs are in bounds.
        if function.value_count as usize != function.value_types.len() {
            bail!("Value count mismatch in {}", function.name);
        }
        if function.slot_count as usize != function.slot_types.len() {
            bail!("Slot count mismatch in {}", function.name);
        }

        // Validate blocks.
        for block in &function.blocks {
            validate_ir_block(block, function)?;
        }
    }
    Ok(())
}

fn validate_ir_block(block: &IrBlock, func: &IrFunction) -> AnyResult<()> {
    // Validate all operands reference valid IDs.
    for instr in &block.instructions {
        validate_instruction(instr, func)?;
    }
    validate_terminator(&block.terminator, func)?;
    Ok(())
}
```

## Benefits

1. **Faster testing** - Skip parsing/typechecking in test runs
2. **Distribution** - Share compiled modules without source
3. **Caching** - Incremental compilation in development
4. **AOT preparation** - IR is input to LLVM/Cranelift backends
5. **Debugging** - JSON format allows IR inspection
6. **Reproducibility** - Exact IR can be archived and replayed

## Future Extensions

### Module Linking
Link multiple `.dflir` files into a single executable:
```bash
datalove link module1.dflir module2.dflir -o combined.dflir
```

### Separate Compilation
Compile modules independently, resolve imports at link time.

### Debug Information
Embed source locations and names for better error messages:
```rust
pub struct DebugInfo {
    pub source_map: HashMap<ValueId, SourceLocation>,
    pub variable_names: HashMap<ValueId, String>,
}
```

### Optimization Passes
Run optimizations on serialized IR before execution:
- Dead code elimination
- Constant folding
- Inlining
- Loop unrolling
