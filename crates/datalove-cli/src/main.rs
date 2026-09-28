use rmx::prelude::*;

use rmx::clap::{self, Parser as _};
use rmx::std::path::PathBuf;

mod render;

use datalove_datafun::pipeline::rider_load::register_linked_natives;

/// Point an executor at the rider functions linked into this binary.
///
/// The interpreter calls them through its native table, and the JIT calls
/// them directly, so it needs the addresses as well; a script that reaches a
/// native call with a JIT that has not been told about it aborts the process.
fn register_natives(
    compiled: &datalove_datafun::pipeline::CompiledModules,
    sys: &datalove_datafun::pipeline::SystemLibrary,
    executor: &mut datalove_datafun::pipeline::ScriptExecutor,
) -> AnyResult<()> {
    use datalove_datafun_cranelift_jit::JitEngine;

    let native_fn_ptrs = register_linked_natives(
        &compiled.native_symbols(),
        &sys.natives,
        executor.native_table_mut(),
    )?;

    if let Some(dispatcher) = executor.take_dispatcher() {
        if let Some(jit) = dispatcher.as_any().downcast_ref::<JitEngine>() {
            for (symbol, ptr) in &native_fn_ptrs {
                jit.register_native_symbol(symbol, *ptr);
            }
        }
        executor.set_dispatcher(dispatcher);
    }

    Ok(())
}

/// The failure for a type error, saying what it was when nothing else did.
///
/// A diagnostic carries a span and renders itself. An error without one - a
/// duplicate import or type alias, say - renders nothing, and a bare "type
/// error" is all the user would otherwise get.
fn type_error(diagnostics: &[&datalove_diagnostic::TypeDiagnostic], errors: &[String]) -> AnyError {
    if diagnostics.is_empty() && !errors.is_empty() {
        anyhow!("Type error:\n{}", errors.join("\n"))
    } else {
        anyhow!("Type error")
    }
}

fn main() -> AnyResult<()> {
    rmx::extras::init_crate_name(env!("CARGO_CRATE_NAME"));

    let cli = Cli::parse();
    cli.run()?;

    Ok(())
}

#[derive(clap::Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
    #[command(flatten)]
    args: Args,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Run the type checker and report errors.
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
    /// Execute a worldfile containing modules and a script section.
    ScriptWorld(ScriptWorldCommand),
    /// Generate a random worldfile.
    Worldgen(WorldgenCommand),
}

#[derive(clap::Args)]
struct Args {
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
    /// Path to a script file (.dls) to execute in non-interactive mode.
    #[arg(long)]
    script: Option<PathBuf>,
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
            Command::LitTycheck(cmd) => cmd.run(&self.args),
            Command::LitAst(cmd) => cmd.run(&self.args),
            Command::LitPretty(cmd) => cmd.run(&self.args),
            Command::LitOp(cmd) => cmd.run(&self.args),
            Command::Repl(cmd) => cmd.run(&self.args),
            Command::Script(cmd) => cmd.run(&self.args),
            Command::ScriptIr(cmd) => cmd.run(&self.args),
            Command::AotCompile(cmd) => cmd.run(&self.args),
            Command::TypecheckStd(cmd) => cmd.run(&self.args),
            Command::Docs(cmd) => cmd.run(&self.args),
            Command::ScriptWorld(cmd) => cmd.run(&self.args),
            Command::Worldgen(cmd) => cmd.run(&self.args),
        }
    }
}

/// Report an ownership failure the way the interpreter does, and stop.
///
/// Every command that compiles a script has to do this. One that skips it gets
/// as far as asking for the IR, finds none, and says "IR unit not available
/// after lowering" -- which names the step that produced nothing rather than
/// the analysis that refused to produce it.
fn bail_on_ownership_error(
    compiled_unit: &datalove_datafun::pipeline::ScriptCompilationResult,
    compiler: &datalove_datafun::pipeline::ScriptCompiler<'_>,
    file_path: &std::path::Path,
    cwd: &std::path::Path,
) -> AnyResult<()> {
    use datalove_datafun as datafun;
    if let datafun::pipeline::OwnershipResult::Error { message: _ } = &compiled_unit.ownership {
        if let Some(spans) = compiler.get_last_spans() {
            render::render_ownership_errors_direct(
                compiler.db(),
                compiler.get_ownership_errors(),
                spans,
                file_path,
                cwd,
            );
        }
        bail!("Ownership error");
    }
    Ok(())
}

impl LitTycheckCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use datalove_datalit as datalit;
        use bct::input::Source;

        let db = datalit::Database::default();

        let contents = rmx::std::fs::read_to_string(&self.file_path)?;
        let source = Source::new(&db, contents.S());
        let cwd = rmx::std::env::current_dir().unwrap_or_default();

        let parse_result = datalit::parser::parse(&db, source);
        let ast = parse_result.expr(&db);
        let parse_diags = datalit::parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(&db, source);
        if !parse_diags.is_empty() {
            render::render_parse_diagnostics(&db, &parse_diags, &self.file_path, &cwd);
            bail!("Parse error");
        }

        let resolved = datalit::resolve::resolve_names(&db, source, ast);
        let result = datalit::tycheck::type_check(&db, ast, resolved);
        let type_diags = datalit::tycheck::type_check::accumulated::<datalove_diagnostic::TypeDiagnostic>(&db, ast, resolved);
        render::render_type_diagnostics(&db, &type_diags, &self.file_path, &cwd);
        if !result.errors(&db).is_empty() {
            bail!("Type error");
        }

        println!("No type errors found.");
        Ok(())
    }
}

impl LitAstCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use datalove_datalit as datalit;
        use bct::input::Source;

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
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use datalove_datalit as datalit;
        use bct::input::Source;

        let db = datalit::Database::default();

        // Read the file.
        let contents = rmx::std::fs::read_to_string(&self.file_path)?;
        let source = Source::new(&db, contents.S());

        // Parse the expression.
        let parse_result = datalit::parser::parse(&db, source);
        let ast = parse_result.expr(&db);

        // Pretty print using the pretty printer.
        let pretty_printed = datalit::pretty::pretty_print(&db, ast);
        println!("{}", pretty_printed);

        Ok(())
    }
}

impl LitOpCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use datalove_datalit as datalit;
        use bct::input::Source;

        let db = datalit::Database::default();

        // Parse both expressions.
        let source1 = Source::new(&db, self.expr1.S());
        let parse_result1 = datalit::parser::parse(&db, source1);
        let expr1 = parse_result1.expr(&db);

        let source2 = Source::new(&db, self.expr2.S());
        let parse_result2 = datalit::parser::parse(&db, source2);
        let expr2 = parse_result2.expr(&db);

        // Resolve and type check.
        let resolved1 = datalit::resolve::resolve_names(&db, source1, expr1);
        let typechecked1 = datalit::tycheck::type_check(&db, expr1, resolved1);

        let resolved2 = datalit::resolve::resolve_names(&db, source2, expr2);
        let typechecked2 = datalit::tycheck::type_check(&db, expr2, resolved2);

        // Check for type errors.
        if !typechecked1.errors(&db).is_empty() {
            let errors: Vec<_> = typechecked1.errors(&db)
                .iter()
                .map(|e| e.error(&db))
                .collect();
            bail!("Type errors in first expression: {:?}", errors);
        }
        if !typechecked2.errors(&db).is_empty() {
            let errors: Vec<_> = typechecked2.errors(&db)
                .iter()
                .map(|e| e.error(&db))
                .collect();
            bail!("Type errors in second expression: {:?}", errors);
        }

        // Instantiate values with RAII guards for cleanup.
        let rt = datalove_rt::rust::Runtime::new();
        let mut tydesc_table1 = datalit::tydesc_table::TyDescTable::new(&db);
        let inst1 = datalit::instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table1, typechecked1)?;
        let _guard1 = unsafe {
            datalove_rt::rust::ValueGuard::from_raw(rt.handle(), inst1.tydesc.as_ptr(), inst1.ptr as *mut u8)
        };
        let mut tydesc_table2 = datalit::tydesc_table::TyDescTable::new(&db);
        let inst2 = datalit::instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table2, typechecked2)?;
        let _guard2 = unsafe {
            datalove_rt::rust::ValueGuard::from_raw(rt.handle(), inst2.tydesc.as_ptr(), inst2.ptr as *mut u8)
        };

        // Execute the operation.
        match self.op.as_str() {
            "eq" => {
                // Call dtlv_rti_eq.
                let result = unsafe {
                    datalove_rt::c::dtlv_rti_eq_local(
                        std::ptr::null_mut(), // runtime handle not needed
                        inst1.ptr,
                        inst1.tydesc.as_ptr(),
                        inst2.ptr,
                        inst2.tydesc.as_ptr(),
                    )
                };

                match result {
                    datalove_rt::c::RtEq::Equals => println!("true"),
                    datalove_rt::c::RtEq::NotEquals => println!("false"),
                    datalove_rt::c::RtEq::Error => bail!("Type mismatch in equality comparison"),
                }
            }
            "cmp" => {
                // Call dtlv_rti_cmp_total.
                let result = unsafe {
                    datalove_rt::c::dtlv_rti_cmp_total_local(
                        std::ptr::null_mut(), // runtime handle not needed
                        inst1.ptr,
                        inst1.tydesc.as_ptr(),
                        inst2.ptr,
                        inst2.tydesc.as_ptr(),
                    )
                };

                match result {
                    datalove_rt::c::RtOrdering::Less => println!("less"),
                    datalove_rt::c::RtOrdering::Equal => println!("equal"),
                    datalove_rt::c::RtOrdering::Greater => println!("greater"),
                    datalove_rt::c::RtOrdering::Error => bail!("Type mismatch in comparison"),
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
    fn run(&self, _args: &Args) -> AnyResult<()> {
        if let Some(script_path) = &self.script {
            let db = datalove_datafun::Database::default();
            datalove_repl::Engine::run_script(&db, datalove_stdlib::system_library(), script_path)
        } else {
            datalove_repl_rat::run(datalove_stdlib::system_library)
        }
    }
}

impl ScriptCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        if self.jit {
            // Run with JIT in a spawned thread to work around Cranelift JIT
            // limitations with PIE binaries.
            let file_path = self.file_path.clone();
            let no_sys = self.no_sys;
            std::thread::spawn(move || {
                Self::run_impl(&file_path, no_sys, true)
            }).join().expect("script thread panicked")
        } else {
            Self::run_impl(&self.file_path, self.no_sys, false)
        }
    }

    fn run_impl(file_path: &PathBuf, no_sys: bool, jit: bool) -> AnyResult<()> {
        use datalove_datafun as datafun;
        use datafun::pipeline::WorkspaceDescriptor;

        let db = datafun::Database::default();

        // Build workspace descriptor.
        let sys = datalove_stdlib::system_library();
        let descriptor = if no_sys {
            WorkspaceDescriptor::empty()
        } else {
            WorkspaceDescriptor::from_system_library(&sys)
        };

        // Read the script file, which is what says where compilation starts.
        let script_source = rmx::std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read script file: {}", file_path.display()))?;

        // Create pipeline from descriptor and compile.
        let mut pipeline = descriptor.to_pipeline(&db);
        // Compile only what the script reaches. A program that requires two
        // stdlib modules should not pay for two dozen; `narrow_roots_to_script`
        // refuses when a require does not resolve, leaving the whole world for
        // resolution to report on.
        pipeline.narrow_roots_to_script(&db, &script_source, &[]);
        let compiled = pipeline.compile_fresh(&db);

        // Check for errors using consolidated helper methods.
        if compiled.has_errors() {
            let errors = compiled.all_errors();
            bail!("Compilation failed with {} error(s):\n{}", errors.len(), errors.join("\n"));
        }

        // Create JIT engine if --jit flag is set (threshold=1 compiles on first call).
        let call_dispatcher: Option<Box<dyn datalove_datafun_interp::CallDispatcher>> = if jit {
            let jit_engine = datalove_datafun_cranelift_jit::JitEngine::new(1)
                .map_err(|e| anyhow!("Failed to create JIT engine: {:?}", e))?;
            Some(Box::new(jit_engine))
        } else {
            None
        };

        // Create script compiler and executor with Stderr mode for debuglog output.
        // Safe to unwrap since we checked has_errors() above.
        let mut compiler = compiled.script_compiler_default(&db)
            .expect("script_compiler should succeed after error check");
        let mut executor = compiled.script_executor(datafun::DebugOutputMode::Stderr, call_dispatcher)
            .expect("script_executor should succeed after error check");

        register_natives(&compiled, &sys, &mut executor)?;

        // Compile the script as a fragment.
        let compiled_unit = compiler.compile_fragment(&script_source);

        // Check for errors and render diagnostics.
        let cwd = rmx::std::env::current_dir().unwrap_or_default();
        if let datafun::pipeline::TypecheckResult::ParseError { errors: _ } = &compiled_unit.typecheck {
            let parse_diags = compiler.get_parse_diagnostics();
            render::render_parse_diagnostics(compiler.db(), &parse_diags, file_path, &cwd);
            bail!("Parse error");
        }
        if let datafun::pipeline::TypecheckResult::Error { errors } = &compiled_unit.typecheck {
            let type_diags = compiler.get_type_diagnostics();
            render::render_type_diagnostics(compiler.db(), &type_diags, file_path, &cwd);
            return Err(type_error(&type_diags, errors));
        }
        bail_on_ownership_error(&compiled_unit, &compiler, file_path, &cwd)?;
        if let datafun::pipeline::LoweringResult::Error { message } = &compiled_unit.lowering {
            bail!("Lowering error: {}", message);
        }

        // Execute the compiled unit.
        let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
            executor.execute_fragment(ir_unit)
        } else {
            String::new()
        };
        if output.starts_with("Error:") {
            bail!("{}", output);
        }

        // Look up the "output" binding from the exports.
        if let Some((ty, value)) = executor.get_binding("output") {
            let _ = ty; // Type not needed for display.
            println!("{}", value);
        }
        // No output binding found - this is okay, just don't print anything.

        // Cleanup.
        executor.destroy_live_values();

        Ok(())
    }
}

impl ScriptIrCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use datalove_datafun as datafun;
        use datafun::pipeline::WorkspaceDescriptor;

        let db = datafun::Database::default();

        // Build workspace descriptor.
        let sys = datalove_stdlib::system_library();
        let descriptor = if self.no_sys {
            WorkspaceDescriptor::empty()
        } else {
            WorkspaceDescriptor::from_system_library(&sys)
        };

                // Read the script file, which is what says where compilation starts.
        let script_source = rmx::std::fs::read_to_string(&self.file_path)
            .with_context(|| format!("Failed to read script file: {}", self.file_path.display()))?;

        let mut pipeline = descriptor.to_pipeline(&db);
        // Compile only what the script reaches. A program that requires two
        // stdlib modules should not pay for two dozen; `narrow_roots_to_script`
        // refuses when a require does not resolve, leaving the whole world for
        // resolution to report on.
        pipeline.narrow_roots_to_script(&db, &script_source, &[]);
        let compiled = pipeline.compile_fresh(&db);

        // Check for errors.
        if compiled.has_errors() {
            let errors = compiled.all_errors();
            bail!("Compilation failed with {} error(s):\n{}", errors.len(), errors.join("\n"));
        }

        // Create script compiler.
        // Safe to unwrap since we checked has_errors() above.
        let mut compiler = compiled.script_compiler_default(&db)
            .expect("script_compiler should succeed after error check");


        let compiled_unit = compiler.compile_fragment(&script_source);

        // Check for errors and render diagnostics.
        let cwd = rmx::std::env::current_dir().unwrap_or_default();
        if let datafun::pipeline::TypecheckResult::ParseError { errors: _ } = &compiled_unit.typecheck {
            let parse_diags = compiler.get_parse_diagnostics();
            render::render_parse_diagnostics(compiler.db(), &parse_diags, &self.file_path, &cwd);
            bail!("Parse error");
        }
        if let datafun::pipeline::TypecheckResult::Error { errors } = &compiled_unit.typecheck {
            let type_diags = compiler.get_type_diagnostics();
            render::render_type_diagnostics(compiler.db(), &type_diags, &self.file_path, &cwd);
            return Err(type_error(&type_diags, errors));
        }
        bail_on_ownership_error(&compiled_unit, &compiler, &self.file_path, &cwd)?;
        if let datafun::pipeline::LoweringResult::Error { message } = &compiled_unit.lowering {
            bail!("Lowering error: {}", message);
        }

        // Get the IR unit and print it.
        let ir_unit = compiled_unit.ir_unit
            .ok_or_else(|| anyhow!("IR unit not available after lowering"))?;

        println!("{}", ir_unit);

        Ok(())
    }
}

impl AotCompileCommand {
    /// Compile through C rather than straight to an object.
    ///
    /// The C backend produces one source file per module plus one for the
    /// script, and a C compiler turns the set of them into the executable in
    /// one step, so there is no unlinked halfway point to write out.
    fn compile_via_c(
        &self,
        ir_unit: &datalove_datafun_ir::IrCodeUnit,
        registry: &datalove_datafun_ir::FunctionRegistry,
        rider_libs: &[PathBuf],
        should_link: bool,
    ) -> AnyResult<()> {
        use datalove_datafun::pipeline::{aot, c_aot};

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

    fn run(&self, _args: &Args) -> AnyResult<()> {
        use datalove_datafun as datafun;
        use datafun::pipeline::{WorkspaceDescriptor, aot};

        let db = datafun::Database::default();

        // Build workspace descriptor.
        let sys = datalove_stdlib::system_library();
        let descriptor = if self.no_sys {
            WorkspaceDescriptor::empty()
        } else {
            WorkspaceDescriptor::from_system_library(&sys)
        };

                // Read the script file, which is what says where compilation starts.
        let script_source = rmx::std::fs::read_to_string(&self.file_path)
            .with_context(|| format!("Failed to read script file: {}", self.file_path.display()))?;

        let mut pipeline = descriptor.to_pipeline(&db);
        // Compile only what the script reaches. A program that requires two
        // stdlib modules should not pay for two dozen; `narrow_roots_to_script`
        // refuses when a require does not resolve, leaving the whole world for
        // resolution to report on.
        pipeline.narrow_roots_to_script(&db, &script_source, &[]);

        // Compile modules (typecheck, drop analysis, lower to IR).
        let compiled = pipeline.compile_fresh(&db);

        // Check for errors.
        if compiled.has_errors() {
            let errors = compiled.all_errors();
            bail!("Compilation failed with {} error(s):\n{}", errors.len(), errors.join("\n"));
        }

        // Create script compiler and get registry for AOT.
        // Safe to unwrap since we checked has_errors() above.
        let mut compiler = compiled.script_compiler_default(&db)
            .expect("script_compiler should succeed after error check");
        let registry = compiled.module_registry();


        let compiled_unit = compiler.compile_fragment(&script_source);

        // Check for errors and render diagnostics.
        let cwd = rmx::std::env::current_dir().unwrap_or_default();
        if let datafun::pipeline::TypecheckResult::ParseError { errors: _ } = &compiled_unit.typecheck {
            let parse_diags = compiler.get_parse_diagnostics();
            render::render_parse_diagnostics(compiler.db(), &parse_diags, &self.file_path, &cwd);
            bail!("Parse error");
        }
        if let datafun::pipeline::TypecheckResult::Error { errors } = &compiled_unit.typecheck {
            let type_diags = compiler.get_type_diagnostics();
            render::render_type_diagnostics(compiler.db(), &type_diags, &self.file_path, &cwd);
            return Err(type_error(&type_diags, errors));
        }
        bail_on_ownership_error(&compiled_unit, &compiler, &self.file_path, &cwd)?;
        if let datafun::pipeline::LoweringResult::Error { message } = &compiled_unit.lowering {
            bail!("Lowering error: {}", message);
        }

        // Get the IR unit.
        let ir_unit = compiled_unit.ir_unit
            .ok_or_else(|| anyhow!("IR unit not available after lowering"))?;

        // The runtime and riders the emitted program links against.
        let rider_libs = vec![datalove_stdlib::native_component_staticlib()?];

        // --run implies --link.
        let should_link = self.link || self.run;

        if self.c {
            return self.compile_via_c(&ir_unit, &registry, &rider_libs, should_link);
        }

        // Compile to object bytes using pipeline::aot.
        let obj_bytes = aot::compile_script_to_object_with_world(
            &ir_unit,
            registry.iter_all_code_units(),
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
}

impl ScriptWorldCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use datalove_datafun as datafun;
        use datafun::pipeline::WorkspaceDescriptor;
        use datalove_datafun_pkg::package_load_worldfile::{parse_worldfile_sections, WorldfileSection};

        let db = datafun::Database::default();

        // Read and parse the worldfile.
        let file_contents = rmx::std::fs::read_to_string(&self.file_path)
            .with_context(|| format!("Failed to read worldfile: {}", self.file_path.display()))?;
        let parsed = parse_worldfile_sections(file_contents.as_bytes())
            .with_context(|| format!("Failed to parse worldfile: {}", self.file_path.display()))?;

        // Validate: must have exactly one script section (scriptunit-fragment or scriptunit-expr).
        let script_sections: Vec<_> = parsed.sections.iter()
            .filter(|s| matches!(s, WorldfileSection::ScriptFragment { .. } | WorldfileSection::ScriptExpr { .. }))
            .collect();
        if script_sections.len() != 1 {
            bail!(
                "Worldfile must have exactly one script section (scriptunit-fragment or scriptunit-expr), found {}",
                script_sections.len()
            );
        }

        // Build workspace descriptor from sys library + worldfile sections.
        let sys = datalove_stdlib::system_library();
        let sys_descriptor = if self.no_sys {
            WorkspaceDescriptor::empty()
        } else {
            WorkspaceDescriptor::from_system_library(&sys)
        };
        let worldfile_descriptor = WorkspaceDescriptor::from_worldfile_sections(
            &parsed.sections,
            datafun::pipeline::CompilerOptions::default(),
        );
        let descriptor = sys_descriptor.merge(&worldfile_descriptor);
        let mut pipeline = descriptor.to_pipeline(&db);

        // Compile only what the script section reaches, the same as `script`
        // does. The worldfile's own modules are in the world alongside the
        // system library, and a script that requires two of them should not pay
        // for the rest.
        // The worldfile's own modules are roots whether the script reaches them or
        // not: the author wrote them in the file they asked to be compiled, so an
        // error in one is an error in the worldfile. Only the system library is
        // pruned here.
        let worldfile_modules: Vec<String> = parsed.sections.iter()
            .filter_map(|section| match section {
                WorldfileSection::Module { library, package, module, .. } =>
                    Some(format!("{}/{}/{}", library, package, module)),
                _ => None,
            })
            .collect();
        if let WorldfileSection::ScriptFragment { source } | WorldfileSection::ScriptExpr { source }
            = script_sections[0]
        {
            pipeline.narrow_roots_to_script(&db, source, &worldfile_modules);
        }

        // Compile modules.
        let compiled = pipeline.compile_fresh(&db);

        // Check for module compilation errors.
        let cwd = rmx::std::env::current_dir().unwrap_or_default();
        if compiled.has_errors() {
            // Render module type diagnostics with ariadne.
            let type_diags = compiled.get_module_type_diagnostics(&db);
            if !type_diags.is_empty() {
                render::render_module_type_diagnostics(&db, &type_diags, &self.file_path, &cwd);
            }

            // Also report any lowering errors that don't have diagnostics yet.
            for error in compiled.all_lowering_errors() {
                eprintln!("{}", error);
            }

            bail!("Module compilation error");
        }

        // Create script compiler and executor.
        // Safe to unwrap since we checked has_errors() above.
        let mut compiler = compiled.script_compiler_default(&db)
            .expect("script_compiler should succeed after error check");
        let mut executor = compiled.script_executor(datafun::DebugOutputMode::Stderr, None)
            .expect("script_executor should succeed after error check");

        register_natives(&compiled, &sys, &mut executor)?;

        // Compile and execute the script section.
        let script_section = script_sections[0];
        let (compiled_unit, output) = match script_section {
            WorldfileSection::ScriptFragment { source } => {
                let compiled_unit = compiler.compile_fragment(source);
                let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
                    executor.execute_fragment(ir_unit)
                } else {
                    String::new()
                };
                (compiled_unit, output)
            }
            WorldfileSection::ScriptExpr { source } => {
                let compiled_unit = compiler.compile_expr(source);
                let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
                    let (_, value) = executor.execute_expr(ir_unit);
                    value
                } else {
                    String::new()
                };
                (compiled_unit, output)
            }
            _ => unreachable!(),
        };

        // Check for script errors and render diagnostics.
        if let datafun::pipeline::TypecheckResult::ParseError { errors: _ } = &compiled_unit.typecheck {
            let parse_diags = compiler.get_parse_diagnostics();
            render::render_parse_diagnostics(compiler.db(), &parse_diags, &self.file_path, &cwd);
            bail!("Parse error");
        }
        if let datafun::pipeline::TypecheckResult::Error { errors } = &compiled_unit.typecheck {
            let type_diags = compiler.get_type_diagnostics();
            render::render_type_diagnostics(compiler.db(), &type_diags, &self.file_path, &cwd);
            return Err(type_error(&type_diags, errors));
        }
        bail_on_ownership_error(&compiled_unit, &compiler, &self.file_path, &cwd)?;
        if let datafun::pipeline::LoweringResult::Error { message } = &compiled_unit.lowering {
            bail!("Lowering error: {}", message);
        }
        if output.starts_with("Error:") {
            bail!("{}", output);
        }

        // Print output if any (for scriptunit-expr).
        if !output.is_empty() && output != "(fragment executed)" {
            println!("{}", output);
        }

        // Cleanup.
        executor.destroy_live_values();

        Ok(())
    }
}

impl TypecheckStdCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use datalove_datafun as datafun;

        let db = datafun::Database::default();

        let sys = datalove_stdlib::system_library();
        let descriptor = datafun::pipeline::WorkspaceDescriptor::from_system_library(&sys);

        let mut pipeline = descriptor.to_pipeline(&db);
        let compiled = pipeline.compile_fresh(&db);

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
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use megaspace_pipeliner::{DocSetConfig, RssConfig};

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let manifest_path = PathBuf::from(manifest_dir);
        let project_root = manifest_path
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| anyhow!("Failed to find project root"))?;

        let static_assets = &["style.css", "template.html", "datalove-logo.svg", "datalove-prism.js"];

        // Build mandocs -> docs/.
        let mandocs_dir = project_root.join("mandocs");
        let mandocs_out = project_root.join("docs");
        let mandocs_extra: &[(&str, &str)] = &[
            ("cross_link_url", "bot/index.html"),
            ("cross_link_label", "Botdocs"),
        ];

        megaspace_pipeliner::build_docs(&DocSetConfig {
            input_dir: &mandocs_dir,
            output_dir: &mandocs_out,
            static_assets,
            extra_context: mandocs_extra,
        })?;

        megaspace_pipeliner::build_posts(
            &mandocs_dir,
            &mandocs_out,
            &RssConfig {
                site_title: "Datalove",
                site_description: "Updates from Datalove",
                base_url: "https://datalove.dev",
            },
            mandocs_extra,
        )?;

        println!("Documentation generated in {}", mandocs_out.display());

        // Build botdocs -> docs/bot/.
        let botdocs_dir = project_root.join("botdocs");
        let botdocs_out = project_root.join("docs").join("bot");

        megaspace_pipeliner::build_docs(&DocSetConfig {
            input_dir: &botdocs_dir,
            output_dir: &botdocs_out,
            static_assets,
            extra_context: &[
                ("cross_link_url", "../index.html"),
                ("cross_link_label", "Mandocs"),
            ],
        })?;

        println!("Documentation generated in {}", botdocs_out.display());

        Ok(())
    }
}

impl WorldgenCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
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
