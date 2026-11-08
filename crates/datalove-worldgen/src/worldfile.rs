//! Worldfile assembly and entry point.

use rmx::prelude::*;
use rand::{Rng, SeedableRng};
use datalove_datalit::Database;
use crate::config::WorldGenConfig;
use crate::gen_module::{plan_module_graph, gen_module_type_aliases, gen_module_function_sigs, gen_module_generic_sigs, gen_module};
use crate::gen_expr::gen_literal;
use crate::gen_script::gen_script_items;
use crate::pretty::pretty_type_hint;

/// Generate a complete worldfile from a seed.
///
/// Returns the worldfile as a string that can be parsed and typechecked.
pub fn gen_worldfile_seeded(seed: u64, config: WorldGenConfig) -> String {
    let db = Database::default();
    gen_worldfile_tracked(&db, seed, config)
}

/// A generated REPL session: the modules and data it requires, and its
/// script units.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GenSession {
    /// The module and data sections, as worldfile text.
    pub library: String,
    /// Each module's `library/package/module` path and source.
    pub modules: Vec<(String, String)>,
    /// Each data file's `library/package/name` path and text.
    pub data: Vec<(String, String)>,
    /// The script units, in order.
    pub units: Vec<String>,
    /// For each name the script's top level binds, a statement that binds
    /// it again, as `let` or `var` as it was and with a literal of its type:
    /// a shadow of it, wherever it is in scope.
    pub shadows: Vec<(String, String)>,
}

/// Generate a session from a seed: a worldfile's script cut into units.
pub fn gen_session_seeded(seed: u64, config: WorldGenConfig) -> GenSession {
    let db = Database::default();
    gen_session_tracked(&db, seed, config)
}

#[salsa::tracked(returns(clone))]
fn gen_session_tracked<'db>(
    db: &'db dyn salsa::Database,
    seed: u64,
    config: WorldGenConfig,
) -> GenSession {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let (infos, modules) = gen_modules(db, &mut rng, &config);
    let script = gen_script_items(db, &mut rng, &config, &infos);
    let library = library_sections(&modules, &script.data).join("\n\n");
    let (items, variables) = (script.items, script.variables);
    let mut units = Vec::new();
    let mut items = items.into_iter().peekable();
    while items.peek().is_some() {
        let count = rng.gen_range(1..=3);
        units.push(items.by_ref().take(count).collect::<Vec<_>>().join("\n"));
    }

    // Drawn after the units, so asking for them changes none.
    let mut shadows: Vec<(String, String)> = Vec::new();
    for v in variables {
        if shadows.iter().any(|(name, _)| *name == v.name) {
            continue;
        }
        let keyword = if v.is_mutable { "var" } else { "let" };
        let ty = pretty_type_hint(db, v.type_hint.clone());
        let value = gen_literal(db, &mut rng, v.type_hint, &config);
        shadows.push((v.name.clone(), format!("{} {}: {} = {}", keyword, v.name, ty, value)));
    }
    GenSession { library, modules, data: script.data, units, shadows }
}

/// Internal tracked function for worldfile generation.
///
/// Must be tracked to allow creation of Salsa tracked structs.
#[salsa::tracked(returns(clone))]
fn gen_worldfile_tracked<'db>(
    db: &'db dyn salsa::Database,
    seed: u64,
    config: WorldGenConfig,
) -> String {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    gen_worldfile_inner(db, &mut rng, &config)
}

/// Generate a complete worldfile.
fn gen_worldfile_inner<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
) -> String {
    let (infos, modules) = gen_modules(db, rng, config);
    let script = gen_script_items(db, rng, config, &infos);
    let mut sections = library_sections(&modules, &script.data);
    sections.push(format!(
        "----------\nscriptunit-fragment\n----------\n\n{}",
        script.items.join("\n")
    ));

    sections.join("\n\n")
}

/// The worldfile sections of `modules` and `data`, each a path and its text.
fn library_sections(modules: &[(String, String)], data: &[(String, String)]) -> Vec<String> {
    let module = modules.iter().map(|(path, source)| format!("----------\nmodule {}\n----------\n\n{}", path, source));
    let data = data.iter().map(|(path, text)| format!("----------\ndata {}\n----------\n\n{}", path, text));
    module.chain(data).collect()
}

/// Generate the modules, and each one's path and source.
fn gen_modules<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
) -> (Vec<crate::context::ModuleInfo<'db>>, Vec<(String, String)>) {
    let mut sections = Vec::new();

    // Plan module graph.
    let mut modules = plan_module_graph(rng, config);

    // First pass: generate type aliases and function signatures for all modules.
    for (index, module) in modules.iter_mut().enumerate() {
        module.type_aliases = gen_module_type_aliases(db, rng, config);
        module.enums = crate::gen_enum::gen_module_enums(db, rng, config, index);
        module.consts = crate::gen_const::gen_module_consts(db, rng, config, index);
        module.generics = gen_module_generic_sigs(rng, config, index);
        module.functions = gen_module_function_sigs(db, rng, config);
    }

    // Second pass: generate complete module source.
    for i in 0..modules.len() {
        let prior_modules: Vec<_> = modules[..i].to_vec();
        let module = &modules[i];

        let source = gen_module(db, rng, module, config, &prior_modules);

        sections.push((module.path(), source));
    }

    (modules, sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gen_worldfile_deterministic() {
        let config = WorldGenConfig::default();

        let wf1 = gen_worldfile_seeded(42, config.C());
        let wf2 = gen_worldfile_seeded(42, config);

        assert_eq!(wf1, wf2, "Same seed should produce identical worldfiles");
    }

    #[test]
    fn test_gen_worldfile_different_seeds() {
        let config = WorldGenConfig::default();

        let wf1 = gen_worldfile_seeded(1, config.C());
        let wf2 = gen_worldfile_seeded(2, config);

        assert_ne!(wf1, wf2, "Different seeds should produce different worldfiles");
    }

    #[test]
    fn test_gen_worldfile_has_structure() {
        let config = WorldGenConfig::default();
        let wf = gen_worldfile_seeded(123, config);

        // Should have module sections.
        assert!(wf.contains("----------"), "Should have separators");
        assert!(wf.contains("module local/"), "Should have module header");
        assert!(wf.contains("fun "), "Should have function definitions");
        assert!(wf.contains("end fun"), "Should have function end");
    }

    #[test]
    fn test_gen_worldfile_no_panic() {
        let config = WorldGenConfig::default();

        // Test 100 different seeds.
        for seed in 0..100 {
            let _wf = gen_worldfile_seeded(seed, config.C());
        }
    }
}
