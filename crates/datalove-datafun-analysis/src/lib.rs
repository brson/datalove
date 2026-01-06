//! Prototype: termination detection and refinement types for datafun.
//!
//! This crate is isolated for easy removal. Enable features to activate:
//! - `termination`: Loop termination analysis via carry tracking
//! - `refinement`: Refinement type checking (division by zero, etc.)

use rmx::std::collections::BTreeMap;
use serde::{Serialize, Deserialize};
use datalove_datafun_tycheck::ParsedModuleGraph;

#[cfg(feature = "termination")]
pub mod termination;

#[cfg(feature = "refinement")]
pub mod refinement;

// Re-export for convenience.
#[cfg(feature = "termination")]
pub use termination::{analyze_function_termination, FunctionTerminationAnalysis};

#[cfg(feature = "refinement")]
pub use refinement::{analyze_function_refinement, FunctionRefinementAnalysis};

/// Result of analyzing an entire module graph.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModuleGraphAnalysis {
    /// Per-function analysis results.
    pub functions: Vec<FunctionAnalysis>,
}

/// Analysis results for a single function.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionAnalysis {
    /// Module path (e.g., "sys/std/u32").
    pub module_path: String,
    /// Function name.
    pub function_name: String,
    /// Termination analysis results.
    #[cfg(feature = "termination")]
    pub termination: FunctionTerminationAnalysis,
    /// Refinement analysis results.
    #[cfg(feature = "refinement")]
    pub refinement: FunctionRefinementAnalysis,
}

/// Analyze all functions in a parsed module graph.
///
/// Skips modules that have typecheck errors.
pub fn analyze_module_graph<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: &ParsedModuleGraph<'db>,
    path_to_errors: &BTreeMap<String, Vec<String>>,
) -> ModuleGraphAnalysis {
    let mut functions = Vec::new();

    let module_graph = parsed_graph.graph(db);
    for module in module_graph.iter_modules(db) {
        let module_id = module.id(db);
        let module_path = module_id.path(db).clone();

        // Skip modules with typecheck errors.
        if path_to_errors.get(&module_path).map_or(false, |e| !e.is_empty()) {
            continue;
        }

        // Get pre-parsed statements from the ParsedModuleGraph.
        let Some(parsed) = parsed_graph.get_parsed(db, module_id) else {
            continue;
        };

        for statement in parsed.statements(db) {
            if let datalove_datafun_ast::ast::Statement::Fun(func) = statement {
                let function_name = func.name(db).text(db).to_string();

                #[cfg(feature = "termination")]
                let termination = analyze_function_termination(db, *func);

                #[cfg(feature = "refinement")]
                let refinement = analyze_function_refinement(db, *func);

                functions.push(FunctionAnalysis {
                    module_path: module_path.clone(),
                    function_name,
                    #[cfg(feature = "termination")]
                    termination,
                    #[cfg(feature = "refinement")]
                    refinement,
                });
            }
        }
    }

    ModuleGraphAnalysis { functions }
}
