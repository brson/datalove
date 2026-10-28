use rmx::prelude::*;

use rmx::clap::{self, Parser as _};
use rmx::std::path::PathBuf;

mod compile;
mod render;

fn main() -> AnyResult<()> {
    rmx::extras::init_crate_name(env!("CARGO_CRATE_NAME"));

    let cli = Cli::parse();
    cli.run()?;

    Ok(())
}

#[derive(clap::Parser)]
#[command(version = build_version())]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Run the datalit type checker and report errors.
    LitTycheck(LitTycheckCommand),
    /// Print the AST of a datalit expression.
    LitAst(LitAstCommand),
    /// Pretty print a datalit expression.
    LitPretty(LitPrettyCommand),
    /// Run built-in operations on datalit expressions.
    LitOp(LitOpCommand),
    /// Start an interactive REPL.
    Repl(ReplCommand),
    /// Execute a datafun script.
    Script(ScriptCommand),
    /// Dump the IR for a datafun script.
    ScriptIr(ScriptIrCommand),
    /// AOT compile a datafun script to native code.
    AotCompile(AotCompileCommand),
    /// Typecheck the sys/std library and report errors.
    TypecheckStd(TypecheckStdCommand),
    /// Generate HTML documentation from mandocs/.
    Docs(DocsCommand),
    /// Execute a worldfile's modules and its script units, in one REPL session.
    ScriptWorld(ScriptWorldCommand),
    /// Generate a random worldfile.
    Worldgen(WorldgenCommand),
}

#[derive(clap::Args)]
struct LitTycheckCommand {
    /// Path to the .dlt file to type check.
    file_path: PathBuf,
}

#[derive(clap::Args)]
struct LitAstCommand {
    /// Path to the .dlt file to print AST for.
    file_path: PathBuf,
}

#[derive(clap::Args)]
struct LitPrettyCommand {
    /// Path to the .dlt file to pretty print.
    file_path: PathBuf,
}

#[derive(clap::Args)]
struct LitOpCommand {
    /// First expression (as string or file path).
    expr1: String,
    /// Operation name (e.g., "eq" for equality).
    op: String,
    /// Second expression (as string or file path).
    expr2: String,
}

#[derive(clap::Args)]
struct ReplCommand {
    /// Path to a script file (.dfs) to execute in non-interactive mode.
    #[arg(long)]
    script: Option<PathBuf>,
    /// Replay a session `/save` wrote before the first prompt.
    #[arg(long, value_name = "WORLDFILE", conflicts_with = "script")]
    load: Option<PathBuf>,
}

#[derive(clap::Args)]
struct ScriptCommand {
    /// Path to the script file (.dfs) to execute.
    file_path: PathBuf,
    /// Run without loading the sys library.
    #[arg(long)]
    no_sys: bool,
    /// Enable the JIT compiler for function execution.
    #[arg(long)]
    jit: bool,
    /// Compile a function on this call to it [default: 100].
    #[arg(long, value_name = "CALLS", requires = "jit")]
    jit_threshold: Option<u32>,
    /// Go on in compiled code from a loop once it has run this many times,
    /// in a function not yet compiled [default: 1000].
    #[arg(long, value_name = "ITERATIONS", requires = "jit")]
    jit_osr_threshold: Option<u32>,
    /// How hard the JIT's code generator works: none, speed or speed_and_size
    /// [default: speed].
    #[arg(long, value_name = "LEVEL", requires = "jit")]
    jit_opt_level: Option<datalove::datafun_jit::JitOptLevel>,
    /// Print to stderr what the JIT compiled and how calls crossed between it
    /// and the interpreter. Implies `--time`.
    #[arg(long, requires = "jit")]
    jit_stats: bool,
    /// Print to stderr how long compiling and running took.
    #[arg(long)]
    time: bool,
}

#[derive(clap::Args)]
struct ScriptIrCommand {
    /// Path to the script file (.dfs) to dump IR for.
    file_path: PathBuf,
    /// Run without loading the sys library.
    #[arg(long)]
    no_sys: bool,
}

#[derive(clap::Args)]
struct AotCompileCommand {
    /// Path to the script file (.dfs) to compile.
    file_path: PathBuf,
    /// Output path. Defaults to input name with .o extension (or no extension if --link/--run).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Link into an executable (requires cc and libdatalove_rt.a).
    #[arg(long)]
    link: bool,
    /// Link and run the executable (implies --link).
    #[arg(long)]
    run: bool,
    /// Run without loading the sys library.
    #[arg(long)]
    no_sys: bool,
    /// Emit C and compile it, rather than emitting an object directly.
    ///
    /// The two backends produce the same program by different routes. This one
    /// needs a C compiler on the path and always writes an executable, since
    /// there is no single object file to hand back.
    #[arg(long)]
    c: bool,
}

#[derive(clap::Args)]
struct TypecheckStdCommand {
}

#[derive(clap::Args)]
struct DocsCommand {
}

#[derive(clap::Args)]
struct ScriptWorldCommand {
    /// Path to the worldfile (.world) to execute.
    file_path: PathBuf,
    /// Run without loading the sys library.
    #[arg(long)]
    no_sys: bool,
}

#[derive(clap::Args)]
struct WorldgenCommand {
    /// Seed for random generation. If not provided, a random seed is used.
    seed: Option<u64>,
}

impl Cli {
    fn run(&self) -> AnyResult<()> {
        match &self.cmd {
            Command::LitTycheck(cmd) => cmd.run(),
            Command::LitAst(cmd) => cmd.run(),
            Command::LitPretty(cmd) => cmd.run(),
            Command::LitOp(cmd) => cmd.run(),
            Command::Repl(cmd) => cmd.run(),
            Command::Script(cmd) => cmd.run(),
            Command::ScriptIr(cmd) => cmd.run(),
            Command::AotCompile(cmd) => cmd.run(),
            Command::TypecheckStd(cmd) => cmd.run(),
            Command::Docs(cmd) => cmd.run(),
            Command::ScriptWorld(cmd) => cmd.run(),
            Command::Worldgen(cmd) => cmd.run(),
        }
    }
}

impl LitTycheckCommand {
    fn run(&self) -> AnyResult<()> {
        use datalove::datalit;
        use datalove::bct::input::Source;

        let db = datalit::Database::default();

        let contents = rmx::std::fs::read_to_string(&self.file_path)?;
        let source = Source::new(&db, contents.S());
        let cwd = rmx::std::env::current_dir().unwrap_or_default();

        let parse_result = datalit::parser::parse(&db, source);
        let ast = parse_result.expr(&db);
        let parse_diags = datalit::parser::parse::accumulated::<datalove::diagnostic::ParseDiagnostic>(&db, source);
        if !parse_diags.is_empty() {
            render::render_parse_diagnostics(&db, &parse_diags, &self.file_path, &datalove::bct::render::LineOffsets::new(), &cwd);
            bail!("Parse error");
        }

        let resolved = datalit::resolve::resolve_names(&db, source, ast.clone());
        let result = datalit::tycheck::type_check(&db, ast.clone(), resolved);
        let type_diags = datalit::tycheck::type_check::accumulated::<datalove::diagnostic::TypeDiagnostic>(&db, ast, resolved);
        render::render_type_diagnostics(&db, &type_diags, &self.file_path, &cwd);
        if !result.errors(&db).is_empty() {
            bail!("Type error");
        }

        println!("No type errors found.");
        Ok(())
    }
}

impl LitAstCommand {
    fn run(&self) -> AnyResult<()> {
        use datalove::datalit;
        use datalove::bct::input::Source;

        let db = datalit::Database::default();

        // Read the file.
        let contents = rmx::std::fs::read_to_string(&self.file_path)?;
        let source = Source::new(&db, contents.S());

        // Parse the expression.
        let parse_result = datalit::parser::parse(&db, source);
        let ast = parse_result.expr(&db);

        // Convert to serializable AST and print.
        let serde_ast = datalit::ast_serde::ExprFull::from_ast(&db, ast);
        println!("{:#?}", serde_ast);

        Ok(())
    }
}

impl LitPrettyCommand {
    fn run(&self) -> AnyResult<()> {
        use datalove::datalit;
        use datalove::bct::input::Source;

        let db = datalit::Database::default();

        // Read the file.
        let contents = rmx::std::fs::read_to_string(&self.file_path)?;
        let source = Source::new(&db, contents.S());

        // Parse the expression.
        let parse_result = datalit::parser::parse(&db, source);
        let ast = parse_result.expr(&db);

        // Pretty print using the pretty printer.
        let pretty_printed = datalit::pretty::pretty_print(&db, &ast);
        println!("{}", pretty_printed);

        Ok(())
    }
}

impl LitOpCommand {
    fn run(&self) -> AnyResult<()> {
        use datalove::datalit;
        use datalove::bct::input::Source;

        let db = datalit::Database::default();

        // Parse both expressions.
        let source1 = Source::new(&db, self.expr1.S());
        let parse_result1 = datalit::parser::parse(&db, source1);
        let expr1 = parse_result1.expr(&db);

        let source2 = Source::new(&db, self.expr2.S());
        let parse_result2 = datalit::parser::parse(&db, source2);
        let expr2 = parse_result2.expr(&db);

        // Resolve and type check.
        let resolved1 = datalit::resolve::resolve_names(&db, source1, expr1.clone());
        let typechecked1 = datalit::tycheck::type_check(&db, expr1, resolved1);

        let resolved2 = datalit::resolve::resolve_names(&db, source2, expr2.clone());
        let typechecked2 = datalit::tycheck::type_check(&db, expr2, resolved2);

        // Check for type errors.
        if !typechecked1.errors(&db).is_empty() {
            let errors: Vec<_> = typechecked1.errors(&db)
                .iter()
                .map(|e| e.error(&db).to_string())
                .collect();
            bail!("Type errors in first expression: {}", errors.join("; "));
        }
        if !typechecked2.errors(&db).is_empty() {
            let errors: Vec<_> = typechecked2.errors(&db)
                .iter()
                .map(|e| e.error(&db).to_string())
                .collect();
            bail!("Type errors in second expression: {}", errors.join("; "));
        }

        // Instantiate values with RAII guards for cleanup.
        let rt = datalove::rt::rust::Runtime::new();
        let mut tydesc_table1 = datalit::tydesc_table::TyDescTable::new(&db);
        let inst1 = datalit::instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table1, typechecked1)?;
        let _guard1 = unsafe {
            datalove::rt::rust::ValueGuard::from_raw(rt.handle(), inst1.tydesc.as_ptr(), inst1.ptr as *mut u8)
        };
        let mut tydesc_table2 = datalit::tydesc_table::TyDescTable::new(&db);
        let inst2 = datalit::instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table2, typechecked2)?;
        let _guard2 = unsafe {
            datalove::rt::rust::ValueGuard::from_raw(rt.handle(), inst2.tydesc.as_ptr(), inst2.ptr as *mut u8)
        };

        // Execute the operation.
        match self.op.as_str() {
            "eq" => {
                // Call dtlv_rti_eq.
                let result = unsafe {
                    datalove::rt::c::dtlv_rti_eq_local(
                        std::ptr::null_mut(), // runtime handle not needed
                        datalove::rt::c::AnyRef::new(inst1.ptr),
                        inst1.tydesc.as_ref(),
                        datalove::rt::c::AnyRef::new(inst2.ptr),
                        inst2.tydesc.as_ref(),
                    )
                };

                match result {
                    datalove::rt::c::RtEq::Equals => println!("true"),
                    datalove::rt::c::RtEq::NotEquals => println!("false"),
                    datalove::rt::c::RtEq::Error => bail!("Type mismatch in equality comparison"),
                }
            }
            "cmp" => {
                // Call dtlv_rti_cmp_total.
                let result = unsafe {
                    datalove::rt::c::dtlv_rti_cmp_total_local(
                        std::ptr::null_mut(), // runtime handle not needed
                        datalove::rt::c::AnyRef::new(inst1.ptr),
                        inst1.tydesc.as_ref(),
                        datalove::rt::c::AnyRef::new(inst2.ptr),
                        inst2.tydesc.as_ref(),
                    )
                };

                match result {
                    datalove::rt::c::RtOrdering::Less => println!("less"),
                    datalove::rt::c::RtOrdering::Equal => println!("equal"),
                    datalove::rt::c::RtOrdering::Greater => println!("greater"),
                    datalove::rt::c::RtOrdering::Error => bail!("Type mismatch in comparison"),
                }
            }
            _ => {
                bail!("Unknown operation: {}", self.op);
            }
        }

        // Guards clean up automatically on drop.
        Ok(())
    }
}

impl ReplCommand {
    fn run(&self) -> AnyResult<()> {
        use datalove::datafun::pipeline::local_library_for;

        // A command given no script looks for `local` here, and a REPL's
        // workspace is this directory.
        let cwd = rmx::std::env::current_dir().context("failed to read the current directory")?;
        if let Some(script_path) = &self.script {
            let local = local_library_for(Some(script_path), &cwd)?;
            datalove::repl::run_script(datalove::sys_packages::system_library(), local, script_path)
        } else {
            let local = local_library_for(None, &cwd)?;
            datalove_repl_rat::run(
                datalove::sys_packages::system_library,
                local,
                &cwd,
                self.load.as_deref(),
            )
        }
    }
}

impl ScriptCommand {
    fn run(&self) -> AnyResult<()> {
        use datalove::datafun;
        use datalove::datafun_jit::{
            DispatcherConfig, DispatcherMode, OptimizingDispatcher, DEFAULT_JIT_THRESHOLD,
        };

        let start = std::time::Instant::now();
        let db = datafun::Database::default();
        let script = compile::compile_script(&db, &self.file_path, self.no_sys)?;

        // The production defaults: compiling a function on its hundredth
        // call, and a loop's code, by OSR, on its thousandth iteration. On
        // the store demo, `benchvs` and `botdocs/learn.dfs` that is as fast
        // as compiling every function on its first call or faster -- much
        // faster on a short run -- and compiles about half as many; see
        // `botdocs/plan-jit-tuning.md`. `opt_level` stays `speed`: `none`
        // compiles faster and was faster on the demo, but 28% slower on
        // `benchvs/primes`.
        let call_dispatcher: Option<Box<dyn datalove::datafun_interp::CallDispatcher>> = if self.jit {
            let config = DispatcherConfig {
                jit_enabled: true,
                mode: DispatcherMode::Tuned {
                    jit_threshold: self.jit_threshold.unwrap_or(DEFAULT_JIT_THRESHOLD),
                },
                opt_level: self.jit_opt_level.unwrap_or_default(),
            };
            let mut dispatcher = OptimizingDispatcher::with_config(config)
                .map_err(|e| anyhow!("Failed to create JIT engine: {:?}", e))?;
            if self.jit_stats {
                dispatcher.jit_mut().count_calls();
            }
            if let Some(iterations) = self.jit_osr_threshold {
                dispatcher.jit_mut().set_osr_threshold(iterations);
            }
            Some(Box::new(dispatcher))
        } else {
            None
        };

        // Stderr mode for debuglog output.
        let mut executor = script.compiled.script_executor(datafun::DebugOutputMode::Stderr, call_dispatcher)
            .expect("script_executor should succeed after error check");
        datafun::pipeline::rider_load::register_natives(&script.natives, &script.compiled, &mut executor)?;

        let compiled_at = start.elapsed();
        let output = executor.execute_fragment(&script.ir_unit);
        if self.time || self.jit_stats {
            let ran = start.elapsed() - compiled_at;
            eprintln!("time: {:.1} ms compiling, {:.1} ms running",
                compiled_at.as_secs_f64() * 1e3, ran.as_secs_f64() * 1e3);
        }
        if self.jit_stats {
            let dispatcher = executor.take_dispatcher().expect("--jit-stats runs with the jit");
            let stats = dispatcher.as_any().downcast_ref::<OptimizingDispatcher>()
                .expect("the script command's dispatcher is an OptimizingDispatcher")
                .jit().stats();
            let paths = script.compiled.module_paths(&db);
            eprint!("{}", stats.report(25, &|id| paths.get(&id).cloned()));
            executor.set_dispatcher(dispatcher);
        }
        if let Some(error) = output.strip_prefix("Error: ") {
            bail!("{error}");
        }

        // A script need not bind `output`, and prints nothing if it does not.
        if let Some((_, value)) = executor.get_binding("output") {
            match value {
                Some(value) => println!("{}", value),
                None => bail!("`output` was given away"),
            }
        }

        executor.destroy_live_values();

        // The interpreters check for leaks as they drop, so they go as usual.
        // The database is most of what the process allocated, and freeing it
        // table by table was about 5% of a run requiring all of `sys/std`; the
        // process exits next, which frees it at once.
        drop((executor, script));
        std::mem::forget(db);

        Ok(())
    }
}

impl ScriptIrCommand {
    fn run(&self) -> AnyResult<()> {
        let db = datalove::datafun::Database::default();
        let script = compile::compile_script(&db, &self.file_path, self.no_sys)?;
        println!("{}", script.ir_unit);
        Ok(())
    }
}

impl AotCompileCommand {
    fn run(&self) -> AnyResult<()> {
        use datalove::datafun::pipeline::{aot, rider_build};

        let db = datalove::datafun::Database::default();
        let script = compile::compile_script(&db, &self.file_path, self.no_sys)?;
        let registry = script.compiled.module_registry();
        let ir_unit = &script.ir_unit;
        let work_dir = script.descriptor.work_dir.as_ref()
            .expect("a script is compiled with a work dir");

        // The runtime and the riders this program calls, as one library for
        // the linker. Which riders those are comes from the module graph, so
        // a program using a package of its own gets that package's rider in
        // here alongside the ones from `sys/`.
        let component = rider_build::build_component_staticlib(
            work_dir,
            &script.descriptor.rider_crates(),
        ).map_err(|e| anyhow!("{}", e))?;
        let rider_libs = vec![component];

        // --run implies --link.
        let should_link = self.link || self.run;

        if self.c {
            return self.compile_via_c(ir_unit, &registry, &rider_libs, should_link);
        }

        // Compile to object bytes using pipeline::aot.
        let obj_bytes = aot::compile_script_to_object_with_world(
            ir_unit,
            &registry,
        )?;

        // Determine output path.
        let output_path = if let Some(ref out) = self.output {
            out.C()
        } else {
            let stem = self.file_path.file_stem()
                .ok_or_else(|| anyhow!("Invalid input filename"))?;
            if should_link {
                PathBuf::from(stem)
            } else {
                PathBuf::from(format!("{}.o", stem.to_string_lossy()))
            }
        };

        if should_link {
            // Link into executable using pipeline::aot, including rider libraries.
            aot::link_object_to_path_with_libs(&obj_bytes, &output_path, &rider_libs)
                .map_err(|e| anyhow!("{}", e))?;
            println!("Linked executable: {}", output_path.display());

            if self.run {
                // Run the executable. Use absolute path or prefix with ./ for relative paths.
                let exe_path = if output_path.is_absolute() {
                    output_path.C()
                } else {
                    std::env::current_dir()?.join(&output_path)
                };
                let result = aot::run_executable(&exe_path)
                    .map_err(|e| anyhow!("{}", e))?;
                if !result.stdout.is_empty() {
                    print!("{}", result.stdout);
                }
                if !result.stderr.is_empty() {
                    eprint!("{}", result.stderr);
                }
            }
        } else {
            // Write object file.
            rmx::std::fs::write(&output_path, &obj_bytes)
                .with_context(|| format!("Failed to write object file: {}", output_path.display()))?;
            println!("Wrote object file: {}", output_path.display());
        }

        Ok(())
    }

    /// Compile through C rather than straight to an object.
    ///
    /// The C backend produces one source file per module plus one for the
    /// script, and a C compiler turns the set of them into the executable in
    /// one step, so there is no unlinked halfway point to write out.
    fn compile_via_c(
        &self,
        ir_unit: &datalove::datafun_ir::IrCodeUnit,
        registry: &datalove::datafun_ir::FunctionRegistry,
        rider_libs: &[PathBuf],
        should_link: bool,
    ) -> AnyResult<()> {
        use datalove::datafun::pipeline::{aot, c_aot};

        let sources = c_aot::compile_world(ir_unit, registry)
            .map_err(|e| anyhow!("{}", e))?;

        if !should_link {
            for (name, body) in &sources.files {
                println!("// === {} ===", name);
                print!("{}", body);
            }
            return Ok(());
        }

        let output_path = match &self.output {
            Some(out) => out.C(),
            None => {
                let stem = self.file_path.file_stem()
                    .ok_or_else(|| anyhow!("Invalid input filename"))?;
                PathBuf::from(stem)
            }
        };

        c_aot::link_sources_to_path(&sources, &output_path, rider_libs)
            .map_err(|e| anyhow!("{}", e))?;
        println!("Linked executable: {}", output_path.display());

        if self.run {
            let exe_path = if output_path.is_absolute() {
                output_path.C()
            } else {
                std::env::current_dir()?.join(&output_path)
            };
            let result = aot::run_executable(&exe_path).map_err(|e| anyhow!("{}", e))?;
            if !result.stdout.is_empty() {
                print!("{}", result.stdout);
            }
            if !result.stderr.is_empty() {
                eprint!("{}", result.stderr);
            }
        }
        Ok(())
    }
}

impl TypecheckStdCommand {
    fn run(&self) -> AnyResult<()> {
        use datalove::datafun;

        let db = datafun::Database::default();

        let sys = datalove::sys_packages::system_library();
        let descriptor = datafun::pipeline::WorkspaceDescriptor::from_system_library(&sys);

        let (mut pipeline, _) = descriptor.to_pipeline_with_natives(&db, &sys.natives);
        let compiled = pipeline.compile(&db);

        if let Some(err) = &compiled.resolution_error {
            bail!("Package resolution failed: {}", err);
        }

        let errors: Vec<(&String, &String)> = compiled.path_to_errors.iter()
            .flat_map(|(path, errors)| errors.iter().map(move |err| (path, err)))
            .collect();

        if errors.is_empty() {
            println!("No typecheck errors found in sys/std.");
            Ok(())
        } else {
            println!("Found {} typecheck error(s):", errors.len());
            for (path, err) in &errors {
                println!("  {}: {}", path, err);
            }
            bail!("Typecheck failed with {} error(s)", errors.len());
        }
    }
}

impl DocsCommand {
    fn run(&self) -> AnyResult<()> {
        use megaspace_pipeliner::{DocSet, PostsConfig, Site};

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let manifest_path = PathBuf::from(manifest_dir);
        let project_root = manifest_path
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| anyhow!("Failed to find project root"))?;

        let static_assets = &["style.css", "datalove-logo.svg", "datalove-prism.js"];

        let mandocs_dir = project_root.join("mandocs");
        let botdocs_dir = project_root.join("botdocs");
        let output_dir = project_root.join("docs");

        let doc_sets = [
            DocSet {
                input_dir: &mandocs_dir,
                mount: "",
                static_assets,
                extra_context: &[("cross_link_label", "Botdocs")],
                link_context: &[("cross_link_url", "../botdocs/README.md")],
                posts: Some(PostsConfig {
                    site_title: "Datalove",
                    site_description: "Updates from Datalove",
                    author: "Brian Anderson",
                    language: "en",
                    base_url: "https://datalove-language.net",
                    categories: &["design", "devlog", "milestone"],
                    excerpt_bytes: 600,
                    posts_dir: None,
                    syndication: None,
                }),
            },
            DocSet {
                input_dir: &botdocs_dir,
                mount: "bot",
                static_assets,
                extra_context: &[("cross_link_label", "Mandocs")],
                link_context: &[("cross_link_url", "../mandocs/README.md")],
                posts: None,
            },
        ];

        Site { output_dir: &output_dir, doc_sets: &doc_sets, deployed_with: &[] }.build()?;

        println!("Documentation generated in {}", output_dir.display());

        Ok(())
    }
}

impl ScriptWorldCommand {
    /// Run a worldfile's script units in order, in one engine session.
    ///
    /// A unit that fails to compile, returns early, or reads from one that did
    /// stays in the session and the units after it still run, as they would at
    /// the REPL. A runtime error ends the session, and the run with it.
    fn run(&self) -> AnyResult<()> {
        use datalove::datafun;
        use datafun::pipeline::WorkspaceDescriptor;
        use datalove::datafun_pkg::package_load_worldfile::{parse_worldfile_sections, WorldfileSection};
        use datalove::script_engine::{Engine, Eval, Unit};

        // Read and parse the worldfile.
        let file_contents = rmx::std::fs::read_to_string(&self.file_path)
            .with_context(|| format!("Failed to read worldfile: {}", self.file_path.display()))?;
        let parsed = parse_worldfile_sections(file_contents.as_bytes())
            .with_context(|| format!("Failed to parse worldfile: {}", self.file_path.display()))?;

        if parsed.sections.iter().any(|s| matches!(s, WorldfileSection::ReplCommand { .. })) {
            bail!("script-world does not run repl-command sections; `repl --load` does");
        }
        // Each unit with how many lines of the file come before it.
        let units: Vec<(Unit, usize)> = parsed.sections.iter().zip(&parsed.line_offsets)
            .filter_map(|(section, offset)| match section {
                WorldfileSection::ScriptFragment { source } => Some((Unit::Fragment(source.C()), *offset)),
                WorldfileSection::ScriptExpr { source } => Some((Unit::Expression(source.C()), *offset)),
                _ => None,
            })
            .collect();
        if units.is_empty() {
            bail!("Worldfile has no script section (scriptunit-fragment or scriptunit-expr)");
        }

        // Build workspace descriptor from sys library + worldfile sections.
        let sys = datalove::sys_packages::system_library();
        let sys_descriptor = if self.no_sys {
            WorkspaceDescriptor::empty()
        } else {
            WorkspaceDescriptor::from_system_library(&sys)
        };
        let worldfile_descriptor = WorkspaceDescriptor::from_worldfile_sections(
            &parsed.sections,
            datafun::pipeline::CompilerOptions::default(),
        );
        let descriptor = sys_descriptor.merge(&worldfile_descriptor)
            .with_work_dir(datalove::paths::work_dir()?);

        // The engine compiles what the script units require as they come, the
        // same as the REPL does, and the worldfile's own modules from the
        // start. Those are roots whether the script reaches them or not: the
        // author wrote them in the file they asked to be compiled, so an error
        // in one is an error in the worldfile, and nothing runs. Only the
        // system library is pruned.
        let worldfile_modules: Vec<String> = parsed.sections.iter()
            .filter_map(|section| match section {
                WorldfileSection::Module { library, package, module, .. } =>
                    Some(format!("{}/{}/{}", library, package, module)),
                _ => None,
            })
            .collect();
        let mut engine = Engine::with_options(
            sys, descriptor, datafun::DebugOutputMode::Stderr, worldfile_modules)?;

        // Where each section's text sits in the worldfile, so that what is
        // said about a line of it names the file's line. The units' sources
        // are made as they are evaluated, and are added there.
        let pipeline = engine.pipeline();
        let line_offsets: datalove::bct::render::LineOffsets = parsed.sections.iter().zip(&parsed.line_offsets)
            .filter_map(|(section, offset)| {
                let source = match section {
                    WorldfileSection::Module { library, package, module, .. } =>
                        pipeline.module_source(&fmt!("{library}/{package}/{module}")),
                    WorldfileSection::Data { library, package, name, .. } =>
                        pipeline.data_source(&fmt!("{library}/{package}/{name}")),
                    WorldfileSection::Rider { name, .. } => pipeline.rider_source(name),
                    _ => None,
                };
                Some((source?, *offset))
            })
            .collect();

        let cwd = rmx::std::env::current_dir().unwrap_or_default();
        let module_errors = engine.module_errors();
        if !module_errors.is_empty() {
            render::render_stored_module_diagnostics(
                engine.db(), &module_errors.diagnostics, &self.file_path, &line_offsets, &cwd);
            // Lowering errors have no spans yet, so they are printed as they are.
            for error in &module_errors.lowering_errors {
                eprintln!("{}", error);
            }
            bail!("Module compilation error");
        }

        let files = engine.workspace().source_files(engine.pipeline());

        let total = units.len();
        let mut failed: Vec<(usize, AnyError)> = Vec::new();
        for (index, (unit, offset)) in units.into_iter().enumerate() {
            let mut compile_error = None;
            let eval = engine.eval_reporting(unit, &mut |compiler, compiled| {
                let mut line_offsets = line_offsets.C();
                line_offsets.insert(compiler.last_source().expect("a unit was compiled"), offset);
                compile_error = Some(compile::check_unit(
                    compiler, compiled, &files, &self.file_path, &line_offsets, &cwd)
                    .expect_err("a unit was reported as failing to compile"));
            });
            let error = match eval {
                Eval::Ran(_) => None,
                Eval::Value(expr) => {
                    println!("{}", expr.value);
                    None
                }
                Eval::ReturnedEarly(output) => {
                    println!("{}", output);
                    None
                }
                Eval::NotCompiled(_) => Some(compile_error.expect("a unit that failed to compile was reported")),
                Eval::Blocked { on } => {
                    let on: Vec<usize> = on.iter().map(|unit| unit + 1).collect();
                    Some(anyhow!("{}", datalove::script_engine::blocked_reason(&on)))
                }
                // The session is gone, so nothing is left for the rest to run against.
                Eval::RuntimeError(output) => bail!("{}", output.strip_prefix("Error: ").unwrap_or(&output)),
                Eval::Crashed(message) => bail!("{}", message),
            };
            if let Some(error) = error {
                if total > 1 {
                    eprintln!("unit {}: {}", index + 1, error);
                }
                failed.push((index, error));
            }
        }

        match (failed.len(), total) {
            (0, _) => Ok(()),
            (_, 1) => Err(failed.pop().expect("one unit failed").1),
            (n, total) => bail!("{} of {} script units failed", n, total),
        }
    }
}

impl WorldgenCommand {
    fn run(&self) -> AnyResult<()> {
        use rand::Rng;
        use datalove_worldgen::{WorldGenConfig, gen_worldfile_seeded};

        let seed = self.seed.unwrap_or_else(|| {
            let seed: u64 = rand::thread_rng().r#gen();
            eprintln!("seed: {}", seed);
            seed
        });

        let config = WorldGenConfig::default();
        let worldfile = gen_worldfile_seeded(seed, config);
        println!("{}", worldfile);

        Ok(())
    }
}

/// What `--version` prints.
///
/// The version alone would not say where this binary expects to find the
/// runtime and the riders it has to compile, and those are the two things that
/// go wrong. A build from a checkout names it, and says so when it has since
/// gone missing.
fn build_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| format!(
        "{} (built from {})",
        env!("CARGO_PKG_VERSION"),
        datalove_buildinfo::BUILD_INFO.describe(),
    )).as_str()
}
