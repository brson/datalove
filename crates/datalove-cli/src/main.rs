#![allow(unused)]

use rmx::prelude::*;

use rmx::clap::{self, Parser as _};
use rmx::std::path::PathBuf;

mod docs;

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
    /// Documentation tools.
    Docs(docs::DocsCommand),
    /// Execute a datafun script.
    Script(ScriptCommand),
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

impl Cli {
    fn run(&self) -> AnyResult<()> {
        match &self.cmd {
            Command::LitTycheck(cmd) => cmd.run(&self.args),
            Command::LitAst(cmd) => cmd.run(&self.args),
            Command::LitPretty(cmd) => cmd.run(&self.args),
            Command::LitOp(cmd) => cmd.run(&self.args),
            Command::Repl(cmd) => cmd.run(&self.args),
            Command::Docs(cmd) => cmd.run(),
            Command::Script(cmd) => cmd.run(&self.args),
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
        let ast = parse_result.expr;

        // Resolve names.
        let resolved = datalit::resolve::resolve_names(&db, ast, parse_result.expr_spans);

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
        let ast = parse_result.expr;

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
        let ast = parse_result.expr;

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
        let expr1 = parse_result1.expr;

        let source2 = Source::new(&db, self.expr2.S());
        let parse_result2 = datalit::parser::parse(&db, source2);
        let expr2 = parse_result2.expr;

        // Resolve and type check.
        let resolved1 = datalit::resolve::resolve_names(&db, expr1, parse_result1.expr_spans);
        let typechecked1 = datalit::tycheck::type_check(&db, expr1, resolved1);

        let resolved2 = datalit::resolve::resolve_names(&db, expr2, parse_result2.expr_spans);
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

        // Instantiate values.
        let mut rt = datalove_rt::rt_local::RtLocal::new();
        let mut tydesc_table1 = datalit::tydesc_table::TyDescTable::new(&db);
        let inst1 = datalit::instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table1, typechecked1)?;
        let mut tydesc_table2 = datalit::tydesc_table::TyDescTable::new(&db);
        let inst2 = datalit::instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table2, typechecked2)?;

        // Execute the operation.
        match self.op.as_str() {
            "eq" => {
                // Call dtlv_rti_eq.
                let result = unsafe {
                    datalove_rt::dtlv_rti_eq(
                        std::ptr::null_mut(), // runtime handle not needed
                        inst1.ptr,
                        inst1.tydesc,
                        inst2.ptr,
                        inst2.tydesc,
                    )
                };

                match result {
                    datalove_rt::RtEq::Equals => println!("true"),
                    datalove_rt::RtEq::NotEquals => println!("false"),
                    datalove_rt::RtEq::Error => bail!("Type mismatch in equality comparison"),
                }
            }
            "cmp" => {
                // Call dtlv_rti_cmp_total.
                let result = unsafe {
                    datalove_rt::dtlv_rti_cmp_total(
                        std::ptr::null_mut(), // runtime handle not needed
                        inst1.ptr,
                        inst1.tydesc,
                        inst2.ptr,
                        inst2.tydesc,
                    )
                };

                match result {
                    datalove_rt::RtOrdering::Less => println!("less"),
                    datalove_rt::RtOrdering::Equal => println!("equal"),
                    datalove_rt::RtOrdering::Greater => println!("greater"),
                    datalove_rt::RtOrdering::Error => bail!("Type mismatch in comparison"),
                }
            }
            _ => {
                bail!("Unknown operation: {}", self.op);
            }
        }

        // Clean up instantiated values before shutdown.
        unsafe {
            let rt_handle = &mut *rt as *mut datalove_rt::rt_local::RtLocal as *mut u8;
            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst1.ptr as *mut u8, inst1.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst1.tydesc, 1, inst1.ptr as *mut u8);
            datalove_rt::dtlv_rti_any_destroy_local(rt_handle, inst2.ptr as *mut u8, inst2.tydesc);
            datalove_rt::dtlv_rti_mem_free_local(rt_handle, inst2.tydesc, 1, inst2.ptr as *mut u8);
            rt.shutdown();
        }
        Ok(())
    }
}

impl ReplCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        if let Some(script_path) = &self.script {
            let db = datalove_repl::datafun::Database::default();
            datalove_repl::Engine::run_script(&db, script_path)
        } else {
            datalove_repl_term::run()
        }
    }
}

impl ScriptCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        if self.no_sys {
            self.run_without_sys()
        } else {
            self.run_with_sys()
        }
    }

    fn render_diagnostics(
        &self,
        db: &dyn datalove_datafun::Db,
        source: bct::input::Source,
    ) -> AnyResult<()> {
        use datalove_diagnostic::ParseDiagnostic;
        use datalove_datafun::parser;

        let parse_diags = parser::parse_for_diagnostics::accumulated::<ParseDiagnostic>(db, source);

        if !parse_diags.is_empty() {
            eprintln!("Parse errors:");
            for diag_wrapper in &parse_diags {
                let diag = diag_wrapper.to_diagnostic(db);
                let code_str = diag.code.map(|c| c.as_str(db)).unwrap_or("");
                let message = diag.message.as_str(db);
                eprintln!("error[{}]: {}", code_str, message);

                for label in &diag.labels {
                    let text_str = label.text.as_str(db);
                    let span_str = &text_str[label.span.start..label.span.end];
                    if let Some(label_msg) = label.message {
                        eprintln!("  --> {}", label_msg.as_str(db));
                    }
                    eprintln!("     | {}", span_str);
                }
            }
            bail!("{} parse error(s)", parse_diags.len());
        }

        Ok(())
    }

    fn run_without_sys(&self) -> AnyResult<()> {
        use datalove_datafun as datafun;
        use bct::input::Source;

        let db = datafun::Database::default();

        // Read the script file.
        let source_text = rmx::std::fs::read_to_string(&self.file_path)?;
        let source = Source::new(&db, source_text.S());

        // Call tracked wrapper to enable diagnostic accumulation and get script.
        let script = datafun::parser::parse_for_diagnostics(&db, source);

        // Render parse diagnostics (must be called after parse_for_diagnostics).
        self.render_diagnostics(&db, source)?;

        // Type check the script.
        // TODO: Pass actual spans once we have a way to retrieve them from parse_for_diagnostics.
        let tycheck_result = datafun::tycheck::type_check(&db, script, vec![], vec![]);
        if !tycheck_result.errors(&db).is_empty() {
            bail!("Type check errors: {} error(s)", tycheck_result.errors(&db).len());
        }

        // Build type table.
        let mut tydesc_table = datafun::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = match datafun::type_table::TypeTable::build(&db, script, tycheck_result, &mut tydesc_table) {
            Ok(table) => table,
            Err(e) => bail!("Failed to build type table: {}", e),
        };

        // Create interpreter context.
        let mut ctx = datafun::interp::InterpContext::new(&db, type_table);

        // Execute the script.
        if let Err(e) = ctx.execute(script) {
            bail!("Execution error: {:?}", e);
        }

        // Pretty-print the 'output' variable.
        let output_name = bct::text::InternedText::new(&db, S("output"));
        let result = match ctx.pretty_print_variable(output_name) {
            Ok(res) => res,
            Err(e) => bail!("Failed to pretty-print output: {:?}", e),
        };

        println!("{}", result);

        Ok(())
    }

    fn run_with_sys(&self) -> AnyResult<()> {
        use datalove_datafun as datafun;
        use bct::input::Source;

        let db = datafun::Database::default();

        // Read the script file.
        let script_text = rmx::std::fs::read_to_string(&self.file_path)?;
        let source = Source::new(&db, script_text.S());

        // Call tracked wrapper to enable diagnostic accumulation and get script.
        let script = datafun::parser::parse_for_diagnostics(&db, source);

        // Render parse diagnostics (must be called after parse_for_diagnostics).
        self.render_diagnostics(&db, source)?;

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

        let config = datafun::package_load::PackageWorldConfig {
            dir_pkglib_system: sys_dir,
            dir_pkglib_local: None,
        };

        let package_world_raw = rmx::futures::executor::block_on(
            datafun::package_load::load_world(config)
        )?;

        let package_world = datafun::package::import_from_loader(&db, package_world_raw);

        // Load and resolve script with package world.
        let script_world = datafun::script_world::load_script_with_package_world(&db, script, package_world);

        // Check if resolution succeeded.
        let resolution = script_world.resolution(&db);
        if let Err(e) = resolution.result(&db) {
            bail!("Package resolution failed: {:?}", e);
        }

        // Get typecheck result.
        let typecheck_result = match script_world.typecheck_result(&db) {
            Some(res) => res,
            None => bail!("Package world typecheck failed"),
        };

        // Check for package world typecheck errors.
        let module_errors = typecheck_result.module_errors(&db);
        if !module_errors.is_empty() {
            let error_count: usize = module_errors.values().map(|v| v.len()).sum();
            bail!("Package world has {} typecheck error(s)", error_count);
        }

        // Typecheck the script with package world context.
        // TODO: Pass actual spans once we have a way to retrieve them from parse_for_diagnostics.
        let script_typecheck = datafun::tycheck::type_check_with_package_world(
            &db,
            script,
            vec![],
            vec![],
            package_world,
            *typecheck_result,
        );

        // Check for script typecheck errors.
        if !script_typecheck.errors(&db).is_empty() {
            let errors: Vec<_> = script_typecheck.errors(&db).iter()
                .map(|e| format!("{:?}", e.error(&db)))
                .collect();
            bail!("Script has {} typecheck error(s):\n{}", errors.len(), errors.join("\n"));
        }

        // Build type table for the script.
        let mut tydesc_table = datafun::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = match datafun::type_table::TypeTable::build(&db, script, script_typecheck, &mut tydesc_table) {
            Ok(table) => table,
            Err(e) => bail!("Failed to build type table: {}", e),
        };

        // Create interpreter context with package world support.
        let mut ctx = datafun::interp::InterpContext::with_package_world(
            &db,
            type_table,
            &script,
            package_world,
            &typecheck_result,
        );

        // Execute the script.
        if let Err(e) = ctx.execute(script) {
            bail!("Execution error: {:?}", e);
        }

        // Pretty-print the 'output' variable.
        let output_name = bct::text::InternedText::new(&db, S("output"));
        let result = match ctx.pretty_print_variable(output_name) {
            Ok(res) => res,
            Err(e) => bail!("Failed to pretty-print output: {:?}", e),
        };

        println!("{}", result);

        Ok(())
    }
}
