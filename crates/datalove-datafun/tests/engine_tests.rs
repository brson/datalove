//! Every engine run on every program and checked against the IR walker.
//!
//! The IR walker is the reference: only its results are compared with the
//! fixture's expected file. Every other engine that applies to the fixture
//! runs it again in this process and must observe exactly what the reference
//! observed. See `botdocs/plan-engine-tests.md`.
//!
//! - **bytecode**: the whole analysis again, function bodies on the bytecode.
//! - **jit**: the IR walker with a JIT compiling every function on its first call.
//! - **chaos**: the bytecode under an `OptimizingDispatcher` that compiles,
//!   uses the JIT and inlines at random, seeded from the fixture.
//! - **aot**: the Cranelift AOT backend, for a program of one script fragment
//!   and no expressions that the reference ran to completion; its stderr must
//!   be the reference's debug log.
//! - **c**: the same through the C backend.
//! - **noconst**: the bytecode with const inlining off, so every `const` is
//!   evaluated at run time rather than at compile time.
//! - **nospec**: the bytecode with comptime specialization off.
//!
//! The last two compile differently, so they are held to the reference's
//! outputs and debug logs, not to its IR, and only for a program the reference
//! compiled without error: a `const` that fails to evaluate is a compile error,
//! and at run time it is a value. Under Miri only the interpreted
//! engines without a JIT run.
//!
//! A fixture opts out of an engine with a comment before its first section,
//! `// engines: -aot -c`, for what that backend does not support.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use rmx::prelude::*;

use datalove_datafun as datafun;
use datalove_datafun_cranelift_aot::AotCompiler;
use datalove_datafun_cranelift_jit::{DispatcherConfig, JitEngine, OptimizingDispatcher};
use datalove_datafun_c_aot::CAotCompiler;
use datalove_datafun_interp::{Engine, FunctionRegistry};
use datalove_datafun_ir::{expand_ir_strings, IrCodeUnit};
use datalove_datafun_pkg::package_load_worldfile::{self, ParsedWorldfile, WorldfileSection};
use datafun::pipeline::{CompilerOptions, LoweringResult, OwnershipResult, ScriptExecutor, TypecheckResult};
use datafun::worldfile_analysis::{Analysis, AnalysisOptions, ExecutorHooks};

/// Functions the chaos dispatcher JIT-compiled, over the whole corpus.
static CHAOS_COMPILED: AtomicU32 = AtomicU32::new(0);
/// Calls the chaos dispatcher inlined, over the whole corpus.
static CHAOS_INLINED: AtomicU32 = AtomicU32::new(0);
/// Fixtures each engine ran, in the order of `Differential::ALL`.
static RAN: [AtomicU32; Differential::ALL.len()] = [const { AtomicU32::new(0) }; Differential::ALL.len()];

/// The engines compared with the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Differential {
    Bytecode,
    Jit,
    Chaos,
    Aot,
    C,
    NoConst,
    NoSpec,
}

impl Differential {
    const ALL: [Differential; 7] = [
        Differential::Bytecode,
        Differential::Jit,
        Differential::Chaos,
        Differential::Aot,
        Differential::C,
        Differential::NoConst,
        Differential::NoSpec,
    ];

    fn name(self) -> &'static str {
        match self {
            Differential::Bytecode => "bytecode",
            Differential::Jit => "jit",
            Differential::Chaos => "chaos",
            Differential::Aot => "aot",
            Differential::C => "c",
            Differential::NoConst => "noconst",
            Differential::NoSpec => "nospec",
        }
    }
}

/// The engines a fixture's preamble opts out of.
fn opt_outs(source: &str) -> Result<Vec<Differential>, String> {
    let mut out = Vec::new();
    for line in source.lines().take_while(|l| !l.trim_start().starts_with("----------")) {
        let Some(list) = line.trim().strip_prefix("// engines:") else {
            continue;
        };
        for word in list.split_whitespace() {
            let name = word.strip_prefix('-')
                .ok_or_else(|| format!("`{word}` in `engines:` is not an opt-out like `-aot`"))?;
            let engine = Differential::ALL.into_iter().find(|e| e.name() == name)
                .ok_or_else(|| format!("no engine `{name}`"))?;
            out.push(engine);
        }
    }
    Ok(out)
}

/// Sets the interpreter's engine and, optionally, a dispatcher.
struct Setup {
    engine: Engine,
    dispatcher: Option<Box<dyn datalove_datafun_interp::CallDispatcher>>,
}

impl ExecutorHooks for Setup {
    fn configure(&mut self, executor: &mut ScriptExecutor) {
        executor.set_engine(self.engine);
        if let Some(dispatcher) = self.dispatcher.take() {
            executor.set_dispatcher(dispatcher);
        }
    }

    fn finish(&mut self, executor: &mut ScriptExecutor) {
        let Some(dispatcher) = executor.take_dispatcher() else {
            return;
        };
        if let Some(chaos) = dispatcher.as_any().downcast_ref::<OptimizingDispatcher>() {
            CHAOS_COMPILED.fetch_add(chaos.jit().stats().compiled_count, Ordering::Relaxed);
            CHAOS_INLINED.fetch_add(chaos.inliner().stats().inlinings_performed, Ordering::Relaxed);
        }
    }
}

/// Run the interpreted analysis under `setup` and render it as the expected
/// files record it.
fn analyze(parsed: &ParsedWorldfile, options: AnalysisOptions, setup: Setup) -> Result<(Analysis, String), String> {
    let mut db = datafun::Database::default();
    let mut setup = setup;
    let analysis = datafun::worldfile_analysis::analyze_worldfile_with_hooks(
        &mut db,
        parsed,
        options,
        &mut setup,
    ).map_err(|e| format!("analysis failed: {e}"))?;
    let config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .enumerate_arrays(false)
        .compact_arrays(false);
    let ron = ron::ser::to_string_pretty(&analysis, config)
        .map_err(|e| format!("failed to serialize to RON: {e}"))?;
    Ok((analysis, expand_ir_strings(&ron)))
}

/// What running a program observes, one line per script unit: its result
/// and its debug log.
fn observed(analysis: &Analysis) -> String {
    analysis.sections.iter()
        .filter(|s| s.section_type != "module")
        .map(|s| format!("{}: {:?} {:?}\n", s.section_type, s.output, s.debug_output.as_deref().unwrap_or("")))
        .collect()
}

/// Where two renderings first part, with a few lines of context.
fn first_difference(reference: &str, other: &str) -> String {
    let (r, o): (Vec<_>, Vec<_>) = (reference.lines().collect(), other.lines().collect());
    let at = r.iter().zip(&o).position(|(a, b)| a != b).unwrap_or(r.len().min(o.len()));
    let from = at.saturating_sub(3);
    let show = |lines: &[&str]| lines[from.min(lines.len())..(at + 4).min(lines.len())].join("\n");
    format!("at line {}:\n--- reference\n{}\n--- engine\n{}", at + 1, show(&r), show(&o))
}

/// Whether every unit compiled without error.
fn compiled_cleanly(reference: &Analysis) -> bool {
    reference.sections.iter().all(|s| {
        matches!(s.typecheck, TypecheckResult::Success)
            && matches!(s.ownership, OwnershipResult::Success)
            && matches!(s.lowering, LoweringResult::Success { .. })
    })
}

/// Whether the AOT backends apply: one fragment, no expressions, nothing but
/// modules beside it, and every unit compiled and run to completion by the
/// reference.
fn aot_applies(parsed: &ParsedWorldfile, reference: &Analysis) -> bool {
    let fragments = parsed.sections.iter()
        .filter(|s| matches!(s, WorldfileSection::ScriptFragment { .. }))
        .count();
    let only_modules = parsed.sections.iter().all(|s| matches!(s,
        WorldfileSection::Module { .. } | WorldfileSection::ScriptFragment { .. }));
    let all_ran = reference.sections.iter()
        .all(|s| s.section_type == "module" || s.output == "(fragment executed)");
    fragments == 1 && only_modules && compiled_cleanly(reference) && all_ran
}

/// Compile the fragment for an AOT backend and hand it to `build_and_run`,
/// which returns the program's stderr.
fn run_aot(
    parsed: &ParsedWorldfile,
    build_and_run: fn(&IrCodeUnit, &FunctionRegistry, &Path) -> Result<String, String>,
) -> Result<String, String> {
    let db = datafun::Database::default();
    let mut pipeline = datafun::pipeline::ModuleCompilationPipeline::from_sections(
        &db, &parsed.sections, CompilerOptions::default());
    let compiled = pipeline.compile_fresh(&db);
    let mut compiler = compiled.script_compiler_default(&db)
        .ok_or("the modules did not compile")?;
    let source = parsed.sections.iter()
        .find_map(|s| match s {
            WorldfileSection::ScriptFragment { source } => Some(source.as_str()),
            _ => None,
        })
        .X();
    let unit = compiler.compile_fragment(source).ir_unit.ok_or("the fragment did not compile")?;
    let dir = rmx::tempfile::tempdir().map_err(|e| e.to_string())?;
    build_and_run(&unit, &compiled.module_registry(), dir.path())
}

/// Run an executable, returning its stderr if it succeeded.
fn run_exe(exe: &Path) -> Result<String, String> {
    let output = Command::new(exe).output().map_err(|e| format!("failed to run: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(format!("exited with {}:\n{stderr}", output.status));
    }
    Ok(stderr)
}

fn cranelift_build_and_run(unit: &IrCodeUnit, registry: &FunctionRegistry, dir: &Path) -> Result<String, String> {
    let mut compiler = AotCompiler::new_for_host().map_err(|e| e.to_string())?;
    let product = compiler
        .compile_script_unit_with_world_types(unit, registry.iter_all_code_units(), registry)
        .map_err(|e| format!("compile failed: {e}"))?;
    let object = product.emit().map_err(|e| format!("emit failed: {e}"))?;
    let exe = dir.join("test");
    datafun::pipeline::aot::link_object_to_path(&object, &exe).map_err(|e| format!("link failed: {e}"))?;
    run_exe(&exe)
}

fn c_build_and_run(unit: &IrCodeUnit, registry: &FunctionRegistry, dir: &Path) -> Result<String, String> {
    let output = CAotCompiler::new().compile_world(unit, registry).map_err(|e| format!("compile failed: {e}"))?;
    let mut cmd = Command::new("cc");
    cmd.args(["-std=c11", "-O0", "-g"]);
    if datafun::pipeline::aot::use_lld() {
        cmd.arg("-fuse-ld=lld");
    }
    for (filename, content) in &output.files {
        let path = dir.join(filename);
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
        cmd.arg(path);
    }
    cmd.arg(datafun::pipeline::aot::runtime_only_component().map_err(|e| e.to_string())?);
    let exe = dir.join("test");
    cmd.args(["-ldl", "-lpthread", "-lm", "-o"]).arg(&exe);
    let cc = cmd.output().map_err(|e| format!("failed to run cc: {e}"))?;
    if !cc.status.success() {
        return Err(format!("cc failed:\n{}", String::from_utf8_lossy(&cc.stderr)));
    }
    run_exe(&exe)
}

/// Run every applicable engine on a fixture, returning the reference's
/// rendering for the expected file.
fn check(path: &Path) -> Result<String, String> {
    let source = std::fs::read_to_string(path).map_err(|e| format!("failed to read: {e}"))?;
    let skip = opt_outs(&source)?;
    let parsed = package_load_worldfile::parse_worldfile_sections(source.as_bytes())
        .map_err(|e| format!("failed to parse worldfile: {e}"))?;

    let (reference, rendered) = analyze(&parsed, AnalysisOptions::default(), Setup { engine: Engine::IrWalker, dispatcher: None })?;

    let mut seed = DefaultHasher::new();
    source.hash(&mut seed);
    let seed = seed.finish();

    for (i, engine) in Differential::ALL.into_iter().enumerate() {
        if skip.contains(&engine) {
            continue;
        }
        if cfg!(miri) && !matches!(engine, Differential::Bytecode | Differential::NoConst | Differential::NoSpec) {
            continue;
        }
        let variant = match engine {
            Differential::NoConst => Some(AnalysisOptions { skip_const_inlining: true, ..AnalysisOptions::default() }),
            Differential::NoSpec => Some(AnalysisOptions { skip_specialization: true, ..AnalysisOptions::default() }),
            _ => None,
        };
        if let Some(options) = variant {
            if !compiled_cleanly(&reference) {
                continue;
            }
            RAN[i].fetch_add(1, Ordering::Relaxed);
            let (other, _) = analyze(&parsed, options, Setup { engine: Engine::Bytecode, dispatcher: None })?;
            let (want, got) = (observed(&reference), observed(&other));
            if want != got {
                return Err(format!("{} differs from the reference {}", engine.name(), first_difference(&want, &got)));
            }
            continue;
        }
        let interpreted = match engine {
            Differential::Bytecode => Some(Setup { engine: Engine::Bytecode, dispatcher: None }),
            Differential::Jit => Some(Setup {
                engine: Engine::IrWalker,
                dispatcher: Some(Box::new(JitEngine::new(1).map_err(|e| e.to_string())?)),
            }),
            Differential::Chaos => Some(Setup {
                engine: Engine::Bytecode,
                dispatcher: Some(Box::new(
                    OptimizingDispatcher::with_config(DispatcherConfig::chaos(seed)).map_err(|e| e.to_string())?,
                )),
            }),
            Differential::Aot | Differential::C | Differential::NoConst | Differential::NoSpec => None,
        };
        if let Some(setup) = interpreted {
            RAN[i].fetch_add(1, Ordering::Relaxed);
            let (_, other) = analyze(&parsed, AnalysisOptions::default(), setup)?;
            if other != rendered {
                return Err(format!("{} differs from the reference {}", engine.name(), first_difference(&rendered, &other)));
            }
            continue;
        }

        if !aot_applies(&parsed, &reference) {
            continue;
        }
        let build_and_run = match engine {
            Differential::Aot => cranelift_build_and_run,
            _ => c_build_and_run,
        };
        RAN[i].fetch_add(1, Ordering::Relaxed);
        let stderr = run_aot(&parsed, build_and_run).map_err(|e| format!("{}: {e}", engine.name()))?;
        let fragment = reference.sections.iter().find(|s| s.section_type == "scriptunit-fragment").X();
        let expected = fragment.debug_output.as_deref().unwrap_or("");
        if stderr != expected {
            return Err(format!("{} differs from the reference {}", engine.name(), first_difference(expected, &stderr)));
        }
    }

    Ok(rendered)
}

/// Report what each engine ran; and the chaos dispatcher has to have done
/// something somewhere, or it tested nothing.
fn report() -> Result<(), String> {
    let ran: Vec<_> = Differential::ALL.iter().zip(&RAN)
        .map(|(e, n)| format!("{} {}", e.name(), n.load(Ordering::Relaxed)))
        .collect();
    println!("fixtures run per engine: {}", ran.join(", "));
    if cfg!(miri) || !datalove_exampletest::parse_test_filters().is_empty() {
        return Ok(());
    }
    let (compiled, inlined) = (CHAOS_COMPILED.load(Ordering::Relaxed), CHAOS_INLINED.load(Ordering::Relaxed));
    println!("chaos: {compiled} functions JIT-compiled, {inlined} calls inlined");
    if compiled == 0 || inlined == 0 {
        return Err(format!("the chaos dispatcher never engaged: {compiled} compiled, {inlined} inlined"));
    }
    Ok(())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), check)
        .fixture_subdir("engines")
        .file_extension("world")
        .allow_errors(true)
        .after_all(report)
        .run();
}
