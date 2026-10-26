//! The standard library fixtures, compiled in one process and run in another.
//!
//! Miri can check the interpreter, the runtime and the std rider, which hold
//! most of the unsafe code, but compiling `sys/std` under Miri takes hours
//! before a fixture runs, and the compiler has no unsafe code to check. So the
//! two halves run apart:
//!
//! - Natively, this compiles the library and each fixture in
//!   `fixtures/std_tests`, and writes each fixture's two script units (its
//!   body, and `output`), with the library functions they can reach, to
//!   `target/ir-replay/std`. Then it replays them, as below, to check that what
//!   it wrote runs.
//! - Under Miri, it only replays: it reads the functions and units back,
//!   runs them on the IR walker, the bytecode and the plain bytecode, the body
//!   twice as an edit would rerun it, with the
//!   std rider's natives linked into this binary, and compares the output with
//!   the fixture's expected file.
//!
//! So `just test-miri-replay` is the native run followed by the Miri one.
//! Arguments filter fixtures by name, as in the other fixture suites.

use rmx::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use datalove_datafun as datafun;
use datalove_datafun_interp::{Engine, ModuleFunctionRegistry};
use datalove_datafun_ir::{CodeRef, CodeUnitId, IrCodeUnit, IrModuleId};
use datafun::pipeline::{LoweringResult, ScriptExecutor, TypecheckResult, WorkspaceDescriptor};

/// What one fixture compiled to.
#[derive(Serialize, Deserialize)]
struct Prepared {
    /// The library functions the two units can reach, and no others: the
    /// whole library is most of a megabyte, and parsing it under Miri took
    /// longer than running a fixture.
    registry: ModuleFunctionRegistry,
    fragment: IrCodeUnit,
    output: IrCodeUnit,
}

const ENGINES: [Engine; 3] = [Engine::IrWalker, Engine::Bytecode, Engine::PlainBytecode];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().X().parent().X().to_path_buf()
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/std_tests")
}

fn out_dir() -> PathBuf {
    repo_root().join("target/ir-replay/std")
}

/// The fixtures named by the filters, by stem, in order.
fn fixtures(filters: &[String]) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(fixtures_dir()).X()
        .map(|entry| entry.X().file_name().to_string_lossy().into_owned())
        .filter_map(|name| name.strip_suffix(".dfs").map(str::to_string))
        .filter(|name| datalove_exampletest::matches_filters(name, filters))
        .collect();
    names.sort();
    names
}

/// Compile the library and each fixture, and write what they compiled to.
fn prepare(names: &[String]) {
    let work_dir = repo_root().join("target/datalove-work/ir_replay_tests");
    let descriptor = rmx::futures::executor::block_on(
        WorkspaceDescriptor::load_sys_dir(repo_root().join("sys"))
    ).expect("the standard library's sources must load")
        .with_work_dir(work_dir);
    let mut world = datafun::pipeline::CompiledWorld::new(&descriptor);
    let out = out_dir();
    std::fs::create_dir_all(&out).X();

    world.with_compiled(|db, compiled| {
        assert!(compiled.resolution_error.is_none(), "the standard library does not resolve");
        let registry = compiled.module_registry();
        let registry: &ModuleFunctionRegistry = registry.module_registry();

        for name in names {
            let source = std::fs::read_to_string(fixtures_dir().join(format!("{name}.dfs"))).X();
            let path = out.join(format!("{name}.ron"));
            let _ = std::fs::remove_file(&path);
            let mut compiler = compiled.script_compiler_default(db).expect("the library compiles");
            let Some(fragment) = compiled_unit(compiler.compile_fragment(&source)) else {
                println!("  skip  {name}: does not compile");
                continue;
            };
            let Some(output) = compiled_unit(compiler.compile_expr("output")) else {
                println!("  skip  {name}: `output` does not compile");
                continue;
            };
            let registry = reachable(registry, &[&fragment, &output]);
            let prepared = Prepared { registry, fragment, output };
            std::fs::write(&path, ron::to_string(&prepared).X()).X();
        }
    });
}

/// The part of `registry` that `roots` call, directly or through each other.
fn reachable(registry: &ModuleFunctionRegistry, roots: &[&IrCodeUnit]) -> ModuleFunctionRegistry {
    /// The module functions `unit` and the units nested in it call; a local
    /// call from a module function is to its own module.
    fn calls(unit: &IrCodeUnit, module: Option<IrModuleId>, out: &mut Vec<(IrModuleId, CodeUnitId)>) {
        for block in &unit.blocks {
            for instr in &block.instructions {
                match instr.call_target() {
                    Some(CodeRef::Module { module, id }) => out.push((*module, *id)),
                    Some(CodeRef::Local(id)) => out.extend(module.map(|m| (m, *id))),
                    _ => {}
                }
            }
        }
        for nested in &unit.nested_units {
            calls(nested, module, out);
        }
    }

    let mut pending = Vec::new();
    for root in roots {
        calls(root, None, &mut pending);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut pruned = ModuleFunctionRegistry::new();
    while let Some((module, id)) = pending.pop() {
        if !seen.insert((module, id)) {
            continue;
        }
        let unit = registry.get_module_function(module, id)
            .unwrap_or_else(|| panic!("a call to {module:?} {id:?}, which the registry lacks"));
        calls(unit, Some(module), &mut pending);
        pruned.add_module_code_unit(module, id, unit.clone());
    }
    pruned
}

fn compiled_unit(result: datafun::pipeline::ScriptCompilationResult) -> Option<IrCodeUnit> {
    if matches!(result.typecheck, TypecheckResult::Error { .. })
        || matches!(result.lowering, LoweringResult::Error { .. }) {
        return None;
    }
    result.ir_unit
}

/// Run each prepared fixture on each engine, returning the failures.
fn replay(names: &[String]) -> Vec<String> {
    let out = out_dir();
    let natives: Vec<(String, *const ())> = datalove_rider_sys_std::symbols().into_iter()
        .map(|(symbol, addr)| (symbol.to_string(), addr))
        .collect();

    let mut failures = Vec::new();
    for name in names {
        let Ok(text) = std::fs::read_to_string(out.join(format!("{name}.ron"))) else {
            continue;
        };
        let prepared: Prepared = ron::from_str(&text).X();
        let registry = Arc::new(prepared.registry);
        let wanted: Vec<String> = registry.iter_module_code_units()
            .filter_map(|unit| unit.native_context().map(|ctx| ctx.symbol().to_string()))
            .collect();
        let expected = std::fs::read_to_string(fixtures_dir().join(format!("{name}.out.expected")))
            .unwrap_or_default();
        for engine in ENGINES {
            let mut executor = ScriptExecutor::new(
                registry.clone(), datalove_rt::c::DebugOutputMode::Disabled, None);
            executor.set_engine(engine);
            let table = executor.native_table_mut();
            for symbol in &wanted {
                let (_, addr) = natives.iter().find(|(s, _)| s == symbol)
                    .unwrap_or_else(|| panic!("{symbol} is not a std native"));
                table.register_c(symbol.clone(), *addr);
            }
            // Run twice, the second time in place of the first, the way an
            // edit reruns a unit: that forgets the compiled bodies and
            // destroys what the first run left.
            let ran = executor.execute_fragment(&prepared.fragment);
            let ran = if ran.starts_with("Error:") {
                ran
            } else {
                executor.rerun_unit(0, &prepared.fragment).1
            };
            let value = if ran.starts_with("Error:") {
                ran
            } else {
                executor.execute_expr(&prepared.output).1
            };
            executor.destroy_live_values();
            if value.trim_end() == expected.trim_end() {
                println!("  ok    {name} {engine:?}");
            } else {
                println!("  FAIL  {name} {engine:?}\n    expected: {}\n    got:      {value}", expected.trim_end());
                failures.push(format!("{name} {engine:?}"));
            }
        }
    }
    failures
}

fn main() {
    let filters = datalove_exampletest::parse_test_filters();
    let names = fixtures(&filters);
    if !cfg!(miri) {
        prepare(&names);
    }
    let failures = replay(&names);
    if !failures.is_empty() {
        println!("{} failed: {}", failures.len(), failures.join(", "));
        std::process::exit(1);
    }
}
