
use rmx::prelude::*;

use rmx::clap::{self, Parser as _};
use rmx::std::path::PathBuf;

/// Context for rendering diagnostics with source location information.
///
/// Simplified version for single-file CLI use case.
/// Lives in the driver (CLI) outside of salsa.
struct DiagnosticContext {
    /// Source information (path, display name).
    source_info: SourceInfo,
    /// Original source text (for line:col conversion).
    source_text: String,
}

struct SourceInfo {
    /// File path if the source came from a file.
    _path: Option<PathBuf>,
    /// Display name for rendering (e.g., "file.dfs", "<repl-5>", "<test>").
    display_name: String,
}

impl DiagnosticContext {
    /// Register a file source.
    fn from_file(path: PathBuf, source_text: String) -> Self {
        let display_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        DiagnosticContext {
            source_info: SourceInfo {
                _path: Some(path),
                display_name,
            },
            source_text,
        }
    }

    /// Register a test source (no file path).
    fn _from_test(source_text: String) -> Self {
        DiagnosticContext {
            source_info: SourceInfo {
                _path: None,
                display_name: "<test>".to_string(),
            },
            source_text,
        }
    }

    /// Register a REPL input.
    fn _from_repl(line_num: usize, source_text: String) -> Self {
        DiagnosticContext {
            source_info: SourceInfo {
                _path: None,
                display_name: format!("<repl-{}>", line_num),
            },
            source_text,
        }
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
    /// Typecheck the sys/std library and report errors.
    TypecheckStd(TypecheckStdCommand),
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
struct TypecheckStdCommand {
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
            Command::TypecheckStd(cmd) => cmd.run(&self.args),
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
            let db = datalove_repl::datafun::Database::default();
            datalove_repl::Engine::run_script(&db, script_path)
        } else {
            datalove_repl_term::run()
        }
    }
}

impl ScriptCommand {
    fn run(&self, _args: &Args) -> AnyResult<()> {
        bail!("Script execution has been removed. Use module-based execution instead.")
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

        // Convert to package-agnostic ModuleGraph and typecheck.
        let module_graph = datafun::to_module_graph(&db, package_world, pkg_graph);
        let typecheck_result = datafun::tycheck::typecheck_module_graph(&db, module_graph);

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
