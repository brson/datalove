//! Module compilation from pre-resolved module graphs.
//!
//! This module provides the core compilation that accepts pre-resolved
//! module graphs (output of package resolution) and produces compilation artifacts.
//! It does not depend on package resolution or the interpreter.

use rmx::prelude::*;
use bct::diagnostic::StoredDiagnostic;
use bct::lookup::LookupMap;
use rmx::std::collections::BTreeMap;

use bct::module_graph::{ModuleGraph, ModuleId};
use datalove_datafun_common::warm;
use datalove_datafun_resolve::{
    DbClone, ParallelMode,
    resolve_all_names, resolve_module_names,
    AllModuleNameResolutions,
    ParsedModuleGraph,
};
use datalove_datafun_tycheck::{
    typecheck_graph_module, typecheck_module_graph,
    ModuleGraphTypecheckResult,
};

use crate::module_graph::{parse_module_graph_with_ids, parse_module_full, ModuleIdTable};
use crate::tracked_ownership_analysis::{analysis_work, analyze_module, analyze_module_graph, ModuleGraphAnalysis};

/// Input for module compilation - the output of package resolution.
pub struct ModuleCompilationInput<'db> {
    /// The module graph with modules in dependency order.
    pub graph: ModuleGraph<'db>,
    /// Resolved require aliases per module: (alias, target_module_id).
    pub resolved_requires: LookupMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>,
    /// The number each module's IR is known by, which has one for every
    /// module of the graph and every rider.
    pub module_ids: ModuleIdTable<'db>,
}

/// Output of module compilation - analysis results without lowering.
///
/// Contains parsing, name resolution, typechecking, and ownership analysis results.
/// Callers who need IR lowering should call `lower_module_graph_with_evaluator`
/// after checking for errors.
pub struct ModuleCompilationOutput<'db> {
    /// The input module graph.
    pub module_graph: ModuleGraph<'db>,
    /// Parsed module graph with AST.
    pub parsed_graph: ParsedModuleGraph<'db>,
    /// Name resolution results (type aliases and function signatures).
    pub name_resolution: AllModuleNameResolutions<'db>,
    /// Typecheck results per module.
    pub typecheck_result: ModuleGraphTypecheckResult<'db>,
    /// Ownership analysis results per module.
    pub ownership_analysis: ModuleGraphAnalysis<'db>,
    /// Parse diagnostics by module path, and a rider interface's parse and
    /// resolution diagnostics under `@rider/<name>`.
    pub parse_diagnostics: BTreeMap<String, Vec<StoredDiagnostic>>,
    /// Type diagnostics by module path: those of the module's own check, and
    /// not those of the data files it requires, which only the graph's
    /// accumulated diagnostics hold.
    pub type_diagnostics: BTreeMap<String, Vec<StoredDiagnostic>>,
    /// Ownership diagnostics by module path.
    pub ownership_diagnostics: BTreeMap<String, Vec<StoredDiagnostic>>,
    /// The rider interfaces compiled against, by rider name, whose own
    /// diagnostics are asked of `rider_interface_diagnostics`.
    pub rider_sources: Vec<(String, bct::input::Source)>,
}

impl<'db> ModuleCompilationOutput<'db> {
    /// Check if analysis succeeded (no errors).
    pub fn is_successful(&self) -> bool {
        self.parse_diagnostics.values().all(|e| e.is_empty())
            && self.type_diagnostics.values().all(|e| e.is_empty())
            && self.ownership_diagnostics.values().all(|e| e.is_empty())
    }
}

/// Analyze modules from pre-resolved input.
///
/// Runs parsing, name resolution, typechecking, and ownership analysis.
/// Does NOT run IR lowering. Callers who need IR should call
/// `lower_module_graph_with_evaluator` after checking that `is_successful()` returns true.
///
/// Phases:
/// 1. Parse - lexing/parsing to AST
/// 2. Name Resolution - collect type aliases and function signatures (memoized per-module)
/// 3. Typecheck - type inference and checking
/// 4. Ownership Analysis - borrow checking and drop scheduling
///
/// This separation allows callers to skip lowering entirely when analysis fails,
/// and gives them control over when/if lowering happens.
pub fn compile_modules<'db>(
    db: &'db dyn DbClone,
    input: ModuleCompilationInput<'db>,
    rider_sources: Vec<(String, bct::input::Source)>,
    mode: ParallelMode,
) -> ModuleCompilationOutput<'db> {
    // The data files' large collections, parsed in chunks ahead of the
    // typecheck that asks for the files; see `datalit::parser::parse_chunk`.
    // Planning a file lexes and braces it, which stays sequential.
    warm(db, mode, || data_chunks(db.as_salsa_db(), input.graph.C()), |db, (source, collection, chunk)| {
        let _ = datalove_datalit::parser::parse_chunk(db, source, collection, chunk);
    });

    let parsed_graph = parse_phase(db, mode, input.graph.C(), input.resolved_requires, rider_sources.C(), input.module_ids);
    let name_resolution = resolve_phase(db, mode, parsed_graph);
    let typecheck_result = typecheck_phase(db, mode, parsed_graph, name_resolution);
    let ownership_analysis = ownership_phase(db, mode, parsed_graph, typecheck_result);

    let mut output = collect_results(db, input.graph, parsed_graph, name_resolution, typecheck_result, ownership_analysis);

    // A rider interface that does not parse or resolve stops the compile as a
    // module would: its natives are what it failed to say.
    for (name, source) in &rider_sources {
        let (parse, types) = crate::module_graph::rider_interface_diagnostics(db.as_salsa_db(), *source);
        let diagnostics: Vec<StoredDiagnostic> = parse.iter().map(|d| d.stored().C())
            .chain(types.iter().map(|d| d.stored().C()))
            .collect();
        if !diagnostics.is_empty() {
            output.parse_diagnostics.insert(fmt!("@rider/{name}"), diagnostics);
        }
    }
    output.rider_sources = rider_sources;
    output
}

// Phases 1 to 4, each a sequential step over the graph and, under
// `ParallelMode::Parallel`, the step's per-module queries computed ahead of it
// on rayon (`warm`), from the same function the step makes its own calls with.
// This, the dependency walk before it (`extract_dependencies_with_mode`) and
// phase 5 after (`lower_module_graph_with_evaluator`) are where the compile is
// parallel; the phases themselves know nothing of it.

/// Phase 1: parse. In the module compiler the dependency walk has parsed every
/// module already; a graph built without it, as the benches and tests build
/// theirs, is parsed here, ahead of the phases that read each module's AST.
/// Rider sources are passed through as raw strings; `RiderInterface` creation
/// happens inside the tracked `parse_module_graph_with_ids`, where salsa tracked struct
/// creation is allowed.
pub fn parse_phase<'db>(
    db: &'db dyn DbClone,
    mode: ParallelMode,
    graph: ModuleGraph<'db>,
    resolved_requires: LookupMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>,
    rider_sources: Vec<(String, bct::input::Source)>,
    module_ids: ModuleIdTable<'db>,
) -> ParsedModuleGraph<'db> {
    let db_salsa = db.as_salsa_db();
    warm(db, mode, || graph.iter_modules(db_salsa), |db, module| {
        let _ = parse_module_full(db, module);
    });
    parse_module_graph_with_ids(db_salsa, graph, resolved_requires, rider_sources, module_ids)
}

/// Phase 2: name resolution.
pub fn resolve_phase<'db>(
    db: &'db dyn DbClone,
    mode: ParallelMode,
    parsed_graph: ParsedModuleGraph<'db>,
) -> AllModuleNameResolutions<'db> {
    let db_salsa = db.as_salsa_db();
    let graph = parsed_graph.graph(db_salsa);
    warm(db, mode, || graph.iter_modules(db_salsa), |db, module| {
        let _ = resolve_module_names(db, module);
    });
    resolve_all_names(db_salsa, parsed_graph)
}

/// Phase 3: typecheck, given the names phase 2 resolved.
pub fn typecheck_phase<'db>(
    db: &'db dyn DbClone,
    mode: ParallelMode,
    parsed_graph: ParsedModuleGraph<'db>,
    name_resolution: AllModuleNameResolutions<'db>,
) -> ModuleGraphTypecheckResult<'db> {
    let db_salsa = db.as_salsa_db();
    let graph = parsed_graph.graph(db_salsa);
    warm(db, mode, || graph.iter_modules(db_salsa), |db, module| {
        let _ = typecheck_graph_module(db, parsed_graph, name_resolution, module);
    });
    typecheck_module_graph(db_salsa, parsed_graph, name_resolution)
}

/// Phase 4: ownership analysis.
pub fn ownership_phase<'db>(
    db: &'db dyn DbClone,
    mode: ParallelMode,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
) -> ModuleGraphAnalysis<'db> {
    let db_salsa = db.as_salsa_db();
    warm(db, mode, || analysis_work(db_salsa, parsed_graph, typecheck_result), |db, (module, single_typecheck)| {
        let _ = analyze_module(db, module, single_typecheck);
    });
    analyze_module_graph(db_salsa, parsed_graph, typecheck_result)
}

/// Every chunk of every collection the data files plan to cut, as
/// `datalit::parser::parse_chunk` is keyed.
fn data_chunks<'db>(
    db: &'db dyn salsa::Database,
    graph: ModuleGraph<'db>,
) -> Vec<(bct::input::Source, u32, u32)> {
    let mut chunks = Vec::new();
    for source in graph.data(db).values() {
        let plan = bct::chunk::collection_plan(db, bct::lexer::lex(db, *source));
        for (collection, planned) in plan.iter().enumerate() {
            for chunk in 0..planned.chunks.len() {
                chunks.push((*source, u32::try_from(collection).X(), u32::try_from(chunk).X()));
            }
        }
    }
    chunks
}

/// Collect all analysis results into the output structure.
fn collect_results<'db>(
    db: &'db dyn DbClone,
    module_graph: ModuleGraph<'db>,
    parsed_graph: ParsedModuleGraph<'db>,
    name_resolution: AllModuleNameResolutions<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
) -> ModuleCompilationOutput<'db> {
    let salsa_db = db.as_salsa_db();

    let mut parse_diagnostics: BTreeMap<String, Vec<StoredDiagnostic>> = BTreeMap::new();
    for module in module_graph.iter_modules(salsa_db) {
        let diags = parse_module_full::accumulated::<datalove_diagnostic::ParseDiagnostic>(salsa_db, module);
        if !diags.is_empty() {
            let path = module.id(salsa_db).path(salsa_db).C();
            parse_diagnostics.insert(path, diags.iter().map(|d| d.stored().C()).collect());
        }
    }

    // Every type error is reported by a diagnostic, so these say them all.
    let module_results = typecheck_result.module_results(salsa_db);
    let mut type_diagnostics: BTreeMap<String, Vec<StoredDiagnostic>> = BTreeMap::new();
    for (module_id, result) in module_results.iter_unordered() {
        if result.errors(salsa_db).is_empty() {
            continue;
        }
        let span_lookup = datalove_datafun_tycheck::ModuleGraphSpanLookup::new(&parsed_graph, *module_id);
        let diagnostics = datalove_datafun_tycheck::stored_pending_diagnostics(
            salsa_db, result.pending_diagnostics(salsa_db), &span_lookup);
        type_diagnostics.insert(module_id.path(salsa_db).C(), diagnostics);
    }

    // Filed by the module's own analysis, which is what reads them back.
    let mut ownership_diagnostics: BTreeMap<String, Vec<StoredDiagnostic>> = BTreeMap::new();
    for (module_id, result) in ownership_analysis.module_results(salsa_db).iter_unordered() {
        if result.errors(salsa_db).is_empty() {
            continue;
        }
        let module = parsed_graph.get_module(salsa_db, *module_id)
            .expect("an analyzed module is in its graph");
        let single = *module_results.get(module_id).expect("an analyzed module was typechecked");
        let diagnostics = crate::tracked_ownership_analysis::analyze_module::accumulated::<datalove_diagnostic::OwnershipDiagnostic>(
            salsa_db, module, single);
        ownership_diagnostics.insert(module_id.path(salsa_db).C(), diagnostics.iter().map(|d| d.stored().C()).collect());
    }

    ModuleCompilationOutput {
        module_graph,
        parsed_graph,
        name_resolution,
        typecheck_result,
        ownership_analysis,
        parse_diagnostics,
        type_diagnostics,
        ownership_diagnostics,
        rider_sources: Vec::new(),
    }
}
