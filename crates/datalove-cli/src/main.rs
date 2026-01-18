use rmx::prelude::*;

use rmx::clap::{self, Parser as _};
use rmx::std::path::PathBuf;

mod render;

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
    /// AOT compile a datafun script to native code.
    AotCompile(AotCompileCommand),
    /// Typecheck the sys/std library and report errors.
    TypecheckStd(TypecheckStdCommand),
    /// Generate HTML documentation from mandocs/.
    Docs(DocsCommand),
    /// Execute a worldfile containing modules and a script section.
    ScriptWorld(ScriptWorldCommand),
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

impl Cli {
    fn run(&self) -> AnyResult<()> {
        match &self.cmd {
            Command::LitTycheck(cmd) => cmd.run(&self.args),
            Command::LitAst(cmd) => cmd.run(&self.args),
            Command::LitPretty(cmd) => cmd.run(&self.args),
            Command::LitOp(cmd) => cmd.run(&self.args),
            Command::Repl(cmd) => cmd.run(&self.args),
            Command::Script(cmd) => cmd.run(&self.args),
            Command::AotCompile(cmd) => cmd.run(&self.args),
            Command::TypecheckStd(cmd) => cmd.run(&self.args),
            Command::Docs(cmd) => cmd.run(&self.args),
            Command::ScriptWorld(cmd) => cmd.run(&self.args),
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
        use datalove_datafun as datafun;
        use datafun::pipeline::ModuleCompilationPipeline;

        let db = datafun::Database::default();

        // Load sys library unless --no-sys.
        let mut pipeline = ModuleCompilationPipeline::new();
        if !self.no_sys {
            rmx::futures::executor::block_on(pipeline.load_sys_library_default(&db))?;
        }

        // Compile modules (typecheck, drop analysis, lower to IR).
        let compiled = pipeline.compile_fresh(&db);

        // Check for errors using consolidated helper methods.
        if compiled.has_errors() {
            let errors = compiled.all_errors();
            bail!("Compilation failed with {} error(s):\n{}", errors.len(), errors.join("\n"));
        }

        // Create script compilation context with Stderr mode for debuglog output.
        let mut ctx = compiled.script_context(&db, datafun::DebugOutputMode::Stderr, None);

        // Read the script file.
        let script_source = rmx::std::fs::read_to_string(&self.file_path)
            .with_context(|| format!("Failed to read script file: {}", self.file_path.display()))?;

        // Execute the script as a fragment.
        let result = ctx.eval_fragment(&script_source);

        // Check for errors and render diagnostics.
        let cwd = rmx::std::env::current_dir().unwrap_or_default();
        if let datafun::pipeline::TypecheckResult::ParseError { errors: _ } = &result.typecheck {
            let parse_diags = ctx.get_parse_diagnostics();
            render::render_parse_diagnostics(ctx.db(), &parse_diags, &self.file_path, &cwd);
            bail!("Parse error");
        }
        if let datafun::pipeline::TypecheckResult::Error { errors: _ } = &result.typecheck {
            let type_diags = ctx.get_type_diagnostics();
            render::render_type_diagnostics(ctx.db(), &type_diags, &self.file_path, &cwd);
            bail!("Type error");
        }
        if let datafun::pipeline::LoweringResult::Error { message } = &result.lowering {
            bail!("Lowering error: {}", message);
        }
        if result.output.starts_with("Error:") {
            bail!("{}", result.output);
        }

        // Look up the "output" binding from the exports.
        // After eval_fragment, the exports are in script_ctx and the frame is stored.
        if let Some((unit_idx, value_id)) = ctx.script_ctx.values.get("output").cloned() {
            // It's a let binding - read value from frame.
            let value = ctx.env.frames.external_value(unit_idx, value_id)
                .map_err(|e| anyhow!("Failed to read output value: {:?}", e))?;
            let mut interp = datalove_datafun_interp::IrInterpreter::new();
            let output_str = interp.pretty_print_value(&value)
                .map_err(|e| anyhow!("Failed to pretty print output: {:?}", e))?;
            println!("{}", output_str);
        } else if let Some((unit_idx, slot_id)) = ctx.script_ctx.slots.get("output").cloned() {
            // It's a var binding - read slot from frame.
            let value = ctx.env.frames.external_slot(unit_idx, slot_id)
                .map_err(|e| anyhow!("Failed to read output slot: {:?}", e))?;
            let mut interp = datalove_datafun_interp::IrInterpreter::new();
            let output_str = interp.pretty_print_value(&value)
                .map_err(|e| anyhow!("Failed to pretty print output: {:?}", e))?;
            println!("{}", output_str);
        }
        // No output binding found - this is okay, just don't print anything.

        // Cleanup.
        ctx.destroy_all();

        Ok(())
    }
}

impl AotCompileCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        use datalove_datafun as datafun;
        use datafun::pipeline::{ModuleCompilationPipeline, aot};

        let db = datafun::Database::default();

        // Load sys library unless --no-sys.
        let mut pipeline = ModuleCompilationPipeline::new();
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

        // Create script compilation context.
        let mut ctx = compiled.script_context(&db, datafun::DebugOutputMode::Stderr, None);

        // Read the script file.
        let script_source = rmx::std::fs::read_to_string(&self.file_path)
            .with_context(|| format!("Failed to read script file: {}", self.file_path.display()))?;

        // Lower to IR for AOT (emits drops for script-level bindings).
        let lower_result = ctx.lower_fragment_for_aot(&script_source);

        // Check for errors and render diagnostics.
        let cwd = rmx::std::env::current_dir().unwrap_or_default();
        if let datafun::pipeline::TypecheckResult::ParseError { errors: _ } = &lower_result.typecheck {
            let parse_diags = ctx.get_parse_diagnostics();
            render::render_parse_diagnostics(ctx.db(), &parse_diags, &self.file_path, &cwd);
            bail!("Parse error");
        }
        if let datafun::pipeline::TypecheckResult::Error { errors: _ } = &lower_result.typecheck {
            let type_diags = ctx.get_type_diagnostics();
            render::render_type_diagnostics(ctx.db(), &type_diags, &self.file_path, &cwd);
            bail!("Type error");
        }
        if let datafun::pipeline::LoweringResult::Error { message } = &lower_result.lowering {
            bail!("Lowering error: {}", message);
        }

        // Get the IR unit.
        let ir_unit = lower_result.ir_unit
            .ok_or_else(|| anyhow!("IR unit not available after lowering"))?;

        // Compile to object bytes using pipeline::aot.
        let obj_bytes = aot::compile_script_to_object_with_world(
            &ir_unit,
            ctx.env.registry.iter_all_functions(),
            &ctx.env.registry,
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
            // Link into executable using pipeline::aot.
            aot::link_object_to_path(&obj_bytes, &output_path)
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

        ctx.destroy_all();
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
        let mut pipeline = ModuleCompilationPipeline::new();
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

            // Also report any drop analysis errors that don't have diagnostics yet.
            for error in compiled.all_drop_analysis_errors() {
                eprintln!("{}", error);
            }

            bail!("Module compilation error");
        }

        // Create script compilation context.
        let mut ctx = compiled.script_context(&db, datafun::DebugOutputMode::Stderr, None);

        // Execute the script section.
        let script_section = script_sections[0];
        let result = match script_section {
            WorldfileSection::ScriptFragment { source } => {
                ctx.eval_fragment(source)
            }
            WorldfileSection::ScriptExpr { source } => {
                ctx.eval_expr(source)
            }
            _ => unreachable!(),
        };

        // Check for script errors and render diagnostics.
        if let datafun::pipeline::TypecheckResult::ParseError { errors: _ } = &result.typecheck {
            let parse_diags = ctx.get_parse_diagnostics();
            render::render_parse_diagnostics(ctx.db(), &parse_diags, &self.file_path, &cwd);
            bail!("Parse error");
        }
        if let datafun::pipeline::TypecheckResult::Error { errors: _ } = &result.typecheck {
            let type_diags = ctx.get_type_diagnostics();
            render::render_type_diagnostics(ctx.db(), &type_diags, &self.file_path, &cwd);
            bail!("Type error");
        }
        if let datafun::pipeline::LoweringResult::Error { message } = &result.lowering {
            bail!("Lowering error: {}", message);
        }
        if result.output.starts_with("Error:") {
            bail!("{}", result.output);
        }

        // Print output if any (for scriptunit-expr).
        if !result.output.is_empty() && result.output != "(fragment executed)" {
            println!("{}", result.output);
        }

        // Cleanup.
        ctx.destroy_all();

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
        let parsed_graph = datafun::module_graph::parse_module_graph(&db, module_graph, graph_with_requires.resolved_requires);
        let typecheck_result = datalove_datafun_tycheck::typecheck_module_graph(&db, parsed_graph);

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
        use rmx::tera::{Tera, Context};

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let manifest_path = PathBuf::from(manifest_dir);
        let project_root = manifest_path
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| anyhow!("Failed to find project root"))?;

        let input_dir = project_root.join("mandocs");
        let output_dir = project_root.join("docs");

        // Create output directory.
        fs::create_dir_all(&output_dir)?;

        // Load template.
        let template_path = input_dir.join("template.html");
        let template_content = fs::read_to_string(&template_path)
            .with_context(|| format!("Failed to read template: {}", template_path.display()))?;

        let mut tera = Tera::default();
        tera.add_raw_template("page", &template_content)?;

        // Copy style.css and template.html.
        let style_src = input_dir.join("style.css");
        let style_dst = output_dir.join("style.css");
        fs::copy(&style_src, &style_dst)
            .with_context(|| format!("Failed to copy style.css"))?;
        println!("Copied style.css");

        let template_dst = output_dir.join("template.html");
        fs::copy(&template_path, &template_dst)
            .with_context(|| format!("Failed to copy template.html"))?;
        println!("Copied template.html");

        let logo_src = input_dir.join("datalove-logo.svg");
        let logo_dst = output_dir.join("datalove-logo.svg");
        fs::copy(&logo_src, &logo_dst)
            .with_context(|| format!("Failed to copy datalove-logo.svg"))?;
        println!("Copied datalove-logo.svg");

        // Process all markdown files.
        for entry in fs::read_dir(&input_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "md").unwrap_or(false) {
                let file_name = path.file_name().unwrap().to_string_lossy();

                // Determine output filename.
                let output_name = if file_name == "README.md" {
                    "index.html".S()
                } else {
                    file_name.replace(".md", ".html")
                };

                // Read and convert markdown.
                let markdown = fs::read_to_string(&path)?;

                // Replace .md links with .html links.
                let markdown = Self::rewrite_links(&markdown);

                // Convert to HTML with GFM extensions.
                let mut options = rmx::comrak::Options::default();
                options.extension.table = true;
                options.extension.strikethrough = true;
                options.extension.autolink = true;
                options.extension.header_ids = Some("user-content-".S());
                options.render.unsafe_ = true; // Allow raw HTML in markdown.
                let html = rmx::comrak::markdown_to_html(&markdown, &options);

                // Extract title from first heading or filename.
                let title = Self::extract_title(&markdown, &file_name);

                // Render template.
                let mut context = Context::new();
                context.insert("title", &title);
                context.insert("content", &html);
                let rendered = tera.render("page", &context)?;

                // Write output.
                let output_path = output_dir.join(&output_name);
                fs::write(&output_path, rendered)?;
                println!("{} -> {}", file_name, output_name);
            }
        }

        println!("Documentation generated in {}", output_dir.display());
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
}
