// fixme this is all just busted nonsense

use rmx::prelude::*;

use datalove_datafun_ast::ast;
use datalove_datafun_parser as parser;
use datalove_datafun_ast::script;

/// Result of resolving function declarations in a script.
/// Tracks which units successfully resolved ("green" units).
#[salsa::tracked]
pub struct FunctionResolution<'db> {
    /// Indices of units that successfully resolved.
    #[returns(ref)]
    pub green_units: Vec<usize>,
}

/// Resolve function declarations bidirectionally across the script.
/// This implements the algorithm from notes/script-semantics.md.
#[salsa::tracked]
pub fn resolve_functions<'db>(
    db: &'db dyn salsa::Database,
    script: script::Script,
) -> FunctionResolution<'db> {
    let units = script.units(db);
    let mut green_units = Vec::new();

    // Process each unit in order.
    for unit_index in 0..units.len() {
        let parsed = parser::parse_script_unit(db, script, unit_index);
        let statements = parsed.statements(db);

        // Check if this unit contains function statements.
        let has_fun = statements.iter().any(|stmt| matches!(stmt, ast::Statement::Fun(_)));

        if !has_fun {
            // Non-function units are handled separately by let resolution.
            continue;
        }

        // Collect all function statements from green units + current unit.
        let mut candidate_functions = Vec::new();

        // Add functions from previously green units.
        for &green_idx in &green_units {
            let green_parsed = parser::parse_script_unit(db, script, green_idx);
            for stmt in green_parsed.statements(db) {
                if let ast::Statement::Fun(fun) = stmt {
                    candidate_functions.push((green_idx, *fun));
                }
            }
        }

        // Add functions from current unit.
        for stmt in statements {
            if let ast::Statement::Fun(fun) = stmt {
                candidate_functions.push((unit_index, *fun));
            }
        }

        // Perform name resolution on candidate functions.
        // For now, we just check for duplicate names and mark as green if no duplicates.
        let is_valid = check_function_names(db, &candidate_functions);

        if is_valid {
            green_units.push(unit_index);
        }
        // If not valid, this unit is "dead" and won't be considered in future steps.
    }

    FunctionResolution::new(db, green_units)
}

/// Check function names for conflicts.
/// Returns true if all function names are valid (newer declarations can shadow older ones).
fn check_function_names<'db>(
    _db: &'db dyn salsa::Database,
    _functions: &[(usize, ast::StmtFun<'db>)],
) -> bool {
    true
}

/// Result of resolving a let statement.
#[salsa::tracked]
pub struct LetResolution<'db> {
    /// Whether the let statement resolved successfully.
    pub is_valid: bool,
    /// Error message if resolution failed.
    pub error: Option<bct::text::InternedText<'db>>,
}

/// Resolve a let statement in the context of the script.
/// Let statements can reference:
/// - All green functions (from function resolution)
/// - Previous let statements (but not forward references)
#[salsa::tracked]
pub fn resolve_let_statement<'db>(
    db: &'db dyn salsa::Database,
    script: script::Script,
    unit_index: usize,
) -> LetResolution<'db> {
    let parsed = parser::parse_script_unit(db, script, unit_index);
    let statements = parsed.statements(db);

    // Check that this unit contains a let statement.
    let has_let = statements.iter().any(|stmt| matches!(stmt, ast::Statement::Let(_)));

    if !has_let {
        // Not a let statement unit.
        return LetResolution::new(db, false, None);
    }

    // Get function resolution to know which functions are available.
    let function_resolution = resolve_functions(db, script);
    let green_functions = function_resolution.green_units(db);

    // Collect all available names:
    // 1. Functions from green units
    // 2. Let statements from previous units

    let mut available_names = std::collections::HashSet::new();

    // Add function names.
    for &fun_idx in green_functions {
        let fun_parsed = parser::parse_script_unit(db, script, fun_idx);
        for stmt in fun_parsed.statements(db) {
            if let ast::Statement::Fun(fun) = stmt {
                let name = fun.name(db).as_str(db);
                available_names.insert(name);
            }
        }
    }

    // Add let names from previous units.
    let _units = script.units(db);
    for prev_idx in 0..unit_index {
        let prev_parsed = parser::parse_script_unit(db, script, prev_idx);
        for stmt in prev_parsed.statements(db) {
            if let ast::Statement::Let(let_stmt) = stmt {
                let name = let_stmt.name.as_str(db);
                available_names.insert(name);
            }
        }
    }

    // For now, we just check that there are no parse errors.
    // Real name resolution would check that all names used in the expression are available.
    for stmt in statements {
        if matches!(stmt, ast::Statement::ParseError(_)) {
            return LetResolution::new(db, false, None);
        }
    }

    LetResolution::new(db, true, None)
}

