use rmx::prelude::*;

use datalove_datafun_resolve::{resolve_all_names, resolve_all_exports, build_all_function_ast_maps};
use rmx::clap::{self, Parser as _};
use rmx::std::path::PathBuf;

/// Prefix relative links in rendered nav HTML with a path prefix.
///
/// Leaves absolute URLs, anchors, and root-relative links unchanged.
fn prefix_nav_links(nav_html: &str, path_prefix: &str) -> String {
    if path_prefix.is_empty() {
        return nav_html.to_string();
    }
    let re = rmx::regex::Regex::new(r#"href="([^"]+)""#).unwrap();
    re.replace_all(nav_html, |caps: &rmx::regex::Captures| {
        let link = &caps[1];
        if link.starts_with("http://")
            || link.starts_with("https://")
            || link.starts_with('#')
            || link.starts_with('/')
        {
            caps[0].to_string()
        } else {
            format!(r#"href="{path_prefix}{link}""#)
        }
    }).into_owned()
}

mod feed;
mod render;

/// Build the native component (runtime + riders) and return the static library path for AOT linking.
fn build_native_component_for_aot(
    pipeline: &datalove_datafun::pipeline::ModuleCompilationPipeline,
) -> AnyResult<Vec<std::path::PathBuf>> {
    use datalove_datafun::pipeline::rider_build;

    let result = rider_build::build_native_component(pipeline.rider_crate_dirs())
        .map_err(|e| anyhow!("{}", e))?;
    Ok(vec![result.staticlib_path])
}

/// Build unified native component and load it, registering native symbols in the executor.
///
/// Returns the loaded rider handle, which must be kept alive for the duration of execution.
fn build_and_load_riders(
    pipeline: &datalove_datafun::pipeline::ModuleCompilationPipeline,
    compiled: &datalove_datafun::pipeline::CompiledModules,
    executor: &mut datalove_datafun::pipeline::ScriptExecutor,
) -> AnyResult<Vec<datalove_datafun::pipeline::rider_load::LoadedRider>> {
    use datalove_datafun::pipeline::{rider_build, rider_load};

    let build_result = rider_build::build_native_component(pipeline.rider_crate_dirs())
        .map_err(|e| anyhow!("{}", e))?;

    let native_symbols = compiled.native_symbols();
    if native_symbols.is_empty() {
        return Ok(Vec::new());
    }

    let loaded = rider_load::load_rider_library(
        &build_result.cdylib_path,
        "native-component",
        &native_symbols,
        executor.native_table_mut(),
    )?;
    Ok(vec![loaded])
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

impl LitTycheckCommand {
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

        // Resolve names.
        let resolved = datalit::resolve::resolve_names(&db, source, ast);

        // Type check.
        let result = datalit::tycheck::type_check(&db, ast, resolved);

        // Report errors.
        let errors = result.errors(&db);
        if errors.is_empty() {
            println!("No type errors found.");
        } else {
            println!("Type errors found:");
            for error in errors {
                println!("  {:?}", error.error(&db));
            }
        }

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
            datalove_repl::Engine::run_script(&db, script_path)
        } else {
            datalove_repl_term::run()
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
        use datafun::pipeline::ModuleCompilationPipeline;

        let db = datafun::Database::default();

        // Load sys library unless --no-sys.
        let mut pipeline = ModuleCompilationPipeline::default();
        if !no_sys {
            rmx::futures::executor::block_on(pipeline.load_sys_library_default(&db))?;
        }

        // Compile modules (typecheck, drop analysis, lower to IR).
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

        // Build and load rider shared libraries.
        let _loaded_riders = build_and_load_riders(&pipeline, &compiled, &mut executor)?;

        // Read the script file.
        let script_source = rmx::std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read script file: {}", file_path.display()))?;

        // Compile the script as a fragment.
        let compiled_unit = compiler.compile_fragment(&script_source);

        // Check for errors and render diagnostics.
        let cwd = rmx::std::env::current_dir().unwrap_or_default();
        if let datafun::pipeline::TypecheckResult::ParseError { errors: _ } = &compiled_unit.typecheck {
            let parse_diags = compiler.get_parse_diagnostics();
            render::render_parse_diagnostics(compiler.db(), &parse_diags, file_path, &cwd);
            bail!("Parse error");
        }
        if let datafun::pipeline::TypecheckResult::Error { errors: _ } = &compiled_unit.typecheck {
            let type_diags = compiler.get_type_diagnostics();
            render::render_type_diagnostics(compiler.db(), &type_diags, file_path, &cwd);
            bail!("Type error");
        }
        if let datafun::pipeline::OwnershipResult::Error { message: _ } = &compiled_unit.ownership {
            // Render ownership diagnostics directly using structured errors and spans.
            if let Some(spans) = compiler.get_last_spans() {
                render::render_ownership_errors_direct(
                    compiler.db(),
                    compiler.get_ownership_errors(),
                    spans,
                    file_path,
                    &cwd,
                );
            }
            bail!("Ownership error");
        }
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
        use datafun::pipeline::ModuleCompilationPipeline;

        let db = datafun::Database::default();

        // Load sys library unless --no-sys.
        let mut pipeline = ModuleCompilationPipeline::default();
        if !self.no_sys {
            rmx::futures::executor::block_on(pipeline.load_sys_library_default(&db))?;
        }

        // Compile modules (typecheck, drop analysis, lower to IR).
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

        // Read the script file.
        let script_source = rmx::std::fs::read_to_string(&self.file_path)
            .with_context(|| format!("Failed to read script file: {}", self.file_path.display()))?;

        let compiled_unit = compiler.compile_fragment(&script_source);

        // Check for errors and render diagnostics.
        let cwd = rmx::std::env::current_dir().unwrap_or_default();
        if let datafun::pipeline::TypecheckResult::ParseError { errors: _ } = &compiled_unit.typecheck {
            let parse_diags = compiler.get_parse_diagnostics();
            render::render_parse_diagnostics(compiler.db(), &parse_diags, &self.file_path, &cwd);
            bail!("Parse error");
        }
        if let datafun::pipeline::TypecheckResult::Error { errors: _ } = &compiled_unit.typecheck {
            let type_diags = compiler.get_type_diagnostics();
            render::render_type_diagnostics(compiler.db(), &type_diags, &self.file_path, &cwd);
            bail!("Type error");
        }
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
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use datalove_datafun as datafun;
        use datafun::pipeline::{ModuleCompilationPipeline, aot};

        let db = datafun::Database::default();

        // Load sys library unless --no-sys.
        let mut pipeline = ModuleCompilationPipeline::default();
        if !self.no_sys {
            rmx::futures::executor::block_on(pipeline.load_sys_library_default(&db))?;
        }

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

        // Read the script file.
        let script_source = rmx::std::fs::read_to_string(&self.file_path)
            .with_context(|| format!("Failed to read script file: {}", self.file_path.display()))?;

        let compiled_unit = compiler.compile_fragment(&script_source);

        // Check for errors and render diagnostics.
        let cwd = rmx::std::env::current_dir().unwrap_or_default();
        if let datafun::pipeline::TypecheckResult::ParseError { errors: _ } = &compiled_unit.typecheck {
            let parse_diags = compiler.get_parse_diagnostics();
            render::render_parse_diagnostics(compiler.db(), &parse_diags, &self.file_path, &cwd);
            bail!("Parse error");
        }
        if let datafun::pipeline::TypecheckResult::Error { errors: _ } = &compiled_unit.typecheck {
            let type_diags = compiler.get_type_diagnostics();
            render::render_type_diagnostics(compiler.db(), &type_diags, &self.file_path, &cwd);
            bail!("Type error");
        }
        if let datafun::pipeline::LoweringResult::Error { message } = &compiled_unit.lowering {
            bail!("Lowering error: {}", message);
        }

        // Get the IR unit.
        let ir_unit = compiled_unit.ir_unit
            .ok_or_else(|| anyhow!("IR unit not available after lowering"))?;

        // Build rider crates for linking.
        let rider_libs = build_native_component_for_aot(&pipeline)?;

        // Compile to object bytes using pipeline::aot.
        let obj_bytes = aot::compile_script_to_object_with_world(
            &ir_unit,
            registry.iter_all_code_units(),
            &registry,
        )?;

        // --run implies --link.
        let should_link = self.link || self.run;

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
        use datafun::pipeline::ModuleCompilationPipeline;
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

        // Build pipeline from module sections.
        let mut pipeline = ModuleCompilationPipeline::default();
        if !self.no_sys {
            rmx::futures::executor::block_on(pipeline.load_sys_library_default(&db))?;
        }
        pipeline.add_modules_from_sections(&db, &parsed.sections);

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

        // Build and load rider shared libraries.
        let _loaded_riders = build_and_load_riders(&pipeline, &compiled, &mut executor)?;

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
        if let datafun::pipeline::TypecheckResult::Error { errors: _ } = &compiled_unit.typecheck {
            let type_diags = compiler.get_type_diagnostics();
            render::render_type_diagnostics(compiler.db(), &type_diags, &self.file_path, &cwd);
            bail!("Type error");
        }
        if let datafun::pipeline::OwnershipResult::Error { message: _ } = &compiled_unit.ownership {
            if let Some(spans) = compiler.get_last_spans() {
                render::render_ownership_errors_direct(
                    compiler.db(),
                    compiler.get_ownership_errors(),
                    spans,
                    &self.file_path,
                    &cwd,
                );
            }
            bail!("Ownership error");
        }
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

        // Load package world from sys/ directory.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let manifest_path = rmx::std::path::PathBuf::from(manifest_dir);
        let parent = match manifest_path.parent() {
            Some(p) => p,
            None => bail!("Failed to get parent directory"),
        };
        let grandparent = match parent.parent() {
            Some(p) => p,
            None => bail!("Failed to get grandparent directory"),
        };
        let sys_dir = grandparent.join("sys");

        println!("Loading sys/ from: {}", sys_dir.display());

        let config = datafun::package_load::PackageWorldConfig {
            dir_pkglib_system: sys_dir,
            dir_pkglib_local: None,
        };

        let package_world_raw = rmx::futures::executor::block_on(
            datafun::package_load::load_world(config)
        )?;

        // Extract rider sources before converting to Salsa types.
        let rider_sources = package_world_raw.rider_sources();

        let package_world = datafun::package::import_from_loader(&db, package_world_raw);

        // Resolve and convert to ModuleGraph.
        let resolution = datafun::package_resolve::resolve_package_world_with_imports(&db, package_world);
        let pkg_graph = match resolution.result(&db) {
            Ok(g) => g,
            Err(e) => bail!("Package resolution failed: {:?}", e),
        };

        // Convert to package-agnostic ModuleGraph, parse, and typecheck.
        let graph_with_requires = datafun::to_module_graph(&db, package_world, pkg_graph);
        let module_graph = graph_with_requires.graph;
        let parsed_graph = datafun::module_graph::parse_module_graph(&db, module_graph, graph_with_requires.resolved_requires, rider_sources);
        let all_names = resolve_all_names(&db, parsed_graph);
        let all_exports = resolve_all_exports(&db, parsed_graph);
        let all_function_asts = build_all_function_ast_maps(&db, parsed_graph);
        let typecheck_result = datalove_datafun_tycheck::typecheck_module_graph(&db, parsed_graph, all_names, all_exports, all_function_asts, datalove_datafun_tycheck::AutoAdaptMode::Disabled);

        // Report typecheck errors.
        let module_errors = typecheck_result.module_errors(&db);
        if module_errors.is_empty() {
            println!("No typecheck errors found in sys/std.");
            Ok(())
        } else {
            let error_count: usize = module_errors.values().map(|v| v.len()).sum();
            println!("Found {} typecheck error(s):", error_count);
            for (module_id, errors) in module_errors.iter() {
                for err in errors {
                    println!("  {}: {:?}", module_id.path(&db), err);
                }
            }
            bail!("Typecheck failed with {} error(s)", error_count);
        }
    }
}

impl DocsCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use rmx::std::fs;

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let manifest_path = PathBuf::from(manifest_dir);
        let project_root = manifest_path
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| anyhow!("Failed to find project root"))?;

        // Build mandocs -> docs/.
        let mandocs_dir = project_root.join("mandocs");
        let mandocs_out = project_root.join("docs");

        let mandocs_nav = Self::render_nav_file(&mandocs_dir)?;

        Self::build_docs(
            &mandocs_dir,
            &mandocs_out,
            &mandocs_nav,
            "bot/index.html",
            "Botdocs",
        )?;

        // Generate posts feed for mandocs.
        let posts_dir = mandocs_dir.join("posts");
        let posts = feed::parse_posts(&posts_dir)?;
        if !posts.is_empty() {
            let mut tera = rmx::tera::Tera::default();
            let posts_template_path = mandocs_dir.join("posts-template.html");
            let posts_template_content = fs::read_to_string(&posts_template_path)
                .with_context(|| format!("Failed to read posts template: {}", posts_template_path.display()))?;
            tera.add_raw_template("posts-template.html", &posts_template_content)?;
            feed::generate_feed_page(
                &posts,
                &tera,
                &mandocs_out,
                &mandocs_nav,
                "bot/index.html",
                "Botdocs",
            )?;
            feed::generate_rss(&posts, &mandocs_out, "https://datalove.dev")?;
        }

        println!("Documentation generated in {}", mandocs_out.display());

        // Build botdocs -> docs/bot/.
        let botdocs_dir = project_root.join("botdocs");
        let botdocs_out = project_root.join("docs").join("bot");

        let botdocs_nav = Self::render_nav_file(&botdocs_dir)?;
        Self::build_docs(
            &botdocs_dir,
            &botdocs_out,
            &botdocs_nav,
            "../index.html",
            "Mandocs",
        )?;

        println!("Documentation generated in {}", botdocs_out.display());

        Ok(())
    }

    /// Build HTML docs from markdown files in `input_dir`, writing to `output_dir`.
    ///
    /// Copies static assets (style.css, template.html, logo, prism.js)
    /// and converts all .md files (including subdirectories) to HTML.
    fn build_docs(
        input_dir: &std::path::Path,
        output_dir: &std::path::Path,
        nav_html: &str,
        cross_link_url: &str,
        cross_link_label: &str,
    ) -> AnyResult<()> {
        use rmx::std::fs;
        use rmx::tera::{Tera, Context};

        fs::create_dir_all(output_dir)?;

        // Load template.
        let template_path = input_dir.join("template.html");
        let template_content = fs::read_to_string(&template_path)
            .with_context(|| format!("Failed to read template: {}", template_path.display()))?;

        let mut tera = Tera::default();
        tera.add_raw_template("page", &template_content)?;

        // Copy static assets.
        for asset in &["style.css", "template.html", "datalove-logo.svg", "datalove-prism.js"] {
            let src = input_dir.join(asset);
            if src.exists() {
                let dst = output_dir.join(asset);
                fs::copy(&src, &dst)
                    .with_context(|| format!("Failed to copy {}", asset))?;
                println!("Copied {}", asset);
            }
        }

        // Collect all markdown files, including subdirectories.
        let mut md_files = Vec::new();
        Self::collect_markdown_files(input_dir, input_dir, &mut md_files)?;

        // Process all markdown files.
        for (rel_path, abs_path) in &md_files {
            let file_name = abs_path.file_name().unwrap().to_string_lossy();

            // Determine output path, preserving subdirectory structure.
            let output_rel = if file_name == "README.md" {
                rel_path.with_file_name("index.html")
            } else {
                rel_path.with_extension("html")
            };

            // Ensure output subdirectory exists.
            let output_path = output_dir.join(&output_rel);
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)?;
            }

            // Read and convert markdown.
            let markdown = fs::read_to_string(abs_path)?;
            let markdown = Self::rewrite_links(&markdown);

            // Convert to HTML with GFM extensions.
            let mut options = rmx::comrak::Options::default();
            options.extension.table = true;
            options.extension.strikethrough = true;
            options.extension.autolink = true;
            options.extension.tasklist = true;
            options.extension.header_ids = Some("user-content-".S());
            options.render.unsafe_ = true;
            let html = rmx::comrak::markdown_to_html(&markdown, &options);

            let title = Self::extract_title(&markdown, &file_name);

            // Compute path_prefix from directory depth of output file.
            let path_prefix = Self::compute_path_prefix(&output_rel);

            let prefixed_nav = prefix_nav_links(nav_html, &path_prefix);

            let mut context = Context::new();
            context.insert("title", &title);
            context.insert("content", &html);
            context.insert("nav_html", &prefixed_nav);
            context.insert("path_prefix", &path_prefix);
            context.insert("cross_link_url", cross_link_url);
            context.insert("cross_link_label", cross_link_label);
            let rendered = tera.render("page", &context)?;

            fs::write(&output_path, rendered)?;
            println!("{} -> {}", rel_path.display(), output_rel.display());
        }

        Ok(())
    }

    /// Recursively collect .md files under `dir`, recording paths relative to `base`.
    fn collect_markdown_files(
        base: &std::path::Path,
        dir: &std::path::Path,
        out: &mut Vec<(PathBuf, PathBuf)>,
    ) -> AnyResult<()> {
        use rmx::std::fs;

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                Self::collect_markdown_files(base, &path, out)?;
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                // Skip nav.md; it is used for sidebar navigation, not content.
                if path.file_name().map(|n| n == "nav.md").unwrap_or(false) {
                    continue;
                }
                let rel = path.strip_prefix(base)?.to_path_buf();
                out.push((rel, path));
            }
        }
        Ok(())
    }

    fn rewrite_links(markdown: &str) -> String {
        use rmx::regex::Regex;

        // Match markdown links: [text](path.md) or [text](path.md#anchor)
        // Also handle README.md -> index.html
        let re = Regex::new(r"\]\(([^)]+)\.md(#[^)]*)?\)").unwrap();

        re.replace_all(markdown, |caps: &rmx::regex::Captures| {
            let path = &caps[1];
            let anchor = caps.get(2).map(|m| m.as_str()).unwrap_or("");

            if path == "README" {
                format!("](index.html{})", anchor)
            } else {
                format!("]({}.html{})", path, anchor)
            }
        }).into_owned()
    }

    fn extract_title(markdown: &str, filename: &str) -> String {
        // Try to extract title from first # heading.
        for line in markdown.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("# ") {
                return trimmed[2..].trim().S();
            }
        }
        // Fall back to filename without extension.
        filename.trim_end_matches(".md").S()
    }

    /// Render `nav.md` from the input directory to HTML.
    ///
    /// Rewrites `.md` links to `.html` and renders the markdown with comrak.
    fn render_nav_file(input_dir: &std::path::Path) -> AnyResult<String> {
        use rmx::std::fs;

        let nav_path = input_dir.join("nav.md");
        let content = fs::read_to_string(&nav_path)
            .with_context(|| format!("Failed to read {}", nav_path.display()))?;

        let content = Self::rewrite_links(&content);

        let mut options = rmx::comrak::Options::default();
        options.render.unsafe_ = true;
        let html = rmx::comrak::markdown_to_html(&content, &options);

        Ok(html)
    }

    /// Compute the path prefix for a file based on its directory depth.
    ///
    /// Root-level files get "", one subdir deep gets "../", etc.
    fn compute_path_prefix(rel_path: &std::path::Path) -> String {
        let depth = rel_path.parent().map_or(0, |p| p.components().count());
        "../".repeat(depth)
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
