//! Every engine run on a program and checked against the IR walker.
//!
//! The IR walker is the reference. Every other engine that applies to the
//! program runs it again in this process and must observe exactly what the
//! reference observed. `engine_tests` hands this its fixtures, and
//! `worldgen_engine_tests` generated worldfiles.
//!
//! - **bytecode**: the whole analysis again, function bodies on the bytecode.
//! - **plain**: the same on the bytecode as lowered, without its optimization
//!   pass, so that every rule of the pass is checked against what it rewrote.
//! - **jit**: the IR walker with a JIT compiling every function on its first call.
//! - **chaos**: the bytecode under an `OptimizingDispatcher` that compiles,
//!   and uses the JIT at random, seeded from the program.
//! - **osr**: the bytecode under a `JitEngine` that compiles no function by its
//!   calls and enters every loop of one after one to three iterations, chosen
//!   by the program's seed: each function's loops, and what runs after them,
//!   in code entered partway through the call.
//! - **aot**: the Cranelift AOT backend, for a program of one script fragment
//!   and no expressions that the reference ran to completion; its stderr must
//!   be the reference's debug log.
//! - **c**: the same through the C backend.
//! - **noconst**: the bytecode with const inlining off, so every `const` is
//!   evaluated at run time rather than at compile time.
//! - **nospec**: the bytecode with comptime specialization off.
//! - **spec**: the IR walker with every call to a generic at concrete bindings
//!   pointed at an instance of it (`GenericsPolicy::All`).
//! - **specchaos**: the same, a coin per call site, seeded from the program.
//!
//! The last four compile differently, so they are held to the reference's
//! outputs and debug logs, not to its IR, and only for a program the reference
//! compiled without error: a `const` that fails to evaluate is a compile error,
//! and at run time it is a value. Under Miri only the interpreted
//! engines without a JIT run.
//!
//! A program opts out of an engine with a comment before its first section,
//! `// engines: -aot -c`, for what that backend does not support.
//!
//! It can also say what has to have run, since an engine that quietly falls
//! back to the reference's way of running something agrees with the reference
//! and tests nothing:
//!
//! - `// expect-bytecode-ops: ListStoreAt BrListBounds`: the bytecode run
//!   lowered some body to each of these ops.
//! - `// expect-bytecode-lowered: RefStore ListSet`: no body the bytecode
//!   lowered left an instruction of these kinds to the IR walker.
//! - `// expect-osr: entered`: the osr run entered compiled code at some loop,
//!   and was refused at none.

use datalove_datafun_compiler::instances::GenericsPolicy;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use rmx::prelude::*;

use bct::random::{SeededRng, derive_seed, seed_of};
use datalove_datafun as datafun;
use datalove_datafun_cranelift_aot::AotCompiler;
use datalove_datafun_cranelift_jit::{DispatcherConfig, JitEngine, OptimizingDispatcher};
use datalove_datafun_c_aot::CAotCompiler;
use datalove_datafun_interp::{Engine, FunctionRegistry};
use datalove_datafun_ir::{expand_ir_strings, IrCodeUnit};
use datalove_datafun_pkg::package_load_worldfile::{self, ParsedWorldfile, WorldfileSection};
use datafun::driver::rider_build;
use datafun::driver::{CompilerOptions, SystemLibrary, LoweringResult, OwnershipResult, ScriptExecutor, TypecheckResult};
use datafun::worldfile_analysis::{Analysis, AnalysisOptions, ExecutorHooks, SectionResult};

/// What the engines did, over every program checked here.
static TALLY: Mutex<Tally> = Mutex::new(Tally::new());

/// What the engines did over some programs: what a run reports, and what
/// its floors are checked on. A process checking programs adds to its own,
/// [`Tally::taken`]; one that has others check them adds up theirs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tally {
    /// Programs each engine ran, in the order of `Differential::ALL`.
    ran: [u32; Differential::ALL.len()],
    /// Functions the chaos dispatcher JIT-compiled.
    chaos_compiled: u32,
    /// Entries at loop headers it compiled, and loop headers it could not
    /// enter.
    chaos_loops: u32,
    chaos_loops_refused: u32,
    /// The same for the `osr` engine.
    osr_loops: u32,
    osr_loops_refused: u32,
    /// The same for the `jit` engine, whose loops are compiled with their
    /// functions and so should never be entered on their own.
    jit_loops: u32,
    /// The bytecode's ops some body was lowered to, by kind.
    bytecode_ops: BTreeSet<String>,
    /// The IR instructions, by kind, the bytecode left to the IR walker.
    escaped: BTreeSet<String>,
}

impl Default for Tally {
    fn default() -> Tally {
        Tally::new()
    }
}

// `engine_tests` checks its programs in one process and uses less of this
// than `worldgen_engine_tests`, which shares the module.
#[allow(dead_code)]
impl Tally {
    const fn new() -> Tally {
        Tally {
            ran: [0; Differential::ALL.len()],
            chaos_compiled: 0,
            chaos_loops: 0,
            chaos_loops_refused: 0,
            osr_loops: 0,
            osr_loops_refused: 0,
            jit_loops: 0,
            bytecode_ops: BTreeSet::new(),
            escaped: BTreeSet::new(),
        }
    }

    /// What this process's checks have done since it was last taken.
    pub fn take() -> Tally {
        std::mem::take(&mut *TALLY.lock().X())
    }

    /// Add what `other` counted.
    pub fn add(&mut self, other: &Tally) {
        for (n, m) in self.ran.iter_mut().zip(&other.ran) {
            *n += m;
        }
        self.chaos_compiled += other.chaos_compiled;
        self.chaos_loops += other.chaos_loops;
        self.chaos_loops_refused += other.chaos_loops_refused;
        self.osr_loops += other.osr_loops;
        self.osr_loops_refused += other.osr_loops_refused;
        self.jit_loops += other.jit_loops;
        self.bytecode_ops.extend(other.bytecode_ops.iter().cloned());
        self.escaped.extend(other.escaped.iter().cloned());
    }

    /// Programs `engine` ran, by its name.
    pub fn ran(&self, engine: &str) -> u32 {
        let at = Differential::ALL.iter().position(|e| e.name() == engine).X();
        self.ran[at]
    }
}

/// The engines compared with the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Differential {
    Bytecode,
    Plain,
    Jit,
    Chaos,
    Osr,
    Aot,
    C,
    NoConst,
    NoSpec,
    Spec,
    SpecChaos,
}

impl Differential {
    const ALL: [Differential; 11] = [
        Differential::Bytecode,
        Differential::Plain,
        Differential::Jit,
        Differential::Chaos,
        Differential::Osr,
        Differential::Aot,
        Differential::C,
        Differential::NoConst,
        Differential::NoSpec,
        Differential::Spec,
        Differential::SpecChaos,
    ];

    fn name(self) -> &'static str {
        match self {
            Differential::Bytecode => "bytecode",
            Differential::Plain => "plain",
            Differential::Jit => "jit",
            Differential::Chaos => "chaos",
            Differential::Osr => "osr",
            Differential::Aot => "aot",
            Differential::C => "c",
            Differential::NoConst => "noconst",
            Differential::NoSpec => "nospec",
            Differential::Spec => "spec",
            Differential::SpecChaos => "specchaos",
        }
    }
}

/// What a fixture's preamble says: the engines it opts out of, and what has
/// to have run.
#[derive(Default)]
struct Preamble {
    opt_outs: Vec<Differential>,
    bytecode_ops: Vec<String>,
    bytecode_lowered: Vec<String>,
    osr_entered: bool,
}

fn preamble(source: &str) -> Result<Preamble, String> {
    let mut out = Preamble::default();
    for line in source.lines().take_while(|l| !l.trim_start().starts_with("----------")) {
        let line = line.trim();
        let words = |list: &str| list.split_whitespace().map(str::to_string).collect::<Vec<_>>();
        if let Some(list) = line.strip_prefix("// engines:") {
            for word in list.split_whitespace() {
                let name = word.strip_prefix('-')
                    .ok_or_else(|| format!("`{word}` in `engines:` is not an opt-out like `-aot`"))?;
                let engine = Differential::ALL.into_iter().find(|e| e.name() == name)
                    .ok_or_else(|| format!("no engine `{name}`"))?;
                out.opt_outs.push(engine);
            }
        } else if let Some(list) = line.strip_prefix("// expect-bytecode-ops:") {
            out.bytecode_ops.extend(words(list));
        } else if let Some(list) = line.strip_prefix("// expect-bytecode-lowered:") {
            out.bytecode_lowered.extend(words(list));
        } else if let Some(what) = line.strip_prefix("// expect-osr:") {
            if what.trim() != "entered" {
                return Err(format!("`expect-osr:` takes `entered`, not `{}`", what.trim()));
            }
            out.osr_entered = true;
        }
    }
    Ok(out)
}

/// What the last analysis on this thread ran, for a fixture's expectations.
#[derive(Default)]
struct LastRun {
    bytecode_ops: std::collections::BTreeSet<String>,
    escaped: std::collections::BTreeSet<String>,
    loops_entered: u32,
    loops_refused: u32,
}

thread_local! {
    static LAST_RUN: std::cell::RefCell<LastRun> = std::cell::RefCell::new(LastRun::default());
}

impl Preamble {
    /// Whether the bytecode run did what the fixture says it has to.
    fn check_bytecode(&self) -> Result<(), String> {
        LAST_RUN.with(|run| {
            let run = run.borrow();
            for op in &self.bytecode_ops {
                if !run.bytecode_ops.contains(op) {
                    return Err(format!("the bytecode lowered nothing to `{op}`; it used {:?}", run.bytecode_ops));
                }
            }
            for kind in &self.bytecode_lowered {
                if run.escaped.contains(kind) {
                    return Err(format!("the bytecode left a `{kind}` to the IR walker"));
                }
            }
            Ok(())
        })
    }

    /// Whether the osr run did what the fixture says it has to.
    fn check_osr(&self) -> Result<(), String> {
        if !self.osr_entered {
            return Ok(());
        }
        LAST_RUN.with(|run| {
            let run = run.borrow();
            if run.loops_entered == 0 {
                return Err("the osr engine entered no loop".S());
            }
            if run.loops_refused != 0 {
                return Err(format!("the osr engine was refused at {} loops", run.loops_refused));
            }
            Ok(())
        })
    }
}

/// Sets the interpreter's engine and, optionally, a dispatcher.
struct Setup {
    engine: Engine,
    dispatcher: Option<Box<dyn datalove_datafun_interp::CallDispatcher>>,
    /// Which differential engine this is, if it is one.
    differential: Option<Differential>,
}

impl ExecutorHooks for Setup {
    fn configure(&mut self, executor: &mut ScriptExecutor) {
        executor.set_engine(self.engine);
        if let Some(dispatcher) = self.dispatcher.take() {
            executor.set_dispatcher(dispatcher);
        }
    }

    fn finish(&mut self, executor: &mut ScriptExecutor) {
        LAST_RUN.with(|run| {
            let stats = executor.bytecode_stats();
            *run.borrow_mut() = LastRun {
                bytecode_ops: stats.op_kinds.C(),
                escaped: stats.escaped_kinds.C(),
                ..LastRun::default()
            };
            let mut tally = TALLY.lock().X();
            tally.bytecode_ops.extend(stats.op_kinds.iter().cloned());
            tally.escaped.extend(stats.escaped_kinds.iter().cloned());
        });
        let Some(dispatcher) = executor.take_dispatcher() else {
            return;
        };
        if let Some(chaos) = dispatcher.as_any().downcast_ref::<OptimizingDispatcher>() {
            let stats = chaos.jit().stats();
            // What the backend would not compile, and why, for whoever is
            // teaching it more.
            if std::env::var_os("ENGINE_TESTS_JIT_REFUSALS").is_some() {
                for f in stats.functions.values_unordered() {
                    if let Some(why) = &f.refusal {
                        eprintln!("jit refused {}: {}", f.name, why);
                    }
                }
            }
            let mut tally = TALLY.lock().X();
            tally.chaos_compiled += stats.compiled_count;
            tally.chaos_loops += stats.osr_compiled_count;
            tally.chaos_loops_refused += stats.osr_refused_count;
        } else if let Some(jit) = dispatcher.as_any().downcast_ref::<JitEngine>() {
            let stats = jit.stats();
            LAST_RUN.with(|run| {
                let mut run = run.borrow_mut();
                run.loops_entered = stats.osr_compiled_count;
                run.loops_refused = stats.osr_refused_count;
            });
            match self.differential {
                Some(Differential::Osr) => {
                    let mut tally = TALLY.lock().X();
                    tally.osr_loops += stats.osr_compiled_count;
                    tally.osr_loops_refused += stats.osr_refused_count;
                }
                Some(Differential::Jit) => {
                    TALLY.lock().X().jit_loops += stats.osr_compiled_count;
                }
                _ => {}
            }
        }
    }
}

thread_local! {
    /// The system library this binary carries, loaded once a thread.
    static SYSTEM: Arc<SystemLibrary> = Arc::new(datalove_sys_packages::system_library());
}

/// The system library, for a program that requires any of its modules:
/// others are compiled from their own sections alone, as they always were.
fn system_for(source: &str) -> Option<Arc<SystemLibrary>> {
    source.contains("require module sys/").then(|| SYSTEM.with(|system| system.C()))
}

/// Run the interpreted analysis under `setup` and render it as the expected
/// files record it.
fn analyze(parsed: &ParsedWorldfile, options: AnalysisOptions, setup: Setup) -> Result<(Analysis, String), String> {
    let db = datafun::Database::default();
    let mut setup = setup;
    let analysis = datafun::worldfile_analysis::analyze_worldfile_with_hooks(
        &db,
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
pub fn compiled_cleanly(reference: &Analysis) -> bool {
    reference.sections.iter().all(section_compiled)
}

/// Whether a section compiled without error.
pub fn section_compiled(section: &SectionResult) -> bool {
    matches!(section.typecheck, TypecheckResult::Success)
        && matches!(section.ownership, OwnershipResult::Success)
        && matches!(section.lowering, LoweringResult::Success { .. })
}

/// Whether the AOT backends apply: one fragment, no expressions, nothing but
/// modules and data beside it, and every unit compiled and run to completion by the
/// reference.
fn aot_applies(parsed: &ParsedWorldfile, reference: &Analysis) -> bool {
    let fragments = parsed.sections.iter()
        .filter(|s| matches!(s, WorldfileSection::ScriptFragment { .. }))
        .count();
    let only_modules = parsed.sections.iter().all(|s| matches!(s,
        WorldfileSection::Module { .. } | WorldfileSection::Data { .. } | WorldfileSection::ScriptFragment { .. }));
    let all_ran = reference.sections.iter()
        .all(|s| s.section_type == "module" || s.output == "(fragment executed)");
    fragments == 1 && only_modules && compiled_cleanly(reference) && all_ran
}

/// Compile the fragment for an AOT backend and hand it to `build_and_run`,
/// which returns the program's stderr.
///
/// With a system library, what the program links is a component built with
/// the library's riders in it, rather than the runtime alone.
fn run_aot(
    parsed: &ParsedWorldfile,
    system: Option<&SystemLibrary>,
    build_and_run: fn(&IrCodeUnit, &FunctionRegistry, &[PathBuf], &Path) -> Result<String, String>,
) -> Result<String, String> {
    let db = datafun::Database::default();
    let (mut module_compiler, workspace) = datafun::worldfile_analysis::worldfile_module_compiler(
        &db, &parsed.sections, CompilerOptions::default(), system);
    let compiled = module_compiler.compile(&db);
    let mut compiler = compiled.script_compiler_default(&db)
        .ok_or("the modules did not compile")?;
    let source = parsed.sections.iter()
        .find_map(|s| match s {
            WorldfileSection::ScriptFragment { source } => Some(source.as_str()),
            _ => None,
        })
        .X();
    let unit = compiler.compile_fragment(source).ir_unit.ok_or("the fragment did not compile")?;
    let libs = match workspace {
        Some((_, descriptor)) => vec![
            rider_build::build_component_staticlib(&rider_build::default_work_dir(), &descriptor.rider_crates())
                .map_err(|e| format!("the riders did not build: {e}"))?,
        ],
        None => Vec::new(),
    };
    let dir = rmx::tempfile::tempdir().map_err(|e| e.S())?;
    build_and_run(&unit, &compiled.module_registry(), &libs, dir.path())
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

/// Each builds `unit` into an executable linking `libs`, the native
/// component, or the runtime alone where there are none, and runs it.
fn cranelift_build_and_run(unit: &IrCodeUnit, registry: &FunctionRegistry, libs: &[PathBuf], dir: &Path) -> Result<String, String> {
    let mut compiler = AotCompiler::new_for_host().map_err(|e| e.S())?;
    let product = compiler
        .compile_script_unit_in_world(unit, registry)
        .map_err(|e| format!("compile failed: {e}"))?;
    let object = product.emit().map_err(|e| format!("emit failed: {e}"))?;
    let exe = dir.join("test");
    datafun::driver::aot::link_object_to_path_with_libs(&object, &exe, libs).map_err(|e| format!("link failed: {e}"))?;
    run_exe(&exe)
}

fn c_build_and_run(unit: &IrCodeUnit, registry: &FunctionRegistry, libs: &[PathBuf], dir: &Path) -> Result<String, String> {
    let output = CAotCompiler::new().compile_world(unit, registry).map_err(|e| format!("compile failed: {e}"))?;
    let mut cmd = Command::new("cc");
    cmd.args(["-std=c11", "-O0", "-g"]);
    if datafun::driver::aot::use_lld() {
        cmd.arg("-fuse-ld=lld");
    }
    for (filename, content) in &output.files {
        let path = dir.join(filename);
        std::fs::write(&path, content).map_err(|e| e.S())?;
        cmd.arg(path);
    }
    match libs {
        [] => {
            cmd.arg(datafun::driver::aot::runtime_only_component().map_err(|e| e.S())?);
        }
        libs => {
            cmd.args(libs);
        }
    }
    let exe = dir.join("test");
    cmd.args(["-ldl", "-lpthread", "-lm", "-o"]).arg(&exe);
    let cc = cmd.output().map_err(|e| format!("failed to run cc: {e}"))?;
    if !cc.status.success() {
        return Err(format!("cc failed:\n{}", String::from_utf8_lossy(&cc.stderr)));
    }
    run_exe(&exe)
}

/// Run a program on the reference and on every other engine that applies,
/// returning what the reference made of it and its rendering for an expected
/// file, or where an engine first differs from it.
///
/// The preamble is read from the program's text, and the seeds of the
/// engines that draw are derived from the text too.
pub fn check(source: &str) -> Result<(Analysis, String), String> {
    let preamble = preamble(source)?;
    let skip = &preamble.opt_outs;
    let parsed = package_load_worldfile::parse_worldfile_sections(source.as_bytes())
        .map_err(|e| format!("failed to parse worldfile: {e}"))?;

    let base = AnalysisOptions { system: system_for(source), ..AnalysisOptions::default() };
    let (reference, rendered) = analyze(&parsed, base.C(), Setup { engine: Engine::IrWalker, dispatcher: None, differential: None })?;

    let seed = seed_of(source);

    for (i, engine) in Differential::ALL.into_iter().enumerate() {
        if skip.contains(&engine) || !datalove_exampletest::engine_enabled(engine.name()) {
            continue;
        }
        if cfg!(miri) && !matches!(engine, Differential::Bytecode | Differential::Plain | Differential::NoConst | Differential::NoSpec | Differential::Spec | Differential::SpecChaos) {
            continue;
        }
        let variant = match engine {
            Differential::NoConst => Some(AnalysisOptions { skip_const_inlining: true, ..base.C() }),
            Differential::NoSpec => Some(AnalysisOptions { skip_specialization: true, ..base.C() }),
            Differential::Spec => Some(AnalysisOptions { generics: GenericsPolicy::All, ..base.C() }),
            Differential::SpecChaos => Some(AnalysisOptions { generics: GenericsPolicy::Chaos { seed: derive_seed(seed, "generics") }, ..base.C() }),
            _ => None,
        };
        if let Some(options) = variant {
            if !compiled_cleanly(&reference) {
                continue;
            }
            TALLY.lock().X().ran[i] += 1;
            // Specialization is offered only to the IR walker so far.
            let walker = if matches!(engine, Differential::Spec | Differential::SpecChaos) {
                Engine::IrWalker
            } else {
                Engine::Bytecode
            };
            let (other, _) = analyze(&parsed, options, Setup { engine: walker, dispatcher: None, differential: Some(engine) })?;
            let (want, got) = (observed(&reference), observed(&other));
            if want != got {
                return Err(format!("{} differs from the reference {}", engine.name(), first_difference(&want, &got)));
            }
            continue;
        }
        let interpreted = match engine {
            Differential::Bytecode => Some(Setup { engine: Engine::Bytecode, dispatcher: None, differential: Some(engine) }),
            Differential::Plain => Some(Setup { engine: Engine::PlainBytecode, dispatcher: None, differential: Some(engine) }),
            Differential::Jit => Some(Setup {
                differential: Some(engine),
                engine: Engine::IrWalker,
                dispatcher: Some(Box::new(JitEngine::new(1).map_err(|e| e.S())?)),
            }),
            Differential::Chaos => Some(Setup {
                differential: Some(engine),
                engine: Engine::Bytecode,
                dispatcher: Some(Box::new(
                    OptimizingDispatcher::with_config(DispatcherConfig::chaos(derive_seed(seed, "dispatcher"))).map_err(|e| e.S())?,
                )),
            }),
            Differential::Osr => Some(Setup {
                differential: Some(engine),
                engine: Engine::Bytecode,
                dispatcher: Some({
                    let mut jit = JitEngine::new(u32::MAX).map_err(|e| e.S())?;
                    jit.set_osr_threshold(SeededRng::derive(seed, "osr").random_range(1..=3));
                    Box::new(jit)
                }),
            }),
            Differential::Aot | Differential::C | Differential::NoConst | Differential::NoSpec | Differential::Spec | Differential::SpecChaos => None,
        };
        if let Some(setup) = interpreted {
            TALLY.lock().X().ran[i] += 1;
            let (_, other) = analyze(&parsed, base.C(), setup)?;
            if other != rendered {
                return Err(format!("{} differs from the reference {}", engine.name(), first_difference(&rendered, &other)));
            }
            match engine {
                Differential::Bytecode => preamble.check_bytecode()?,
                Differential::Osr => preamble.check_osr()?,
                _ => {}
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
        TALLY.lock().X().ran[i] += 1;
        let stderr = run_aot(&parsed, base.system.as_deref(), build_and_run)
            .map_err(|e| format!("{}: {e}", engine.name()))?;
        let fragment = reference.sections.iter().find(|s| s.section_type == "scriptunit-fragment").X();
        let expected = fragment.debug_output.as_deref().unwrap_or("");
        if stderr != expected {
            return Err(format!("{} differs from the reference {}", engine.name(), first_difference(expected, &stderr)));
        }
    }

    Ok((reference, rendered))
}

/// Report what each engine did over the programs `tally` counted; and, with
/// `floors`, the chaos dispatcher and the osr engine have to have done
/// something somewhere, or they tested nothing. A JIT that compiles every
/// function first must never have entered a loop by OSR, floors or not.
pub fn report(tally: &Tally, floors: bool) -> Result<(), String> {
    let ran: Vec<_> = Differential::ALL.iter().zip(&tally.ran)
        .map(|(e, n)| format!("{} {}", e.name(), n))
        .collect();
    println!("programs run per engine: {}", ran.join(", "));
    if datalove_exampletest::engine_enabled("bytecode") {
        let escaped: Vec<&str> = tally.escaped.iter().map(|k| k.as_str()).collect();
        println!(
            "bytecode: {} kinds of op lowered to; left to the IR walker: {}",
            tally.bytecode_ops.len(),
            if escaped.is_empty() { "nothing".S() } else { escaped.join(" ") },
        );
    }
    if datalove_exampletest::engine_enabled("chaos") {
        let (compiled, loops, refused) = (tally.chaos_compiled, tally.chaos_loops, tally.chaos_loops_refused);
        println!("chaos: {compiled} functions JIT-compiled, {loops} loops entered ({refused} refused)");
        if floors && compiled == 0 {
            return Err("the chaos dispatcher never engaged: 0 compiled".S());
        }
        if floors && loops == 0 {
            return Err("the chaos dispatcher never entered a loop".S());
        }
    }
    if datalove_exampletest::engine_enabled("osr") {
        let (osr, osr_refused) = (tally.osr_loops, tally.osr_loops_refused);
        println!("osr: {osr} loops entered ({osr_refused} refused)");
        if floors && osr == 0 {
            return Err("the osr engine never entered a loop".S());
        }
    }
    if tally.jit_loops != 0 {
        return Err(format!("the jit engine entered {} loops by OSR, though it compiles every function first", tally.jit_loops));
    }
    Ok(())
}
